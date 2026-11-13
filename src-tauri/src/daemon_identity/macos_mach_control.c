#include "macos_mach_control.h"
#include "macos_codesign.h"
#include "macos_exit_observer.h"
#include <mach/mach.h>
#include <mach/task.h>
#include <servers/bootstrap.h>
#include <bsm/libbsm.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define A2S_HELLO 0xA231
#define A2S_HELLO_REPLY 0xA232
#define A2S_STOP 0xA233
#define A2S_STOP_REPLY 0xA234
#define A2S_TOKEN_CAP 128

typedef struct {
    mach_msg_header_t h;
    mach_msg_body_t body;
    mach_msg_port_descriptor_t reply;
    uint8_t nonce[16];
    char instance[33];
    uint32_t expected_pid;
} a2s_hello;
typedef struct {
    mach_msg_header_t h;
    mach_msg_body_t body;
    mach_msg_port_descriptor_t identity, endpoint;
    uint8_t nonce[16];
    char instance[33];
    uint32_t pid;
} a2s_hello_reply;
typedef struct {
    mach_msg_header_t h;
    mach_msg_body_t body;
    mach_msg_port_descriptor_t reply;
    uint8_t nonce[16];
    char instance[33];
    uint32_t expected_pid;
    char token[A2S_TOKEN_CAP];
} a2s_stop;
typedef struct {
    mach_msg_header_t h;
    uint8_t nonce[16];
    uint32_t accepted;
    uint32_t pid;
} a2s_stop_reply;
typedef union {
    mach_msg_header_t h;
    a2s_hello hello;
    a2s_stop stop;
    uint8_t bytes[4096];
} a2s_receive;
typedef union {
    mach_msg_header_t h;
    a2s_hello_reply hello;
    a2s_stop_reply stop;
    uint8_t bytes[4096];
} a2s_response;

static mach_port_t listener = MACH_PORT_NULL;
static mach_port_t discovery = MACH_PORT_NULL;
static mach_port_t receive_port = MACH_PORT_NULL;
static char instance_id[33];
static char token_buf[A2S_TOKEN_CAP];
static struct {
    int active;
    uint8_t nonce[16];
    audit_token_t client;
    uint64_t expires_ms;
} sessions[32];

// Defined weakly in a separate translation unit so a release build cannot
// inline the empty body. The daemon binary supplies the real callback.
extern void a2s_mach_control_notify_shutdown(void);

static uint64_t monotonic_ms(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return 0;
    return (uint64_t)now.tv_sec * 1000u + (uint64_t)now.tv_nsec / 1000000u;
}

static int hex_id(const char *value, size_t len) {
    if (!value || strlen(value) != len) return 0;
    for (size_t i = 0; i < len; i++) {
        char c = value[i];
        if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'))) return 0;
    }
    return 1;
}

static int service_ok(const char *service) {
    static const char prefix[] = "org.agent2ssh.";
    return service && strncmp(service, prefix, sizeof(prefix) - 1) == 0 && hex_id(service + sizeof(prefix) - 1, 32);
}

static int same_token(const char *got) {
    unsigned diff = 0;
    for (size_t i = 0; i < A2S_TOKEN_CAP; i++) diff |= (unsigned char)got[i] ^ (unsigned char)token_buf[i];
    return diff == 0;
}

static mach_msg_audit_trailer_t *trailer_of(void *bytes, size_t cap, mach_msg_header_t *header) {
    size_t start = round_msg(header->msgh_size);
    if (start + sizeof(mach_msg_audit_trailer_t) > cap) return NULL;
    mach_msg_audit_trailer_t *trailer = (void *)((uint8_t *)bytes + start);
    return trailer->msgh_trailer_size >= sizeof(*trailer) ? trailer : NULL;
}

static void accept_hello(a2s_hello *req, audit_token_t sender) {
    if (req->h.msgh_size != sizeof(*req) || !(req->h.msgh_bits & MACH_MSGH_BITS_COMPLEX) ||
        req->body.msgh_descriptor_count != 1 || req->reply.type != MACH_MSG_PORT_DESCRIPTOR ||
        req->reply.disposition != MACH_MSG_TYPE_PORT_SEND_ONCE || !MACH_PORT_VALID(req->reply.name) ||
        req->expected_pid != (uint32_t)getpid() || memcmp(req->instance, instance_id, sizeof(instance_id)))
        return;
    uint64_t now = monotonic_ms();
    if (!now) return;
    unsigned slot = 32;
    for (unsigned i = 0; i < 32; i++) {
        if (sessions[i].active && sessions[i].expires_ms <= now) sessions[i].active = 0;
        if (!sessions[i].active && slot == 32) slot = i;
    }
    if (slot == 32) return;
    task_id_token_t identity = MACH_PORT_NULL;
    if (task_create_identity_token(mach_task_self(), &identity) != KERN_SUCCESS) return;
    a2s_hello_reply resp = {0};
    resp.h.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_MOVE_SEND_ONCE, 0) | MACH_MSGH_BITS_COMPLEX;
    resp.h.msgh_remote_port = req->reply.name;
    resp.h.msgh_size = sizeof(resp);
    resp.h.msgh_id = A2S_HELLO_REPLY;
    resp.body.msgh_descriptor_count = 2;
    req->reply.name = MACH_PORT_NULL;
    resp.identity.name = identity;
    resp.identity.disposition = MACH_MSG_TYPE_MOVE_SEND;
    resp.identity.type = MACH_MSG_PORT_DESCRIPTOR;
    resp.endpoint.name = listener;
    resp.endpoint.disposition = MACH_MSG_TYPE_MAKE_SEND;
    resp.endpoint.type = MACH_MSG_PORT_DESCRIPTOR;
    memcpy(resp.nonce, req->nonce, sizeof(resp.nonce));
    memcpy(resp.instance, instance_id, sizeof(instance_id));
    resp.pid = (uint32_t)getpid();
    sessions[slot].active = 1;
    sessions[slot].client = sender;
    sessions[slot].expires_ms = now + 10000;
    memcpy(sessions[slot].nonce, req->nonce, 16);
    if (mach_msg(&resp.h, MACH_SEND_MSG | MACH_SEND_TIMEOUT, sizeof(resp), 0, 0, 1000, 0) != KERN_SUCCESS) {
        sessions[slot].active = 0;
        mach_msg_destroy(&resp.h);
    }
}

static void accept_stop(a2s_stop *req, audit_token_t sender) {
    if (req->h.msgh_size != sizeof(*req) || !(req->h.msgh_bits & MACH_MSGH_BITS_COMPLEX) ||
        req->body.msgh_descriptor_count != 1 || req->reply.type != MACH_MSG_PORT_DESCRIPTOR ||
        req->reply.disposition != MACH_MSG_TYPE_PORT_SEND_ONCE || !MACH_PORT_VALID(req->reply.name) ||
        req->expected_pid != (uint32_t)getpid() || memcmp(req->instance, instance_id, sizeof(instance_id)))
        return;
    uint64_t now = monotonic_ms();
    if (!now) return;
    unsigned slot = 32;
    for (unsigned i = 0; i < 32; i++) {
        if (sessions[i].active && sessions[i].expires_ms > now && !memcmp(sessions[i].nonce, req->nonce, 16) &&
            !memcmp(&sessions[i].client, &sender, sizeof(sender))) {
            slot = i;
            break;
        }
    }
    if (slot == 32 || !same_token(req->token)) return;
    sessions[slot].active = 0;
    a2s_stop_reply resp = {0};
    resp.h.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_MOVE_SEND_ONCE, 0);
    resp.h.msgh_remote_port = req->reply.name;
    resp.h.msgh_size = sizeof(resp);
    resp.h.msgh_id = A2S_STOP_REPLY;
    memcpy(resp.nonce, req->nonce, 16);
    resp.accepted = 1;
    resp.pid = (uint32_t)getpid();
    req->reply.name = MACH_PORT_NULL;
    if (mach_msg(&resp.h, MACH_SEND_MSG | MACH_SEND_TIMEOUT, sizeof(resp), 0, 0, 1000, 0) != KERN_SUCCESS)
        mach_msg_destroy(&resp.h);
    a2s_mach_control_notify_shutdown();
}

static void *serve(void *unused) {
    (void)unused;
    for (;;) {
        a2s_receive msg = {0};
        mach_msg_option_t opts = MACH_RCV_MSG | MACH_RCV_TIMEOUT | MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0) |
                                 MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT);
        kern_return_t kr = mach_msg(&msg.h, opts, 0, sizeof(msg), receive_port, 1000, 0);
        if (kr == MACH_RCV_TIMED_OUT) continue;
        if (kr == MACH_RCV_TOO_LARGE) continue;
        if (kr != KERN_SUCCESS) break;
        mach_msg_audit_trailer_t *trailer = trailer_of(msg.bytes, sizeof(msg), &msg.h);
        if (!trailer || audit_token_to_euid(trailer->msgh_audit) != geteuid()) {
            mach_msg_destroy(&msg.h);
            continue;
        }
        if (msg.h.msgh_id == A2S_HELLO && msg.h.msgh_local_port == discovery)
            accept_hello(&msg.hello, trailer->msgh_audit);
        else if (msg.h.msgh_id == A2S_STOP && msg.h.msgh_local_port == listener)
            accept_stop(&msg.stop, trailer->msgh_audit);
        mach_msg_destroy(&msg.h);
    }
    return NULL;
}

int a2s_mach_control_start(const char *service, const char *instance, const char *token) {
    size_t token_len = token ? strlen(token) : 0;
    if (!service_ok(service) || !hex_id(instance, 32) || token_len == 0 || token_len >= A2S_TOKEN_CAP) return -1;
    if (listener != MACH_PORT_NULL) return -1;
    memcpy(instance_id, instance, 32);
    instance_id[32] = '\0';
    memset(token_buf, 0, sizeof(token_buf));
    memcpy(token_buf, token, token_len);
    if (mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &listener) != KERN_SUCCESS) return -2;
    if (mach_port_guard_with_flags(mach_task_self(), listener, 0xa531, MPG_IMMOVABLE_RECEIVE) != KERN_SUCCESS) return -3;
    if (mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &discovery) != KERN_SUCCESS) return -2;
    if (mach_port_guard_with_flags(mach_task_self(), discovery, 0xb531, MPG_IMMOVABLE_RECEIVE) != KERN_SUCCESS) return -3;
    if (mach_port_insert_right(mach_task_self(), discovery, discovery, MACH_MSG_TYPE_MAKE_SEND) != KERN_SUCCESS) return -4;
    if (bootstrap_register(bootstrap_port, (char *)service, discovery) != KERN_SUCCESS) return -5;
    if (mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_PORT_SET, &receive_port) != KERN_SUCCESS) return -2;
    if (mach_port_move_member(mach_task_self(), listener, receive_port) != KERN_SUCCESS ||
        mach_port_move_member(mach_task_self(), discovery, receive_port) != KERN_SUCCESS)
        return -4;
    pthread_t thread;
    if (pthread_create(&thread, NULL, serve, NULL) != 0) return -6;
    pthread_detach(thread);
    return 0;
}

static int receive(mach_port_t port, a2s_response *msg) {
    memset(msg, 0, sizeof(*msg));
    mach_msg_option_t opts = MACH_RCV_MSG | MACH_RCV_TIMEOUT | MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0) |
                             MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT);
    return mach_msg(&msg->h, opts, 0, sizeof(*msg), port, 2500, 0);
}

int a2s_mach_control_stop(const char *service, const char *instance, uint32_t pid, const char *token, int *token_sent) {
    if (token_sent) *token_sent = 0;
    size_t token_len = token ? strlen(token) : 0;
    if (!service_ok(service) || !hex_id(instance, 32) || pid == 0 || token_len == 0 || token_len >= A2S_TOKEN_CAP || !token_sent)
        return 1;
    mach_port_t looked_up = MACH_PORT_NULL, reply = MACH_PORT_NULL;
    if (bootstrap_look_up(bootstrap_port, (char *)service, &looked_up) != KERN_SUCCESS || !MACH_PORT_VALID(looked_up)) return 1;
    if (mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &reply) != KERN_SUCCESS ||
        mach_port_guard_with_flags(mach_task_self(), reply, 0xc531, MPG_IMMOVABLE_RECEIVE) != KERN_SUCCESS)
        return 1;
    a2s_hello hello = {0};
    hello.h.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0) | MACH_MSGH_BITS_COMPLEX;
    hello.h.msgh_remote_port = looked_up;
    hello.h.msgh_size = sizeof(hello);
    hello.h.msgh_id = A2S_HELLO;
    hello.body.msgh_descriptor_count = 1;
    hello.reply.name = reply;
    hello.reply.disposition = MACH_MSG_TYPE_MAKE_SEND_ONCE;
    hello.reply.type = MACH_MSG_PORT_DESCRIPTOR;
    arc4random_buf(hello.nonce, sizeof(hello.nonce));
    memcpy(hello.instance, instance, 32);
    hello.expected_pid = pid;
    if (mach_msg(&hello.h, MACH_SEND_MSG | MACH_SEND_TIMEOUT, sizeof(hello), 0, 0, 1000, 0) != KERN_SUCCESS) {
        mach_msg_destroy(&hello.h);
        mach_port_deallocate(mach_task_self(), looked_up);
        return 1;
    }
    a2s_response resp;
    if (receive(reply, &resp) != KERN_SUCCESS || resp.h.msgh_id != A2S_HELLO_REPLY || resp.h.msgh_size != sizeof(a2s_hello_reply) ||
        !(resp.h.msgh_bits & MACH_MSGH_BITS_COMPLEX) || resp.hello.body.msgh_descriptor_count != 2 ||
        resp.hello.identity.type != MACH_MSG_PORT_DESCRIPTOR || resp.hello.endpoint.type != MACH_MSG_PORT_DESCRIPTOR ||
        resp.hello.identity.disposition != MACH_MSG_TYPE_PORT_SEND || resp.hello.endpoint.disposition != MACH_MSG_TYPE_PORT_SEND) {
        if (resp.h.msgh_bits & MACH_MSGH_BITS_COMPLEX) mach_msg_destroy(&resp.h);
        mach_port_deallocate(mach_task_self(), looked_up);
        return 1;
    }
    mach_msg_audit_trailer_t *audit = trailer_of(resp.bytes, sizeof(resp), &resp.h);
    task_name_t name = MACH_PORT_NULL;
    mach_port_t identity = resp.hello.identity.name;
    mach_port_t endpoint = resp.hello.endpoint.name;
    if (!audit || audit_token_to_euid(audit->msgh_audit) != geteuid() || audit_token_to_pid(audit->msgh_audit) != (pid_t)pid ||
        resp.hello.pid != pid || memcmp(resp.hello.nonce, hello.nonce, 16) || memcmp(resp.hello.instance, hello.instance, 33) ||
        !MACH_PORT_VALID(endpoint) || endpoint == looked_up ||
        task_identity_token_get_task_port(identity, TASK_FLAVOR_NAME, &name) != KERN_SUCCESS ||
        !a2s_task_audit_matches(&audit->msgh_audit, name, (pid_t)pid)) {
        if (MACH_PORT_VALID(identity) || MACH_PORT_VALID(endpoint)) mach_msg_destroy(&resp.h);
        if (MACH_PORT_VALID(name)) mach_port_deallocate(mach_task_self(), name);
        mach_port_deallocate(mach_task_self(), looked_up);
        return 1;
    }
    a2s_exit_observer *observer = NULL;
    audit_token_t sender = audit->msgh_audit;
    if (a2s_exit_observer_create(&sender, identity, (pid_t)pid, &observer) != 0) {
        mach_port_deallocate(mach_task_self(), identity);
        mach_port_deallocate(mach_task_self(), endpoint);
        mach_port_deallocate(mach_task_self(), name);
        mach_port_deallocate(mach_task_self(), looked_up);
        return 1;
    }
    mach_port_deallocate(mach_task_self(), identity);
    a2s_stop stop = {0};
    stop.h.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0) | MACH_MSGH_BITS_COMPLEX;
    stop.h.msgh_remote_port = endpoint;
    stop.h.msgh_size = sizeof(stop);
    stop.h.msgh_id = A2S_STOP;
    stop.body.msgh_descriptor_count = 1;
    stop.reply.name = reply;
    stop.reply.disposition = MACH_MSG_TYPE_MAKE_SEND_ONCE;
    stop.reply.type = MACH_MSG_PORT_DESCRIPTOR;
    memcpy(stop.nonce, hello.nonce, 16);
    memcpy(stop.instance, hello.instance, 33);
    stop.expected_pid = pid;
    memcpy(stop.token, token, token_len);
    *token_sent = 1;
    if (mach_msg(&stop.h, MACH_SEND_MSG | MACH_SEND_TIMEOUT, sizeof(stop), 0, 0, 1000, 0) != KERN_SUCCESS) {
        mach_msg_destroy(&stop.h);
        a2s_exit_observer_destroy(observer);
        mach_port_deallocate(mach_task_self(), name);
        mach_port_deallocate(mach_task_self(), endpoint);
        mach_port_deallocate(mach_task_self(), looked_up);
        return 2;
    }
    int outcome = 2;
    if (receive(reply, &resp) == KERN_SUCCESS && resp.h.msgh_id == A2S_STOP_REPLY && resp.h.msgh_size == sizeof(a2s_stop_reply)) {
        mach_msg_audit_trailer_t *again = trailer_of(resp.bytes, sizeof(resp), &resp.h);
        if (again && memcmp(&sender, &again->msgh_audit, sizeof(sender)) == 0 && memcmp(resp.stop.nonce, hello.nonce, 16) == 0 &&
            resp.stop.pid == pid && resp.stop.accepted && a2s_exit_observer_wait(observer, 8000) == 1)
            outcome = 0;
    }
    a2s_exit_observer_destroy(observer);
    mach_port_deallocate(mach_task_self(), name);
    mach_port_deallocate(mach_task_self(), endpoint);
    mach_port_deallocate(mach_task_self(), looked_up);
    return outcome;
}
