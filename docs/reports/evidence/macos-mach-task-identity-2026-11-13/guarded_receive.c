// Controlled receive-right transfer between this probe and its own children.
// No launchd registration, daemon, credential or unrelated process is touched.
#include <mach/mach.h>
#include <mach/task_special_ports.h>
#include <sys/wait.h>
#include <signal.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { mach_msg_header_t header; mach_msg_body_t body; mach_msg_port_descriptor_t port; } msg_t;
static void ok(const char *where, kern_return_t kr) { if(kr != KERN_SUCCESS) { fprintf(stderr,"%s: %d %s\n",where,kr,mach_error_string(kr)); exit(1); } }
static int run_case(mach_port_t service, int guarded) {
    pid_t child = fork(); if(child<0) {perror("fork");exit(1);}
    if(!child) {
        mach_port_t destination=MACH_PORT_NULL, receive=MACH_PORT_NULL;
        kern_return_t kr=task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&destination);
        if(kr) _exit(2);
        task_exc_guard_behavior_t behavior=0;
        kr=task_get_exc_guard_behavior(mach_task_self(),&behavior);
        printf("guarded=%d get_guard_behavior_kr=%d flags=0x%x\n",guarded,kr,behavior);
        kr=task_set_exc_guard_behavior(mach_task_self(),TASK_EXC_GUARD_NONE);
        printf("guarded=%d set_nonfatal_kr=%d\n",guarded,kr);
        kr=mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&receive);
        if(kr) _exit(3);
        if(guarded) {
            kr=mach_port_guard_with_flags(mach_task_self(),receive,0xa522u,MPG_IMMOVABLE_RECEIVE);
            printf("guarded=%d guard_kr=%d (%s)\n",guarded,kr,mach_error_string(kr));
            if(kr) _exit(4);
        }
        msg_t message={0};
        message.header.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;
        message.header.msgh_remote_port=destination; message.header.msgh_size=sizeof(message); message.header.msgh_id=4321;
        message.body.msgh_descriptor_count=1; message.port.name=receive;
        message.port.disposition=MACH_MSG_TYPE_MOVE_RECEIVE; message.port.type=MACH_MSG_PORT_DESCRIPTOR;
        kr=mach_msg(&message.header,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(message),0,MACH_PORT_NULL,1000,MACH_PORT_NULL);
        printf("guarded=%d transfer_send_kr=%d (%s)\n",guarded,kr,mach_error_string(kr));
        _exit(kr==KERN_SUCCESS ? 0 : 5);
    }
    struct {msg_t msg; unsigned char trailer[128];} received={0};
    kern_return_t kr=mach_msg(&received.msg.header,MACH_RCV_MSG|MACH_RCV_TIMEOUT,0,sizeof(received),service,1200,MACH_PORT_NULL);
    printf("guarded=%d transfer_receive_kr=%d (%s)",guarded,kr,mach_error_string(kr));
    if(kr==KERN_SUCCESS) printf(" descriptor_count=%u type=%d",received.msg.body.msgh_descriptor_count,received.msg.port.type);
    putchar('\n');
    if(kr==KERN_SUCCESS) mach_msg_destroy(&received.msg.header);
    int status=0; if(waitpid(child,&status,0)!=child) {perror("waitpid");exit(1);}
    printf("guarded=%d child_exited=%d exit=%d signaled=%d signal=%d\n",guarded,WIFEXITED(status),WIFEXITED(status)?WEXITSTATUS(status):-1,WIFSIGNALED(status),WIFSIGNALED(status)?WTERMSIG(status):0);
    if (!guarded) return kr==KERN_SUCCESS && WIFEXITED(status) && WEXITSTATUS(status)==0;
    return kr==MACH_RCV_TIMED_OUT && ((WIFEXITED(status) && WEXITSTATUS(status)==5) || (WIFSIGNALED(status) && WTERMSIG(status)==SIGKILL));
}
static int query_case(mach_port_t service, int guarded) {
    int sync[2]; if(pipe(sync)) { perror("pipe"); exit(1); }
    pid_t child=fork(); if(child<0) {perror("fork");exit(1);}
    if(!child) {
        close(sync[1]);
        mach_port_t destination=MACH_PORT_NULL, receive=MACH_PORT_NULL;
        kern_return_t kr=task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&destination);
        if(kr || mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&receive)) _exit(2);
        if(guarded && mach_port_guard_with_flags(mach_task_self(),receive,0xa522u,MPG_IMMOVABLE_RECEIVE)) _exit(3);
        msg_t message={0};
        message.header.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;
        message.header.msgh_remote_port=destination; message.header.msgh_size=sizeof(message); message.header.msgh_id=4322;
        message.body.msgh_descriptor_count=1; message.port.name=receive;
        message.port.disposition=MACH_MSG_TYPE_MAKE_SEND; message.port.type=MACH_MSG_PORT_DESCRIPTOR;
        kr=mach_msg(&message.header,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(message),0,MACH_PORT_NULL,1000,MACH_PORT_NULL);
        if(kr) _exit(4);
        char byte; if(read(sync[0],&byte,1)!=1) _exit(5);
        _exit(0);
    }
    close(sync[0]);
    struct {msg_t msg; unsigned char trailer[128];} received={0};
    kern_return_t kr=mach_msg(&received.msg.header,MACH_RCV_MSG|MACH_RCV_TIMEOUT,0,sizeof(received),service,1200,MACH_PORT_NULL);
    if(kr==KERN_SUCCESS && received.msg.body.msgh_descriptor_count==1) {
        mach_port_context_t context=0; kern_return_t ck=mach_port_get_context(mach_task_self(),received.msg.port.name,&context);
        mach_port_status_t status={0}; mach_msg_type_number_t count=MACH_PORT_RECEIVE_STATUS_COUNT;
        kern_return_t sk=mach_port_get_attributes(mach_task_self(),received.msg.port.name,MACH_PORT_RECEIVE_STATUS,(mach_port_info_t)&status,&count);
        mach_port_info_ext_t ext={0}; mach_msg_type_number_t ext_count=MACH_PORT_INFO_EXT_COUNT;
        kern_return_t ek=mach_port_get_attributes(mach_task_self(),received.msg.port.name,MACH_PORT_INFO_EXT,(mach_port_info_t)&ext,&ext_count);
        printf("query_guarded=%d get_context_kr=%d receive_status_kr=%d flags=0x%x info_ext_kr=%d ext_flags=0x%x\n",guarded,ck,sk,status.mps_flags,ek,ext.mpie_status.mps_flags);
        mach_msg_destroy(&received.msg.header);
    } else printf("query_guarded=%d receive_kr=%d\n",guarded,kr);
    if(write(sync[1],"x",1)!=1) {perror("query release");exit(1);}
    close(sync[1]); int status=0; if(waitpid(child,&status,0)!=child) {perror("wait query");exit(1);}
    return kr==KERN_SUCCESS && WIFEXITED(status) && WEXITSTATUS(status)==0;
}
int main(void) {
    setvbuf(stdout,NULL,_IONBF,0);
    mach_port_t service=MACH_PORT_NULL, original=MACH_PORT_NULL;
    ok("allocate service",mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&service));
    ok("make send",mach_port_insert_right(mach_task_self(),service,service,MACH_MSG_TYPE_MAKE_SEND));
    ok("get bootstrap",task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&original));
    ok("set bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,service));
    int control_ok=run_case(service,0), guarded_ok=run_case(service,1);
    int query_control_ok=query_case(service,0), query_guarded_ok=query_case(service,1);
    ok("restore bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,original));
    (void)mach_port_deallocate(mach_task_self(),original);
    (void)mach_port_mod_refs(mach_task_self(),service,MACH_PORT_RIGHT_RECEIVE,-1);
    (void)mach_port_deallocate(mach_task_self(),service);
    if (!control_ok || !guarded_ok || !query_control_ok || !query_guarded_ok) { fprintf(stderr,"guarded receive control or refusal did not match expectations\n"); return 1; }
    return 0;
}
