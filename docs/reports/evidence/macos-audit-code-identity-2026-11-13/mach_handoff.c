// A code-authenticated Mach sender can delegate an unguarded receive right.
// Only this probe's children, Mach ports and a synthetic integer are used.
#include <mach/mach.h>
#include <mach/task.h>
#include <mach/task_special_ports.h>
#include <Security/Security.h>
#include <CoreFoundation/CoreFoundation.h>
#include <bsm/libbsm.h>
#include <sys/wait.h>
#include <signal.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { mach_msg_header_t h; mach_msg_body_t body; mach_msg_port_descriptor_t port; int marker; } one_t;
typedef struct { mach_msg_header_t h; mach_msg_body_t body; mach_msg_port_descriptor_t token, endpoint; int marker; } two_t;
typedef struct { mach_msg_header_t h; int marker; } plain_t;
typedef union { one_t one; two_t two; plain_t plain; unsigned char bytes[1024]; } buffer_t;

static void ok(const char *where, kern_return_t kr) {
    if (kr) { fprintf(stderr, "%s: %d %s\n", where, kr, mach_error_string(kr)); exit(1); }
}
static void put(int fd, char c) { if (write(fd, &c, 1) != 1) { perror("write"); exit(1); } }
static char get(int fd) { char c; if (read(fd, &c, 1) != 1) { perror("read"); exit(1); } return c; }
static mach_port_t new_receive(void) { mach_port_t p=0; ok("allocate", mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &p)); return p; }
static void send_one(mach_port_t dest, mach_port_t port, int disposition, int marker) {
    one_t m={0}; m.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;
    m.h.msgh_remote_port=dest; m.h.msgh_size=sizeof(m); m.h.msgh_id=4700+marker;
    m.body.msgh_descriptor_count=1; m.port.name=port;
    m.port.disposition=(mach_msg_type_name_t)disposition; m.port.type=MACH_MSG_PORT_DESCRIPTOR; m.marker=marker;
    ok("send one", mach_msg(&m.h, MACH_SEND_MSG|MACH_SEND_TIMEOUT, sizeof(m),0,0,2000,0));
}
static void send_plain(mach_port_t dest, int id, int marker) {
    plain_t m={0}; m.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0);
    m.h.msgh_remote_port=dest; m.h.msgh_size=sizeof(m); m.h.msgh_id=id; m.marker=marker;
    ok("send plain", mach_msg(&m.h, MACH_SEND_MSG|MACH_SEND_TIMEOUT, sizeof(m),0,0,2000,0));
}
static kern_return_t receive_for(mach_port_t port, buffer_t *b, mach_msg_timeout_t timeout) {
    memset(b,0,sizeof(*b));
    mach_msg_option_t opts=MACH_RCV_MSG|MACH_RCV_TIMEOUT|MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0)|MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT);
    return mach_msg(&b->one.h,opts,0,sizeof(*b),port,timeout,0);
}
static void receive(mach_port_t port, buffer_t *b) { ok("receive",receive_for(port,b,4000)); }
static audit_token_t message_audit(buffer_t *b) {
    mach_msg_audit_trailer_t *trailer=(void*)(b->bytes+round_msg(b->one.h.msgh_size));
    if(trailer->msgh_trailer_size<sizeof(*trailer)) { fprintf(stderr,"short trailer\n"); exit(2); }
    return trailer->msgh_audit;
}
static OSStatus code_check(audit_token_t token, const char *requirement) {
    CFDataRef data=CFDataCreate(NULL,(UInt8*)&token,sizeof(token));
    const void *keys[]={kSecGuestAttributeAudit}, *values[]={data};
    CFDictionaryRef attrs=CFDictionaryCreate(NULL,keys,values,1,&kCFTypeDictionaryKeyCallBacks,&kCFTypeDictionaryValueCallBacks);
    SecCodeRef code=NULL; OSStatus result=SecCodeCopyGuestWithAttributes(NULL,attrs,kSecCSDefaultFlags,&code);
    if(!result) {
        CFStringRef text=CFStringCreateWithCString(NULL,requirement,kCFStringEncodingUTF8);
        SecRequirementRef req=NULL; result=SecRequirementCreateWithString(text,kSecCSDefaultFlags,&req);
        if(!result) { result=SecCodeCheckValidity(code,kSecCSDefaultFlags,req); CFRelease(req); }
        CFRelease(text); CFRelease(code);
    }
    CFRelease(attrs); CFRelease(data); return result;
}

int main(int argc,char **argv) {
    setvbuf(stdout,NULL,_IONBF,0);
    if(argc!=2 && !(argc==3 && !strcmp(argv[2],"--guarded"))) return 1;
    int guarded=(argc==3);
    mach_port_t broker=new_receive(), old=0;
    ok("make broker send",mach_port_insert_right(mach_task_self(),broker,broker,MACH_MSG_TYPE_MAKE_SEND));
    ok("old bootstrap",task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&old));
    ok("set bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,broker));
    int worker_done[2],worker_release[2],owner_control[2];
    if(pipe(worker_done)||pipe(worker_release)||pipe(owner_control)) { perror("pipe"); return 1; }
    pid_t worker=fork(); if(worker<0) { perror("fork worker"); return 1; }
    if(!worker) {
        close(worker_done[0]); close(worker_release[1]); close(owner_control[0]); close(owner_control[1]);
        mach_port_t dest=0; ok("worker bootstrap",task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&dest));
        mach_port_t inbox=new_receive(); send_one(dest,inbox,MACH_MSG_TYPE_MAKE_SEND,2);
        buffer_t b; receive(inbox,&b);
        if(b.one.h.msgh_id==4798) { printf("worker_cancelled pid=%d\n",getpid()); put(worker_done[1],'n'); (void)get(worker_release[0]); _exit(0); }
        if(b.one.marker!=3) _exit(3);
        mach_port_t stolen=b.one.port.name; receive(stolen,&b);
        if(b.plain.marker!=0x51a2) _exit(4);
        printf("worker_received_synthetic pid=%d marker=0x%x\n",getpid(),b.plain.marker);
        put(worker_done[1],'y'); (void)get(worker_release[0]); _exit(0);
    }
    close(worker_done[1]); close(worker_release[0]);
    buffer_t b; receive(broker,&b);
    if(b.one.marker!=2) { fprintf(stderr,"bad worker inbox\n"); return 1; }
    mach_port_t worker_inbox=b.one.port.name;
    pid_t owner=fork(); if(owner<0) { perror("fork owner"); return 1; }
    if(!owner) {
        close(owner_control[1]); close(worker_done[0]); close(worker_release[1]);
        mach_port_t dest=0; ok("owner bootstrap",task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&dest));
        mach_port_t endpoint=new_receive();
        if(guarded) { kern_return_t kr=mach_port_guard_with_flags(mach_task_self(),endpoint,0xa522u,MPG_IMMOVABLE_RECEIVE); printf("guard_kr=%d\n",kr); if(kr) _exit(5); }
        task_id_token_t token=0; ok("token",task_create_identity_token(mach_task_self(),&token));
        two_t m={0}; m.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;
        m.h.msgh_remote_port=dest; m.h.msgh_size=sizeof(m); m.h.msgh_id=4701; m.body.msgh_descriptor_count=2;
        m.token.name=token; m.token.disposition=MACH_MSG_TYPE_MOVE_SEND; m.token.type=MACH_MSG_PORT_DESCRIPTOR;
        m.endpoint.name=endpoint; m.endpoint.disposition=MACH_MSG_TYPE_MAKE_SEND; m.endpoint.type=MACH_MSG_PORT_DESCRIPTOR; m.marker=1;
        ok("send hello",mach_msg(&m.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(m),0,0,2000,0));
        (void)get(owner_control[0]); send_one(dest,endpoint,MACH_MSG_TYPE_MOVE_RECEIVE,3);
        (void)get(owner_control[0]); _exit(0);
    }
    close(owner_control[0]); ok("restore bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,old));
    receive(broker,&b);
    if(b.two.marker!=1 || b.two.body.msgh_descriptor_count!=2) { fprintf(stderr,"bad owner hello\n"); return 1; }
    audit_token_t sender=message_audit(&b), task_audit={0}; task_name_t name=0;
    ok("token to name",task_identity_token_get_task_port(b.two.token.name,TASK_FLAVOR_NAME,&name));
    mach_msg_type_number_t count=TASK_AUDIT_TOKEN_COUNT;
    ok("task audit",task_info(name,TASK_AUDIT_TOKEN,(task_info_t)&task_audit,&count));
    int same=memcmp(&sender,&task_audit,sizeof(sender))==0; OSStatus code=code_check(sender,argv[1]);
    printf("verified_owner_pid=%d sender_pid=%d task_audit_match=%d pinned_code_status=%d\n",owner,audit_token_to_pid(sender),same,(int)code);
    if(!same || code || audit_token_to_pid(sender)!=owner) return 2;
    mach_port_t client_send=b.two.endpoint.name;
    put(owner_control[1],'x');
    if(guarded) {
        kern_return_t kr=receive_for(broker,&b,800);
        if(kr==KERN_SUCCESS) mach_msg_destroy(&b.one.h);
        int status=0; pid_t ended=waitpid(owner,&status,WNOHANG);
        if(ended==0) { (void)kill(owner,SIGTERM); ended=waitpid(owner,&status,0); }
        send_plain(worker_inbox,4798,0);
        char result=get(worker_done[0]); put(worker_release[1],'x');
        int worker_status=0; if(waitpid(worker,&worker_status,0)!=worker) return 4;
        printf("guarded_transfer_receive_kr=%d owner_signaled=%d owner_signal=%d worker_received_synthetic=%d\n",kr,WIFSIGNALED(status),WIFSIGNALED(status)?WTERMSIG(status):0,result=='y');
        return kr==MACH_RCV_TIMED_OUT && ended==owner && WIFSIGNALED(status) && WTERMSIG(status)==SIGKILL && result=='n' && WIFEXITED(worker_status) && WEXITSTATUS(worker_status)==0 ? 0 : 6;
    }
    receive(broker,&b); if(b.one.marker!=3) { fprintf(stderr,"bad transferred endpoint\n"); return 2; }
    send_one(worker_inbox,b.one.port.name,MACH_MSG_TYPE_MOVE_RECEIVE,3);
    send_plain(client_send,4799,0x51a2);
    if(get(worker_done[0])!='y') return 3;
    int owner_status=0,worker_status=0;
    pid_t owner_wait=waitpid(owner,&owner_status,WNOHANG),worker_wait=waitpid(worker,&worker_status,WNOHANG);
    printf("after_handoff_owner_alive=%d worker_alive=%d\n",owner_wait==0,worker_wait==0);
    put(owner_control[1],'x'); put(worker_release[1],'x');
    if(waitpid(owner,&owner_status,0)!=owner || waitpid(worker,&worker_status,0)!=worker || !WIFEXITED(owner_status) || WEXITSTATUS(owner_status) || !WIFEXITED(worker_status) || WEXITSTATUS(worker_status)) return 4;
    return owner_wait==0 && worker_wait==0 ? 0 : 5;
}
