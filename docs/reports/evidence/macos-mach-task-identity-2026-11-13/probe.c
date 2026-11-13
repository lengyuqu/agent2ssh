// Isolated macOS task identity experiment. Only this program's own child is
// targeted; no daemon, credentials, signals to unrelated processes, or GUI.
#include <mach/mach.h>
#include <mach/task.h>
#include <mach/mach_types.h>
#include <mach/task_special_ports.h>
#include <mach/notify.h>
#include <bsm/libbsm.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct {
    mach_msg_header_t header;
    mach_msg_body_t body;
    mach_msg_port_descriptor_t token;
    uint32_t marker;
} identity_msg;
typedef struct {
    identity_msg msg;
    mach_msg_audit_trailer_t trailer;
    unsigned char extra[128];
} receive_buf;

static void fatal(const char *where) { perror(where); exit(1); }
static void kr_ok(const char *where, kern_return_t kr) {
    if (kr != KERN_SUCCESS) { fprintf(stderr, "%s: %d (%s)\n", where, kr, mach_error_string(kr)); exit(1); }
}
static void write_byte(int fd, char value) { if (write(fd, &value, 1) != 1) fatal("write"); }
static char read_byte(int fd) { char v; if (read(fd, &v, 1) != 1) fatal("read"); return v; }
static void calibrate_dead_name(void) {
    mach_port_t source = MACH_PORT_NULL, sink = MACH_PORT_NULL, prior = MACH_PORT_NULL;
    kr_ok("calibration source", mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &source));
    kr_ok("calibration send", mach_port_insert_right(mach_task_self(), source, source, MACH_MSG_TYPE_MAKE_SEND));
    kr_ok("calibration sink", mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &sink));
    kr_ok("calibration register", mach_port_request_notification(mach_task_self(), source, MACH_NOTIFY_DEAD_NAME, 1, sink, MACH_MSG_TYPE_MAKE_SEND_ONCE, &prior));
    kr_ok("calibration drop receive", mach_port_mod_refs(mach_task_self(), source, MACH_PORT_RIGHT_RECEIVE, -1));
    struct { mach_msg_header_t header; unsigned char data[128]; } msg = {0};
    kern_return_t kr = mach_msg(&msg.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof(msg), sink, 1000, MACH_PORT_NULL);
    mach_dead_name_notification_t *dead = (void *)&msg;
    printf("dead_name_calibration kr=%d msg_id=%d expected=%d name_matches=%d\n", kr, kr == KERN_SUCCESS ? msg.header.msgh_id : 0, MACH_NOTIFY_DEAD_NAME, kr == KERN_SUCCESS && dead->not_port == source);
    if (kr != KERN_SUCCESS || msg.header.msgh_id != MACH_NOTIFY_DEAD_NAME || dead->not_port != source) exit(10);
    if (MACH_PORT_VALID(prior)) (void)mach_port_deallocate(mach_task_self(), prior);
    (void)mach_port_deallocate(mach_task_self(), source);
    (void)mach_port_mod_refs(mach_task_self(), sink, MACH_PORT_RIGHT_RECEIVE, -1);
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc == 3 && strcmp(argv[1], "--exec-helper") == 0) {
        int fd = atoi(argv[2]);
        write_byte(fd, 'e');
        (void)read_byte(fd);
        close(fd);
        return 0;
    }
    int test_exec = argc == 2 && strcmp(argv[1], "--exec-transition") == 0;
    if (argc > 1 && !test_exec) { fprintf(stderr, "unknown arguments\n"); return 1; }
    printf("parent_pid=%d uid=%d\n", getpid(), getuid());
    calibrate_dead_name();
    int pair[2]; if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair)) fatal("socketpair");
    mach_port_t service = MACH_PORT_NULL;
    kr_ok("allocate", mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &service));
    kr_ok("insert", mach_port_insert_right(mach_task_self(), service, service, MACH_MSG_TYPE_MAKE_SEND));
    // Give only our forked child a send right through the inherited bootstrap
    // special-port slot. This modifies this probe's task only, registers no
    // launchd service, and the parent restores its original port immediately.
    mach_port_t original_bootstrap = MACH_PORT_NULL;
    kr_ok("get bootstrap", task_get_special_port(mach_task_self(), TASK_BOOTSTRAP_PORT, &original_bootstrap));
    kr_ok("set probe bootstrap", task_set_special_port(mach_task_self(), TASK_BOOTSTRAP_PORT, service));
    kern_return_t kr;
    pid_t child = fork(); if (child < 0) fatal("fork");
    if (!child) {
        close(pair[0]);
        mach_port_t destination = MACH_PORT_NULL;
        kr = task_get_special_port(mach_task_self(), TASK_BOOTSTRAP_PORT, &destination);
        if (kr != KERN_SUCCESS) { fprintf(stderr, "get child special port: %d\n", kr); _exit(3); }
        task_id_token_t token = MACH_PORT_NULL;
        kr = task_create_identity_token(mach_task_self(), &token);
        printf("child_pid=%d create_token kr=%d (%s)\n", getpid(), kr, mach_error_string(kr));
        if (kr != KERN_SUCCESS) _exit(4);
        identity_msg message = {0};
        message.header.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0) | MACH_MSGH_BITS_COMPLEX;
        message.header.msgh_remote_port = destination;
        message.header.msgh_size = sizeof(message);
        message.header.msgh_id = 4222;
        message.body.msgh_descriptor_count = 1;
        message.token.name = token;
        message.token.disposition = MACH_MSG_TYPE_MOVE_SEND;
        message.token.type = MACH_MSG_PORT_DESCRIPTOR;
        message.marker = 0xa522;
        kr = mach_msg(&message.header, MACH_SEND_MSG | MACH_SEND_TIMEOUT, sizeof(message), 0, MACH_PORT_NULL, 2000, MACH_PORT_NULL);
        printf("child_send_token kr=%d (%s)\n", kr, mach_error_string(kr));
        if (kr != KERN_SUCCESS) _exit(5);
        write_byte(pair[1], 'a');
        char command = read_byte(pair[1]);
        if (command == 'e') {
            char fdstr[24]; snprintf(fdstr, sizeof(fdstr), "%d", pair[1]);
            execl(argv[0], argv[0], "--exec-helper", fdstr, (char *)NULL);
            fatal("exec helper");
        }
        close(pair[1]);
        _exit(0);
    }
    kr_ok("restore bootstrap", task_set_special_port(mach_task_self(), TASK_BOOTSTRAP_PORT, original_bootstrap));
    (void)mach_port_deallocate(mach_task_self(), original_bootstrap);
    close(pair[1]);
    receive_buf buffer = {0};
    mach_msg_option_t options = MACH_RCV_MSG | MACH_RCV_TIMEOUT | MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0) | MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT);
    kr = mach_msg(&buffer.msg.header, options, 0, sizeof(buffer), service, 4000, MACH_PORT_NULL);
    printf("parent_receive kr=%d (%s)\n", kr, mach_error_string(kr));
    if (kr != KERN_SUCCESS) { close(pair[0]); waitpid(child, NULL, 0); return 6; }
    if (!(buffer.msg.header.msgh_bits & MACH_MSGH_BITS_COMPLEX) || buffer.msg.body.msgh_descriptor_count != 1 || buffer.msg.token.type != MACH_MSG_PORT_DESCRIPTOR || buffer.msg.marker != 0xa522) {
        fprintf(stderr, "unexpected Mach message shape\n"); close(pair[0]); waitpid(child, NULL, 0); return 7;
    }
    mach_msg_audit_trailer_t *trailer = (void *)((char *)&buffer.msg + round_msg(buffer.msg.header.msgh_size));
    if (trailer->msgh_trailer_size < sizeof(*trailer)) { fprintf(stderr, "short audit trailer\n"); close(pair[0]); waitpid(child, NULL, 0); return 8; }
    audit_token_t message_audit = trailer->msgh_audit;
    (void)read_byte(pair[0]);
    audit_token_t socket_audit = {0}; socklen_t token_len = sizeof(socket_audit);
    errno = 0; int peer_rc = getsockopt(pair[0], SOL_LOCAL, LOCAL_PEERTOKEN, &socket_audit, &token_len); int peer_errno = errno;
    int peer_pid = -1; socklen_t pid_len = sizeof(peer_pid);
    int peer_pid_rc = getsockopt(pair[0], SOL_LOCAL, LOCAL_PEERPID, &peer_pid, &pid_len);
    printf("mach_sender_pid=%d sender_pidversion=%d expected_child=%d\n", audit_token_to_pid(message_audit), audit_token_to_pidversion(message_audit), child);
    printf("unix_peer_rc=%d errno=%d len=%u peer_pid_rc=%d peer_pid=%d", peer_rc, peer_errno, token_len, peer_pid_rc, peer_pid);
    if (!peer_rc && token_len == sizeof(socket_audit)) printf(" audit_pid=%d audit_pidversion=%d audit_equal=%d", audit_token_to_pid(socket_audit), audit_token_to_pidversion(socket_audit), memcmp(&message_audit, &socket_audit, sizeof(socket_audit)) == 0);
    putchar('\n');
    task_id_token_t received = buffer.msg.token.name;
    mach_port_t ports[4] = {MACH_PORT_NULL, MACH_PORT_NULL, MACH_PORT_NULL, MACH_PORT_NULL};
    for (unsigned f = 0; f < 4; f++) {
        kr = task_identity_token_get_task_port(received, f, &ports[f]);
        printf("live_convert flavor=%u kr=%d (%s) port_valid=%d", f, kr, mach_error_string(kr), MACH_PORT_VALID(ports[f]));
        if (kr == KERN_SUCCESS && MACH_PORT_VALID(ports[f])) {
            int pid = -1; kern_return_t pk = pid_for_task(ports[f], &pid);
            audit_token_t task_audit = {0}; mach_msg_type_number_t count = TASK_AUDIT_TOKEN_COUNT;
            kern_return_t ak = task_info(ports[f], TASK_AUDIT_TOKEN, (task_info_t)&task_audit, &count);
            printf(" pid_for_task_kr=%d pid=%d task_audit_kr=%d count=%u", pk, pid, ak, count);
            if (ak == KERN_SUCCESS) printf(" task_audit_pid=%d task_audit_pidversion=%d matches_sender=%d", audit_token_to_pid(task_audit), audit_token_to_pidversion(task_audit), memcmp(&task_audit, &message_audit, sizeof(task_audit)) == 0);
        }
        putchar('\n');
    }
    mach_port_type_t before = 0;
    kr = mach_port_type(mach_task_self(), received, &before);
    printf("token_before_exit kr=%d type=0x%x\n", kr, before);
    mach_port_t notify_token = MACH_PORT_NULL, notify_name = MACH_PORT_NULL, previous_token = MACH_PORT_NULL, previous_name = MACH_PORT_NULL;
    kr_ok("allocate token notification port", mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &notify_token));
    kr_ok("allocate name notification port", mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &notify_name));
    kern_return_t nt = mach_port_request_notification(mach_task_self(), received, MACH_NOTIFY_DEAD_NAME, 1, notify_token, MACH_MSG_TYPE_MAKE_SEND_ONCE, &previous_token);
    kern_return_t nn = MACH_PORT_VALID(ports[TASK_FLAVOR_NAME]) ? mach_port_request_notification(mach_task_self(), ports[TASK_FLAVOR_NAME], MACH_NOTIFY_DEAD_NAME, 1, notify_name, MACH_MSG_TYPE_MAKE_SEND_ONCE, &previous_name) : KERN_INVALID_NAME;
    printf("dead_name_registration token_kr=%d name_kr=%d\n", nt, nn);
    struct { mach_msg_header_t header; unsigned char data[256]; } note = {0};
    kr = mach_msg(&note.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof(note), notify_name, 50, MACH_PORT_NULL);
    printf("name_notification_while_alive kr=%d (%s) msg_id=%d\n", kr, mach_error_string(kr), kr == KERN_SUCCESS ? note.header.msgh_id : 0);
    if (kr == KERN_SUCCESS) { fprintf(stderr, "unexpected name-port notification while child is alive\n"); close(pair[0]); waitpid(child, NULL, 0); return 11; }
    if (test_exec) {
        write_byte(pair[0], 'e');
        if (read_byte(pair[0]) != 'e') { fprintf(stderr, "exec helper did not start\n"); close(pair[0]); waitpid(child, NULL, 0); return 12; }
        audit_token_t post_exec = {0}; mach_msg_type_number_t count = TASK_AUDIT_TOKEN_COUNT;
        kern_return_t ak = MACH_PORT_VALID(ports[TASK_FLAVOR_NAME]) ? task_info(ports[TASK_FLAVOR_NAME], TASK_AUDIT_TOKEN, (task_info_t)&post_exec, &count) : KERN_INVALID_NAME;
        printf("exec_helper_alive pid=%d audit_kr=%d", child, ak);
        if (ak == KERN_SUCCESS) printf(" audit_pid=%d audit_pidversion=%d same_audit=%d", audit_token_to_pid(post_exec), audit_token_to_pidversion(post_exec), memcmp(&post_exec, &message_audit, sizeof(post_exec)) == 0);
        putchar('\n');
        memset(&note, 0, sizeof(note));
        kr = mach_msg(&note.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof(note), notify_name, 200, MACH_PORT_NULL);
        mach_dead_name_notification_t *exec_note = (void *)&note;
        printf("name_notification_after_exec_while_alive kr=%d (%s) msg_id=%d name_matches=%d\n", kr, mach_error_string(kr), kr == KERN_SUCCESS ? note.header.msgh_id : 0, kr == KERN_SUCCESS && note.header.msgh_id == MACH_NOTIFY_DEAD_NAME && exec_note->not_port == ports[TASK_FLAVOR_NAME]);
        int child_status=0; pid_t waited=waitpid(child, &child_status, WNOHANG);
        printf("exec_child_waitpid_nohang=%d still_running=%d\n", waited, waited == 0);
        task_name_t retry = MACH_PORT_NULL;
        kr = task_identity_token_get_task_port(received, TASK_FLAVOR_NAME, &retry);
        printf("exec_old_token_convert_name kr=%d (%s) port_valid=%d\n", kr, mach_error_string(kr), MACH_PORT_VALID(retry));
        if (MACH_PORT_VALID(retry)) (void)mach_port_deallocate(mach_task_self(), retry);
        audit_token_t new_peer={0}; socklen_t peer_len=sizeof(new_peer);
        int peer_ok=getsockopt(pair[0], SOL_LOCAL, LOCAL_PEERTOKEN, &new_peer, &peer_len);
        printf("exec_socket_audit rc=%d", peer_ok);
        if (!peer_ok && peer_len==sizeof(new_peer)) printf(" pid=%d pidversion=%d equals_original=%d",audit_token_to_pid(new_peer),audit_token_to_pidversion(new_peer),memcmp(&new_peer,&message_audit,sizeof(new_peer))==0);
        putchar('\n');
    }
    write_byte(pair[0], 'x'); close(pair[0]);
    int status = 0; if (waitpid(child, &status, 0) != child || !WIFEXITED(status) || WEXITSTATUS(status) != 0) { fprintf(stderr,"child failed status=%d\n",status); return 9; }
    mach_port_type_t after = 0; kr = mach_port_type(mach_task_self(), received, &after);
    printf("token_after_exit kr=%d type=0x%x\n", kr, after);
    for (unsigned f = 0; f < 4; f++) {
        mach_port_t p = MACH_PORT_NULL;
        kr = task_identity_token_get_task_port(received, f, &p);
        printf("exited_convert flavor=%u kr=%d (%s) port_valid=%d\n", f, kr, mach_error_string(kr), MACH_PORT_VALID(p));
        if (MACH_PORT_VALID(p)) (void)mach_port_deallocate(mach_task_self(), p);
        if (MACH_PORT_VALID(ports[f])) {
            mach_port_type_t pt = 0; kern_return_t tk = mach_port_type(mach_task_self(), ports[f], &pt);
            int pid = -1; kern_return_t pk = pid_for_task(ports[f], &pid);
            printf("retained_port_after_exit flavor=%u type_kr=%d type=0x%x pid_kr=%d pid=%d\n", f, tk, pt, pk, pid);
        }
    }
    memset(&note, 0, sizeof(note));
    kr = mach_msg(&note.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof(note), notify_token, 300, MACH_PORT_NULL);
    printf("token_notification_after_exit kr=%d (%s) msg_id=%d\n", kr, mach_error_string(kr), kr == KERN_SUCCESS ? note.header.msgh_id : 0);
    memset(&note, 0, sizeof(note));
    kr = mach_msg(&note.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof(note), notify_name, 300, MACH_PORT_NULL);
    mach_dead_name_notification_t *dead = (void *)&note;
    printf("name_notification_after_exit kr=%d (%s) msg_id=%d name_matches=%d\n", kr, mach_error_string(kr), kr == KERN_SUCCESS ? note.header.msgh_id : 0, kr == KERN_SUCCESS && note.header.msgh_id == MACH_NOTIFY_DEAD_NAME && dead->not_port == ports[TASK_FLAVOR_NAME]);
    mach_port_type_t name_after_note = 0;
    kern_return_t name_type_kr = MACH_PORT_VALID(ports[TASK_FLAVOR_NAME]) ? mach_port_type(mach_task_self(), ports[TASK_FLAVOR_NAME], &name_after_note) : KERN_INVALID_NAME;
    printf("name_after_notification type_kr=%d type=0x%x is_dead_name=%d\n", name_type_kr, name_after_note, name_type_kr == KERN_SUCCESS && !!(name_after_note & MACH_PORT_TYPE_DEAD_NAME));
    for (unsigned f = 0; f < 4; f++) if (MACH_PORT_VALID(ports[f])) (void)mach_port_deallocate(mach_task_self(), ports[f]);
    if (MACH_PORT_VALID(previous_token)) (void)mach_port_deallocate(mach_task_self(), previous_token);
    if (MACH_PORT_VALID(previous_name)) (void)mach_port_deallocate(mach_task_self(), previous_name);
    (void)mach_port_mod_refs(mach_task_self(), notify_token, MACH_PORT_RIGHT_RECEIVE, -1);
    (void)mach_port_mod_refs(mach_task_self(), notify_name, MACH_PORT_RIGHT_RECEIVE, -1);
    (void)mach_port_deallocate(mach_task_self(), received);
    (void)mach_port_mod_refs(mach_task_self(), service, MACH_PORT_RIGHT_RECEIVE, -1);
    (void)mach_port_deallocate(mach_task_self(), service);
    return 0;
}
