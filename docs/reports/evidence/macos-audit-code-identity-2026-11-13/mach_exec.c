// Probe whether a guarded Mach receive right survives exec in an owned child.
// Uses a synthetic integer only; no daemon or credentials are involved.
#include <mach/mach.h>
#include <mach/task.h>
#include <mach/task_special_ports.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifndef A2S_VARIANT
#define A2S_VARIANT "original"
#endif

typedef struct {mach_msg_header_t h;mach_msg_body_t body;mach_msg_port_descriptor_t endpoint,token;} hello_t;
typedef struct {mach_msg_header_t h;int marker;} plain_t;
typedef union {hello_t hello;plain_t plain;unsigned char bytes[1024];} buffer_t;
static void check(const char *name,kern_return_t kr){if(kr){fprintf(stderr,"%s=%d\n",name,kr);exit(1);}}
static void put(int fd,char c){if(write(fd,&c,1)!=1)exit(2);}
static char get(int fd){char c;if(read(fd,&c,1)!=1)exit(2);return c;}
int main(int argc,char **argv) {
 setvbuf(stdout,NULL,_IONBF,0);
 if(argc==4 && !strcmp(argv[1],"--after")) {
  int fd=atoi(argv[2]);mach_port_t endpoint=(mach_port_t)strtoul(argv[3],NULL,10);mach_port_type_t type=0;
  kern_return_t type_kr=mach_port_type(mach_task_self(),endpoint,&type);put(fd,'r');
  buffer_t msg={0};kern_return_t receive_kr=mach_msg(&msg.plain.h,MACH_RCV_MSG|MACH_RCV_TIMEOUT,0,sizeof(msg),endpoint,1000,0);
  printf("after_exec_pid=%d port_type_kr=%d has_receive=%d receive_kr=%d synthetic_received=%d variant=%s\n",getpid(),type_kr,type_kr==0 && (type&MACH_PORT_TYPE_RECEIVE)!=0,receive_kr,receive_kr==0 && msg.plain.marker==0x51a2,A2S_VARIANT);
  put(fd,'d');return 0;
 }
 if(argc!=1 && argc!=2)return 1;
 int control[2];if(socketpair(AF_UNIX,SOCK_STREAM,0,control))return 1;
 mach_port_t broker=0,old=0;check("broker",mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&broker));
 check("make send",mach_port_insert_right(mach_task_self(),broker,broker,MACH_MSG_TYPE_MAKE_SEND));
 check("get bootstrap",task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&old));
 check("set bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,broker));
 pid_t child=fork();if(child<0)return 1;
 if(!child){
  close(control[0]);mach_port_t dest=0,endpoint=0;task_id_token_t token=0;
  check("child bootstrap",task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&dest));
  check("endpoint",mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&endpoint));
  check("guard",mach_port_guard_with_flags(mach_task_self(),endpoint,0xa522,MPG_IMMOVABLE_RECEIVE));
  check("identity",task_create_identity_token(mach_task_self(),&token));
  hello_t hello={0};hello.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;hello.h.msgh_remote_port=dest;hello.h.msgh_size=sizeof(hello);hello.h.msgh_id=5001;
  hello.body.msgh_descriptor_count=2;hello.endpoint.name=endpoint;hello.endpoint.type=MACH_MSG_PORT_DESCRIPTOR;hello.endpoint.disposition=MACH_MSG_TYPE_MAKE_SEND;
  hello.token.name=token;hello.token.type=MACH_MSG_PORT_DESCRIPTOR;hello.token.disposition=MACH_MSG_TYPE_MOVE_SEND;
  check("send hello",mach_msg(&hello.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(hello),0,0,2000,0));
  if(get(control[1])!='x')_exit(3);char fd[24],port[24];snprintf(fd,sizeof(fd),"%d",control[1]);snprintf(port,sizeof(port),"%u",endpoint);
  const char *next=argc==2?argv[1]:argv[0];execl(next,next,"--after",fd,port,(char*)NULL);_exit(4);
 }
 close(control[1]);check("restore bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,old));
 buffer_t msg={0};check("hello receive",mach_msg(&msg.hello.h,MACH_RCV_MSG|MACH_RCV_TIMEOUT,0,sizeof(msg),broker,2000,0));
 if(msg.hello.h.msgh_id!=5001||msg.hello.body.msgh_descriptor_count!=2)return 2;
 mach_port_t send=msg.hello.endpoint.name;task_name_t name=0;check("identity before exec",task_identity_token_get_task_port(msg.hello.token.name,TASK_FLAVOR_NAME,&name));
 printf("owner_before_exec_pid=%d\n",child);put(control[0],'x');if(get(control[0])!='r')return 2;
 plain_t request={0};request.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0);request.h.msgh_remote_port=send;request.h.msgh_size=sizeof(request);request.marker=0x51a2;
 kern_return_t send_kr=mach_msg(&request.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(request),0,0,1000,0);printf("send_after_exec_kr=%d\n",send_kr);
 if(get(control[0])!='d')return 2;int status=0;if(waitpid(child,&status,0)!=child || !WIFEXITED(status) || WEXITSTATUS(status))return 2;
 close(control[0]);return 0;
}
