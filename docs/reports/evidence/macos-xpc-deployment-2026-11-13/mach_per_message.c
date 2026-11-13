// Kernel audit trailer + task token per message. All senders are own children.
#include <mach/mach.h>
#include <mach/task.h>
#include <mach/task_special_ports.h>
#include <bsm/libbsm.h>
#include <sys/wait.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { mach_msg_header_t h; mach_msg_body_t body; mach_msg_port_descriptor_t token; int marker; } msg_t;
typedef struct { msg_t m; mach_msg_audit_trailer_t t; unsigned char extra[128]; } recv_t;
static void ok(const char *where,kern_return_t kr){if(kr){fprintf(stderr,"%s kr=%d %s\n",where,kr,mach_error_string(kr));exit(1);}}
static pid_t spawn_sender(int marker,int *release_fd){
 int sync[2]; if(pipe(sync)){perror("pipe");exit(1);} pid_t child=fork(); if(child<0){perror("fork");exit(1);}
 if(!child){ close(sync[1]); mach_port_t destination=0; task_id_token_t token=0;
  if(task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&destination) || task_create_identity_token(mach_task_self(),&token)) _exit(2);
  msg_t m={0}; m.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX; m.h.msgh_remote_port=destination; m.h.msgh_size=sizeof(m); m.h.msgh_id=4500+marker; m.body.msgh_descriptor_count=1; m.token.name=token; m.token.disposition=MACH_MSG_TYPE_MOVE_SEND; m.token.type=MACH_MSG_PORT_DESCRIPTOR; m.marker=marker;
  kern_return_t kr=mach_msg(&m.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(m),0,0,2000,0); if(kr) _exit(3);
  char c; if(read(sync[0],&c,1)!=1) _exit(4); _exit(0);
 }
 close(sync[0]); *release_fd=sync[1]; return child;
}
static audit_token_t receive_sender(mach_port_t service,pid_t expected,int marker){
 recv_t b={0}; mach_msg_option_t o=MACH_RCV_MSG|MACH_RCV_TIMEOUT|MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0)|MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT);
 ok("receive",mach_msg(&b.m.h,o,0,sizeof(b),service,4000,0));
 if(!(b.m.h.msgh_bits&MACH_MSGH_BITS_COMPLEX)||b.m.body.msgh_descriptor_count!=1||b.m.marker!=marker){fprintf(stderr,"bad message\n");exit(1);}
 mach_msg_audit_trailer_t *tr=(void*)((char*)&b.m+round_msg(b.m.h.msgh_size)); if(tr->msgh_trailer_size<sizeof(*tr)){fprintf(stderr,"short trailer\n");exit(1);}
 task_name_t name=0; ok("token to name",task_identity_token_get_task_port(b.m.token.name,TASK_FLAVOR_NAME,&name));
 audit_token_t task={0}; mach_msg_type_number_t n=TASK_AUDIT_TOKEN_COUNT; ok("task audit",task_info(name,TASK_AUDIT_TOKEN,(task_info_t)&task,&n));
 int same=memcmp(&task,&tr->msgh_audit,sizeof(task))==0; int pid=audit_token_to_pid(tr->msgh_audit); int version=audit_token_to_pidversion(tr->msgh_audit);
 printf("message=%d expected_pid=%d actual_pid=%d pidversion=%d task_matches_trailer=%d\n",marker,expected,pid,version,same);
 if(!same || pid!=expected) {fprintf(stderr,"identity mismatch\n");exit(1);}
 (void)mach_port_deallocate(mach_task_self(),name); (void)mach_port_deallocate(mach_task_self(),b.m.token.name);
 return tr->msgh_audit;
}
int main(void){ setvbuf(stdout,NULL,_IONBF,0); mach_port_t service=0,old=0;
 ok("allocate",mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&service)); ok("send right",mach_port_insert_right(mach_task_self(),service,service,MACH_MSG_TYPE_MAKE_SEND));
 ok("get bootstrap",task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&old)); ok("set bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,service));
 int a_fd,b_fd; pid_t a=spawn_sender(1,&a_fd); audit_token_t first=receive_sender(service,a,1); pid_t b=spawn_sender(2,&b_fd); audit_token_t second=receive_sender(service,b,2);
 ok("restore bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,old));
 int same=memcmp(&first,&second,sizeof(first))==0; printf("pinned_first_accept_second=%d\n",same); if(write(a_fd,"x",1)!=1||write(b_fd,"x",1)!=1){perror("release");exit(1);} close(a_fd); close(b_fd);
 int sa=0,sb=0;if(waitpid(a,&sa,0)!=a||waitpid(b,&sb,0)!=b||!WIFEXITED(sa)||WEXITSTATUS(sa)||!WIFEXITED(sb)||WEXITSTATUS(sb)) return 2;
 (void)mach_port_deallocate(mach_task_self(),old); (void)mach_port_mod_refs(mach_task_self(),service,MACH_PORT_RIGHT_RECEIVE,-1); (void)mach_port_deallocate(mach_task_self(),service);
 return same ? 3 : 0;
}
