// TEST-ONLY, opt-in Mach experiment; never compiled into default releases.
#include "protocol.h"
#include <mach/task.h>
#include <servers/bootstrap.h>
#include <bsm/libbsm.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

extern void a2s_mach_lab_notify_shutdown(void);
static mach_port_t listener=MACH_PORT_NULL;
static mach_port_t discovery=MACH_PORT_NULL;
static mach_port_t receive_port=MACH_PORT_NULL;
static char instance_id[33];
static struct { int active; uint8_t nonce[16]; audit_token_t client; uint64_t expires_ms; } sessions[32];

static uint64_t monotonic_ms(void) {
    struct timespec now;
    if(clock_gettime(CLOCK_MONOTONIC,&now)!=0)return 0;
    return (uint64_t)now.tv_sec*1000u+(uint64_t)now.tv_nsec/1000000u;
}

// Harness-only barrier after authorization, before replying. It uses only a
// synthetic lab config directory and has a bounded wait even if a test dies.
static void wait_reply_barrier(void) {
    const char *base=getenv("A2S_MACH_LAB_REPLY_BARRIER");
    if(!base)return;
    char accepted[1024],release[1024];
    if(snprintf(accepted,sizeof(accepted),"%s.accepted",base)>=(int)sizeof(accepted) ||
       snprintf(release,sizeof(release),"%s.release",base)>=(int)sizeof(release))return;
    FILE *f=fopen(accepted,"w");if(!f)return;fputs("accepted\n",f);fclose(f);
    const struct timespec interval={0,10000000};
    for(unsigned i=0;i<500 && access(release,F_OK)!=0;i++)nanosleep(&interval,NULL);
}

static int same_token(const char *a,const char *b,size_t size) {
    unsigned diff=0;for(size_t i=0;i<size;i++)diff|=(unsigned char)a[i]^(unsigned char)b[i];return diff==0;
}
static mach_msg_audit_trailer_t *message_trailer(a2s_lab_receive *msg) {
    size_t start=round_msg(msg->h.msgh_size);
    if(start+sizeof(mach_msg_audit_trailer_t)>sizeof(*msg)) return NULL;
    mach_msg_audit_trailer_t *trailer=(void *)(msg->bytes+start);
    return trailer->msgh_trailer_size>=sizeof(*trailer)?trailer:NULL;
}
static void accept_hello(a2s_lab_hello *req,audit_token_t sender) {
    if(req->h.msgh_size!=sizeof(*req) || !(req->h.msgh_bits&MACH_MSGH_BITS_COMPLEX) || req->body.msgh_descriptor_count!=1 || req->reply.type!=MACH_MSG_PORT_DESCRIPTOR || req->reply.disposition!=MACH_MSG_TYPE_PORT_SEND_ONCE || !MACH_PORT_VALID(req->reply.name) || req->expected_pid!=(uint32_t)getpid() || memcmp(req->instance,instance_id,sizeof(instance_id))) return;
    uint64_t now=monotonic_ms();if(!now)return;
    unsigned slot=32;
    for(unsigned i=0;i<32;i++) { if(sessions[i].active && sessions[i].expires_ms<=now)sessions[i].active=0;if(!sessions[i].active && slot==32)slot=i; }
    if(slot==32)return;
    task_id_token_t identity=MACH_PORT_NULL;
    if(task_create_identity_token(mach_task_self(),&identity)!=KERN_SUCCESS)return;
    a2s_lab_hello_reply resp={0};resp.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_MOVE_SEND_ONCE,0)|MACH_MSGH_BITS_COMPLEX;
    resp.h.msgh_remote_port=req->reply.name;resp.h.msgh_size=sizeof(resp);resp.h.msgh_id=A2S_LAB_HELLO_REPLY;resp.body.msgh_descriptor_count=2;
    req->reply.name=MACH_PORT_NULL; // The response now owns the send-once right.
    resp.identity.name=identity;resp.identity.disposition=MACH_MSG_TYPE_MOVE_SEND;resp.identity.type=MACH_MSG_PORT_DESCRIPTOR;
    resp.endpoint.name=listener;resp.endpoint.disposition=MACH_MSG_TYPE_MAKE_SEND;resp.endpoint.type=MACH_MSG_PORT_DESCRIPTOR;
    memcpy(resp.nonce,req->nonce,sizeof(resp.nonce));memcpy(resp.instance,instance_id,sizeof(instance_id));resp.pid=(uint32_t)getpid();
    sessions[slot].active=1;sessions[slot].client=sender;sessions[slot].expires_ms=now+10000;memcpy(sessions[slot].nonce,req->nonce,16);
    kern_return_t kr=mach_msg(&resp.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(resp),0,0,1000,0);
    if(kr) { sessions[slot].active=0; mach_msg_destroy(&resp.h); }
}
static void accept_stop(a2s_lab_stop *req,audit_token_t sender) {
    if(req->h.msgh_size!=sizeof(*req) || !(req->h.msgh_bits&MACH_MSGH_BITS_COMPLEX) || req->body.msgh_descriptor_count!=1 || req->reply.type!=MACH_MSG_PORT_DESCRIPTOR || req->reply.disposition!=MACH_MSG_TYPE_PORT_SEND_ONCE || !MACH_PORT_VALID(req->reply.name) || req->expected_pid!=(uint32_t)getpid() || memcmp(req->instance,instance_id,sizeof(instance_id))) return;
    uint64_t now=monotonic_ms();if(!now)return;
    unsigned slot=32;for(unsigned i=0;i<32;i++)if(sessions[i].active && sessions[i].expires_ms>now && !memcmp(sessions[i].nonce,req->nonce,16) && !memcmp(&sessions[i].client,&sender,sizeof(sender))){slot=i;break;}
    if(slot==32 || !same_token(req->token,A2S_LAB_TOKEN,sizeof(A2S_LAB_TOKEN)) || req->token[sizeof(A2S_LAB_TOKEN)]!='\0')return;
    sessions[slot].active=0;
    wait_reply_barrier();
    a2s_lab_stop_reply resp={0};resp.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_MOVE_SEND_ONCE,0);resp.h.msgh_remote_port=req->reply.name;resp.h.msgh_size=sizeof(resp);resp.h.msgh_id=A2S_LAB_STOP_REPLY;memcpy(resp.nonce,req->nonce,16);resp.accepted=1;resp.pid=(uint32_t)getpid();
    req->reply.name=MACH_PORT_NULL;
    if(mach_msg(&resp.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(resp),0,0,1000,0)!=KERN_SUCCESS)mach_msg_destroy(&resp.h);
    // If the reply is lost, the client must report UnknownOutcome.
    a2s_mach_lab_notify_shutdown();
}
static void *serve(void *unused) {
    (void)unused;
    for(;;){a2s_lab_receive msg={0};mach_msg_option_t opts=MACH_RCV_MSG|MACH_RCV_TIMEOUT|MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0)|MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT);
        kern_return_t kr=mach_msg(&msg.h,opts,0,sizeof(msg),receive_port,1000,0);
        if(kr==MACH_RCV_TIMED_OUT)continue;
        if(kr==MACH_RCV_TOO_LARGE){fputs("A2S_MACH_LAB_OVERSIZED_DISCARDED\n",stderr);fflush(stderr);continue;}
        if(kr!=KERN_SUCCESS)break;
        mach_msg_audit_trailer_t *trailer=message_trailer(&msg);if(!trailer || audit_token_to_euid(trailer->msgh_audit)!=geteuid()) {mach_msg_destroy(&msg.h);continue;}
        if(msg.h.msgh_id==A2S_LAB_HELLO && msg.h.msgh_local_port==discovery)accept_hello(&msg.hello,trailer->msgh_audit);
        else if(msg.h.msgh_id==A2S_LAB_STOP && msg.h.msgh_local_port==listener){fputs("A2S_MACH_LAB_STOP_RECEIVED\n",stderr);fflush(stderr);accept_stop(&msg.stop,trailer->msgh_audit);}
        else if(msg.h.msgh_id==A2S_LAB_STOP && discovery!=listener && msg.h.msgh_local_port==discovery){fputs("A2S_MACH_LAB_PUBLIC_STOP_REJECTED\n",stderr);fflush(stderr);}
        // Release all received rights that were not moved into a reply.
        mach_msg_destroy(&msg.h);
    }
    return NULL;
}
int a2s_mach_lab_start(const char *service,const char *instance) {
    if(strlen(service)>100 || strncmp(service,"org.agent2ssh.lab.",18) || strlen(instance)!=32)return -1;
    memcpy(instance_id,instance,33);
    if(mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&listener)!=KERN_SUCCESS)return -2;
    if(mach_port_guard_with_flags(mach_task_self(),listener,0xa522,MPG_IMMOVABLE_RECEIVE)!=KERN_SUCCESS)return -3;
    if(mach_port_insert_right(mach_task_self(),listener,listener,MACH_MSG_TYPE_MAKE_SEND)!=KERN_SUCCESS)return -4;
    const char *launchd=getenv("A2S_MACH_LAB_LAUNCHD_DISCOVERY");
    if(launchd && !strcmp(launchd,"1")) {
        // Public launchd discovery is never the token-bearing endpoint.
        if(bootstrap_check_in(bootstrap_port,(char *)service,&discovery)!=KERN_SUCCESS)return -5;
        if(mach_port_guard_with_flags(mach_task_self(),discovery,0xb522,MPG_IMMOVABLE_RECEIVE)!=KERN_SUCCESS)return -6;
        if(mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_PORT_SET,&receive_port)!=KERN_SUCCESS)return -7;
        if(mach_port_move_member(mach_task_self(),listener,receive_port)!=KERN_SUCCESS || mach_port_move_member(mach_task_self(),discovery,receive_port)!=KERN_SUCCESS)return -8;
    } else {
        // Deprecated API is used only in the lab; production deployment is unresolved.
        if(bootstrap_register(bootstrap_port,(char *)service,listener)!=KERN_SUCCESS)return -5;
        discovery=listener;receive_port=listener;
    }
    pthread_t thread;if(pthread_create(&thread,NULL,serve,NULL)!=0)return -9;
    pthread_detach(thread);return 0;
}
