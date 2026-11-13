// Isolated Mach stop lab client. Never use this experimental protocol with
// user credentials. The supplied token is a fixed synthetic test value only.
#include "protocol.h"
#include <mach/task.h>
#include <mach/notify.h>
#include <servers/bootstrap.h>
#include <Security/Security.h>
#include <CoreFoundation/CoreFoundation.h>
#include <bsm/libbsm.h>
#include <sys/event.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef union {mach_msg_header_t h;a2s_lab_hello_reply hello;a2s_lab_stop_reply stop;unsigned char bytes[2048];} response_t;
static int valid_code(audit_token_t audit,const char *requirement) {
    CFDataRef data=CFDataCreate(NULL,(UInt8*)&audit,sizeof(audit));const void *k[]={kSecGuestAttributeAudit},*v[]={data};
    CFDictionaryRef attrs=CFDictionaryCreate(NULL,k,v,1,&kCFTypeDictionaryKeyCallBacks,&kCFTypeDictionaryValueCallBacks);SecCodeRef code=NULL;
    OSStatus result=SecCodeCopyGuestWithAttributes(NULL,attrs,kSecCSDefaultFlags,&code);CFRelease(attrs);CFRelease(data);if(result)return 0;
    CFStringRef text=CFStringCreateWithCString(NULL,requirement,kCFStringEncodingUTF8);SecRequirementRef req=NULL;
    result=SecRequirementCreateWithString(text,kSecCSDefaultFlags,&req);CFRelease(text);if(result){CFRelease(code);return 0;}
    result=SecCodeCheckValidity(code,kSecCSDefaultFlags,req);CFRelease(req);if(result){CFRelease(code);return 0;}
    CFDictionaryRef info=NULL;result=SecCodeCopySigningInformation(code,kSecCSSigningInformation|kSecCSDynamicInformation,&info);CFRelease(code);if(result||!info)return 0;
    CFNumberRef f=CFDictionaryGetValue(info,kSecCodeInfoFlags),s=CFDictionaryGetValue(info,kSecCodeInfoStatus);int64_t flags=0,status=0;
    int good=f&&CFGetTypeID(f)==CFNumberGetTypeID()&&CFNumberGetValue(f,kCFNumberSInt64Type,&flags)&&(flags&kSecCodeSignatureRuntime)
        && s&&CFGetTypeID(s)==CFNumberGetTypeID()&&CFNumberGetValue(s,kCFNumberSInt64Type,&status)&&(status&kSecCodeStatusValid)&&!(status&kSecCodeStatusDebugged);
    CFDictionaryRef ent=CFDictionaryGetValue(info,kSecCodeInfoEntitlementsDict);
    if(ent && CFGetTypeID(ent)==CFDictionaryGetTypeID()) {
        const CFStringRef forbidden[]={CFSTR("com.apple.security.cs.allow-dyld-environment-variables"),CFSTR("com.apple.security.cs.disable-library-validation"),CFSTR("com.apple.security.cs.allow-unsigned-executable-memory"),CFSTR("com.apple.security.cs.allow-jit"),CFSTR("com.apple.security.cs.disable-executable-page-protection"),CFSTR("com.apple.security.get-task-allow")};
        for(unsigned i=0;i<sizeof(forbidden)/sizeof(forbidden[0]);i++){CFTypeRef x=CFDictionaryGetValue(ent,forbidden[i]);if(x && (CFGetTypeID(x)!=CFBooleanGetTypeID()||CFBooleanGetValue(x)))good=0;}
    } else if(ent || CFDictionaryContainsKey(info,kSecCodeInfoEntitlements)) good=0;
    CFRelease(info);return good;
}
static int receive(mach_port_t port,response_t *msg){
    memset(msg,0,sizeof(*msg));mach_msg_option_t opts=MACH_RCV_MSG|MACH_RCV_TIMEOUT|MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0)|MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT);
    return mach_msg(&msg->h,opts,0,sizeof(*msg),port,2500,0);
}
static mach_msg_audit_trailer_t *trailer(response_t *msg){size_t offset=round_msg(msg->h.msgh_size);if(offset+sizeof(mach_msg_audit_trailer_t)>sizeof(*msg))return NULL;mach_msg_audit_trailer_t *t=(void*)(msg->bytes+offset);return t->msgh_trailer_size>=sizeof(*t)?t:NULL;}
static int hex_nibble(char c){if(c>='0'&&c<='9')return c-'0';if(c>='a'&&c<='f')return c-'a'+10;return -1;}
int main(int argc,char **argv){
    setvbuf(stdout,NULL,_IONBF,0);
    if(argc!=6){fprintf(stderr,"usage: client service pid instance requirement mode\n");return 1;}
    int expected=atoi(argv[2]);if(expected<=0||strlen(argv[3])!=32)return 1;
    mach_port_t service=MACH_PORT_NULL,reply=MACH_PORT_NULL;
    if(bootstrap_look_up(bootstrap_port,argv[1],&service)!=KERN_SUCCESS||!MACH_PORT_VALID(service)){printf("refused=lookup token_sent=0\n");return 2;}
    if(!strcmp(argv[5],"oversized")) {
        struct {mach_msg_header_t h;char payload[4096];} large={0};
        large.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0);large.h.msgh_remote_port=service;large.h.msgh_size=sizeof(large);large.h.msgh_id=0xa229;
        if(mach_msg(&large.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(large),0,0,1000,0))return 3;
        printf("oversized_sent=1 token_sent=0\n");return 0;
    }
    if(mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&reply)||mach_port_guard_with_flags(mach_task_self(),reply,0xb522,MPG_IMMOVABLE_RECEIVE)){printf("refused=reply_port token_sent=0\n");return 2;}
    if(!strcmp(argv[5],"adversarial-stop")) {
        // Deliberately bypass the client proof for a separate-process negative
        // test. The fixed token and nonce are synthetic and non-production.
        const char *hex=getenv("A2S_MACH_LAB_TEST_NONCE");if(!hex||strlen(hex)!=32)return 1;
        printf("sender_after_exec_pid=%d\n",getpid());
        a2s_lab_stop attack={0};attack.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;attack.h.msgh_remote_port=service;attack.h.msgh_size=sizeof(attack);attack.h.msgh_id=A2S_LAB_STOP;
        attack.body.msgh_descriptor_count=1;attack.reply.name=reply;attack.reply.disposition=MACH_MSG_TYPE_MAKE_SEND_ONCE;attack.reply.type=MACH_MSG_PORT_DESCRIPTOR;
        for(unsigned i=0;i<16;i++){int high=hex_nibble(hex[2*i]),low=hex_nibble(hex[2*i+1]);if(high<0||low<0)return 1;attack.nonce[i]=(uint8_t)((high<<4)|low);}
        memcpy(attack.instance,argv[3],32);attack.expected_pid=(uint32_t)expected;strcpy(attack.token,A2S_LAB_TOKEN);
        if(mach_msg(&attack.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(attack),0,0,1000,0))return 3;
        response_t result;int received=receive(reply,&result);
        if(received!=MACH_RCV_TIMED_OUT && (received!=KERN_SUCCESS || result.h.msgh_id!=MACH_NOTIFY_SEND_ONCE)){printf("adversarial_stop=unexpected_reply\n");return 4;}
        printf("adversarial_stop=refused\n");return 3;
    }
    a2s_lab_hello hello={0};hello.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;hello.h.msgh_remote_port=service;hello.h.msgh_size=sizeof(hello);hello.h.msgh_id=A2S_LAB_HELLO;
    hello.body.msgh_descriptor_count=1;hello.reply.name=reply;hello.reply.disposition=MACH_MSG_TYPE_MAKE_SEND_ONCE;hello.reply.type=MACH_MSG_PORT_DESCRIPTOR;arc4random_buf(hello.nonce,sizeof(hello.nonce));memcpy(hello.instance,argv[3],32);hello.expected_pid=(uint32_t)expected;
    if(mach_msg(&hello.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(hello),0,0,1000,0)){printf("refused=hello_send token_sent=0\n");return 2;}
    response_t resp;if(receive(reply,&resp)||resp.h.msgh_id!=A2S_LAB_HELLO_REPLY||resp.h.msgh_size!=sizeof(a2s_lab_hello_reply)||!(resp.h.msgh_bits&MACH_MSGH_BITS_COMPLEX)||resp.hello.body.msgh_descriptor_count!=2){printf("refused=hello_reply token_sent=0\n");return 2;}
    mach_msg_audit_trailer_t *t=trailer(&resp);if(!t){printf("refused=audit token_sent=0\n");return 2;}
    audit_token_t sender=t->msgh_audit;task_name_t name=MACH_PORT_NULL;
    int expect_private=getenv("A2S_MACH_LAB_EXPECT_PRIVATE")!=NULL;
    if(audit_token_to_euid(sender)!=geteuid()||audit_token_to_pid(sender)!=expected||resp.hello.pid!=(uint32_t)expected||memcmp(resp.hello.nonce,hello.nonce,16)||memcmp(resp.hello.instance,hello.instance,33)||!MACH_PORT_VALID(resp.hello.endpoint.name)||(expect_private ? resp.hello.endpoint.name==service : resp.hello.endpoint.name!=service)||task_identity_token_get_task_port(resp.hello.identity.name,TASK_FLAVOR_NAME,&name)){printf("refused=identity token_sent=0\n");return 2;}
    audit_token_t task={0};mach_msg_type_number_t count=TASK_AUDIT_TOKEN_COUNT;
    if(task_info(name,TASK_AUDIT_TOKEN,(task_info_t)&task,&count)||memcmp(&task,&sender,sizeof(task))||!valid_code(sender,argv[4])){printf("refused=code_or_task token_sent=0\n");return 2;}
    printf("identity_observations_match=1 same_endpoint=1 runtime_policy_match=1\n");
    // Guard use is a property of the exact laboratory binary and source path,
    // not something this client can independently query from a send right.
    if(!strcmp(argv[5],"hello-only")){printf("result=hello_only token_sent=0\n");return 0;}
    if(!strcmp(argv[5],"exec-attack")) {
        char nonce[33];for(unsigned i=0;i<16;i++)snprintf(nonce+2*i,3,"%02x",hello.nonce[i]);
        if(setenv("A2S_MACH_LAB_TEST_NONCE",nonce,1))return 1;
        printf("sender_before_exec_pid=%d\n",getpid());
        argv[5]="adversarial-stop";execv(argv[0],argv);return 1;
    }
    if(!strcmp(argv[5],"hold")) {
        const char *path=getenv("A2S_MACH_LAB_CLIENT_READY");
        if(!path){printf("refused=missing_test_marker token_sent=0\n");return 2;}
        FILE *ready=fopen(path,"w");if(!ready){printf("refused=test_marker token_sent=0\n");return 2;}
        for(unsigned i=0;i<16;i++){fprintf(ready,"%02x",hello.nonce[i]);}
        fputc('\n',ready);fclose(ready);
        if(getchar()!='x'){printf("refused=test_release token_sent=0\n");return 2;}
    }
    mach_port_t exit_port=MACH_PORT_NULL;int exit_kq=-1;
    if(!strcmp(argv[5],"wait-exit")||!strcmp(argv[5],"hold-registration")||!strcmp(argv[5],"hold-before-registration")) {
        mach_port_t previous=MACH_PORT_NULL;
        if(mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&exit_port)!=KERN_SUCCESS ||
           mach_port_request_notification(mach_task_self(),name,MACH_NOTIFY_DEAD_NAME,0,exit_port,MACH_MSG_TYPE_MAKE_SEND_ONCE,&previous)!=KERN_SUCCESS || MACH_PORT_VALID(previous)) {
            printf("refused=exit_notification_setup token_sent=0\n");return 2;
        }
        exit_kq=kqueue();struct kevent registration;
        if(exit_kq<0){printf("refused=kqueue token_sent=0\n");return 2;}
        if(!strcmp(argv[5],"hold-before-registration")) {
            const char *path=getenv("A2S_MACH_LAB_CLIENT_READY");if(!path)return 2;
            FILE *ready=fopen(path,"w");if(!ready)return 2;fputs("before registration\n",ready);fclose(ready);
            if(getchar()!='x')return 2;
        }
        EV_SET(&registration,(uintptr_t)expected,EVFILT_PROC,EV_ADD|EV_ENABLE|EV_CLEAR,NOTE_EXIT,0,NULL);
        if(kevent(exit_kq,&registration,1,NULL,0,NULL)<0){printf("refused=proc_registration token_sent=0\n");return 2;}
        if(!strcmp(argv[5],"hold-registration")) {
            const char *path=getenv("A2S_MACH_LAB_CLIENT_READY");if(!path)return 2;
            FILE *ready=fopen(path,"w");if(!ready)return 2;fputs("registered\n",ready);fclose(ready);
            if(getchar()!='x')return 2;
        }
        task_name_t rechecked=MACH_PORT_NULL;audit_token_t again={0};mach_msg_type_number_t again_count=TASK_AUDIT_TOKEN_COUNT;
        if(task_identity_token_get_task_port(resp.hello.identity.name,TASK_FLAVOR_NAME,&rechecked)!=KERN_SUCCESS ||
           task_info(rechecked,TASK_AUDIT_TOKEN,(task_info_t)&again,&again_count)!=KERN_SUCCESS || memcmp(&again,&sender,sizeof(sender))) {
            printf("refused=post_registration_identity token_sent=0\n");return 2;
        }
        (void)mach_port_deallocate(mach_task_self(),rechecked);
        printf("post_registration_identity_match=1\n");
        if(!strcmp(argv[5],"hold-registration")){printf("result=unexpected_identity token_sent=0\n");return 2;}
    }
    a2s_lab_stop stop={0};stop.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;stop.h.msgh_remote_port=!strcmp(argv[5],"public-stop")?service:resp.hello.endpoint.name;stop.h.msgh_size=sizeof(stop);stop.h.msgh_id=A2S_LAB_STOP;stop.body.msgh_descriptor_count=1;stop.reply.name=reply;stop.reply.disposition=MACH_MSG_TYPE_MAKE_SEND_ONCE;stop.reply.type=MACH_MSG_PORT_DESCRIPTOR;memcpy(stop.nonce,hello.nonce,16);memcpy(stop.instance,hello.instance,33);stop.expected_pid=(uint32_t)expected;
    if(!strcmp(argv[5],"borrowed-stop")) {
        const char *hex=getenv("A2S_MACH_LAB_TEST_NONCE");if(!hex||strlen(hex)!=32)return 1;
        for(unsigned i=0;i<16;i++){int high=hex_nibble(hex[2*i]),low=hex_nibble(hex[2*i+1]);if(high<0||low<0)return 1;stop.nonce[i]=(uint8_t)((high<<4)|low);}
    }
    if(!strcmp(argv[5],"wrong-token"))strcpy(stop.token,"wrong-synthetic-token");else if(!strcmp(argv[5],"stop")||!strcmp(argv[5],"hold")||!strcmp(argv[5],"drop-reply")||!strcmp(argv[5],"hold-reply")||!strcmp(argv[5],"wait-exit")||!strcmp(argv[5],"public-stop")||!strcmp(argv[5],"borrowed-stop"))strcpy(stop.token,A2S_LAB_TOKEN);else return 1;
    kern_return_t sent=mach_msg(&stop.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(stop),0,0,1000,0);
    if(sent){printf("result=unknown token_sent=1 send_kr=%d\n",sent);return 3;}
    printf("token_sent=1\n");
    if(!strcmp(argv[5],"hold-reply") && getchar()!='x')return 4;
    if(!strcmp(argv[5],"drop-reply")||!strcmp(argv[5],"hold-reply")){
        if(mach_port_destruct(mach_task_self(),reply,0,0xb522)!=KERN_SUCCESS)return 4;
        printf("result=unknown reply_destroyed=1\n");return 3;
    }
    if(receive(reply,&resp)||resp.h.msgh_id!=A2S_LAB_STOP_REPLY||resp.h.msgh_size!=sizeof(a2s_lab_stop_reply)){printf("result=unknown\n");return 3;}
    t=trailer(&resp);if(!t||memcmp(&sender,&t->msgh_audit,sizeof(sender))||memcmp(resp.stop.nonce,hello.nonce,16)||resp.stop.pid!=(uint32_t)expected||!resp.stop.accepted){printf("result=unknown\n");return 3;}
    printf("result=accepted_exit_unverified\n");
    if(MACH_PORT_VALID(exit_port)) {
        response_t notification;int kr=receive(exit_port,&notification);
        if(kr==KERN_SUCCESS && notification.h.msgh_id==MACH_NOTIFY_DEAD_NAME)printf("task_name_exit_notification=dead_name\n");
        else printf("task_name_exit_notification=other receive_kr=%d message_id=%d\n",kr,kr==KERN_SUCCESS?notification.h.msgh_id:0);
    }
    if(exit_kq>=0) {
        struct kevent event={0};struct timespec timeout={3,0};int n=kevent(exit_kq,NULL,0,&event,1,&timeout);
        printf("proc_exit_event=%d event_pid_matches=%d note_exit=%d\n",n,n==1 && event.ident==(uintptr_t)expected,n==1 && (event.fflags&NOTE_EXIT)!=0);
        close(exit_kq);
    }
    return 0;
}
