// Transient launchd discovery, per-instance guarded endpoint; synthetic only.
#include <mach/mach.h>
#include <mach/task.h>
#include <servers/bootstrap.h>
#include <bsm/libbsm.h>
#include <Security/Security.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
typedef struct {mach_msg_header_t h;mach_msg_body_t body;mach_msg_port_descriptor_t reply;} hello_t;
typedef struct {mach_msg_header_t h;mach_msg_body_t body;mach_msg_port_descriptor_t endpoint,identity;int pid;} response_t;
typedef struct {mach_msg_header_t h;unsigned marker;} plain_t;
typedef union {mach_msg_header_t h;hello_t hello;response_t response;plain_t plain;unsigned char bytes[1024];} msg_t;
static kern_return_t receive(mach_port_t port,msg_t *msg,unsigned timeout){
 memset(msg,0,sizeof(*msg));return mach_msg(&msg->h,MACH_RCV_MSG|MACH_RCV_TIMEOUT|MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0)|MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT),0,sizeof(*msg),port,timeout,0);
}
static void wait_release(const char *root){
 char path[1024];if(snprintf(path,sizeof(path),"%s/%d",root,getpid())>=(int)sizeof(path))return;
 struct timespec pause={0,10000000};for(unsigned i=0;i<1500 && access(path,F_OK)!=0;i++)nanosleep(&pause,NULL);
}
static int valid_code(audit_token_t audit,const char *requirement){
 CFDataRef data=CFDataCreate(NULL,(const UInt8*)&audit,sizeof(audit));const void *keys[]={kSecGuestAttributeAudit},*values[]={data};
 CFDictionaryRef attrs=CFDictionaryCreate(NULL,keys,values,1,&kCFTypeDictionaryKeyCallBacks,&kCFTypeDictionaryValueCallBacks);SecCodeRef code=NULL;
 OSStatus status=SecCodeCopyGuestWithAttributes(NULL,attrs,kSecCSDefaultFlags,&code);CFRelease(attrs);CFRelease(data);if(status)return 0;
 CFStringRef text=CFStringCreateWithCString(NULL,requirement,kCFStringEncodingUTF8);SecRequirementRef req=NULL;
 status=SecRequirementCreateWithString(text,kSecCSDefaultFlags,&req);CFRelease(text);
 if(!status){status=SecCodeCheckValidity(code,kSecCSDefaultFlags,req);CFRelease(req);}CFRelease(code);return status==0;
}
int main(int argc,char **argv){
 setvbuf(stdout,NULL,_IONBF,0);
 if(argc==5 && !strcmp(argv[1],"server")){
  FILE *log=fopen(argv[3],"a");if(!log)return 2;setvbuf(log,NULL,_IONBF,0);
  mach_port_t discovery=0;kern_return_t kr=bootstrap_check_in(bootstrap_port,argv[2],&discovery);
  kern_return_t guard=kr?kr:mach_port_guard_with_flags(mach_task_self(),discovery,0xa522,MPG_IMMOVABLE_RECEIVE);
  fprintf(log,"start pid=%d checkin=%d guard=%d\n",getpid(),kr,guard);if(kr||guard)return 3;
  msg_t msg;if(receive(discovery,&msg,12000))return 4;
  if(msg.h.msgh_id==4811){fprintf(log,"replacement_started pid=%d\n",getpid());wait_release(argv[4]);return 0;}
  if(msg.h.msgh_id!=6001 || msg.h.msgh_size!=sizeof(hello_t) || !(msg.h.msgh_bits&MACH_MSGH_BITS_COMPLEX) || msg.hello.body.msgh_descriptor_count!=1 || msg.hello.reply.disposition!=MACH_MSG_TYPE_PORT_SEND_ONCE)return 5;
  mach_port_t endpoint=0;task_id_token_t token=0;
  if(mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&endpoint)||mach_port_guard_with_flags(mach_task_self(),endpoint,0xb522,MPG_IMMOVABLE_RECEIVE)||task_create_identity_token(mach_task_self(),&token))return 6;
  response_t reply={0};reply.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_MOVE_SEND_ONCE,0)|MACH_MSGH_BITS_COMPLEX;reply.h.msgh_remote_port=msg.hello.reply.name;reply.h.msgh_size=sizeof(reply);reply.h.msgh_id=6002;reply.body.msgh_descriptor_count=2;
  reply.endpoint.name=endpoint;reply.endpoint.type=MACH_MSG_PORT_DESCRIPTOR;reply.endpoint.disposition=MACH_MSG_TYPE_MAKE_SEND;
  reply.identity.name=token;reply.identity.type=MACH_MSG_PORT_DESCRIPTOR;reply.identity.disposition=MACH_MSG_TYPE_MOVE_SEND;reply.pid=getpid();
  if(mach_msg(&reply.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(reply),0,0,2000,0))return 7;
  fprintf(log,"private_issued pid=%d\n",getpid());wait_release(argv[4]);return 0;
 }
 if(argc==5 && !strcmp(argv[1],"client")){
  mach_port_t discovery=0,reply=0;if(bootstrap_look_up(bootstrap_port,argv[2],&discovery)||mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&reply)||mach_port_guard_with_flags(mach_task_self(),reply,0xc522,MPG_IMMOVABLE_RECEIVE))return 2;
  hello_t hello={0};hello.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;hello.h.msgh_remote_port=discovery;hello.h.msgh_size=sizeof(hello);hello.h.msgh_id=6001;hello.body.msgh_descriptor_count=1;hello.reply.name=reply;hello.reply.type=MACH_MSG_PORT_DESCRIPTOR;hello.reply.disposition=MACH_MSG_TYPE_MAKE_SEND_ONCE;
  if(mach_msg(&hello.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(hello),0,0,2000,0))return 3;
  msg_t msg;if(receive(reply,&msg,10000)||msg.h.msgh_id!=6002||msg.h.msgh_size!=sizeof(response_t)||!(msg.h.msgh_bits&MACH_MSGH_BITS_COMPLEX)||msg.response.body.msgh_descriptor_count!=2)return 4;
  mach_msg_audit_trailer_t *trailer=(void *)(msg.bytes+round_msg(msg.h.msgh_size));if(trailer->msgh_trailer_size<sizeof(*trailer))return 5;
  audit_token_t sender=trailer->msgh_audit,task={0};task_name_t name=0;mach_msg_type_number_t count=TASK_AUDIT_TOKEN_COUNT;
  if(audit_token_to_pid(sender)!=msg.response.pid||task_identity_token_get_task_port(msg.response.identity.name,TASK_FLAVOR_NAME,&name)||task_info(name,TASK_AUDIT_TOKEN,(task_info_t)&task,&count)||memcmp(&sender,&task,sizeof(sender))||!valid_code(sender,argv[4]))return 6;
  FILE *ready=fopen(argv[3],"w");if(!ready)return 7;fprintf(ready,"%d\n",msg.response.pid);fclose(ready);
  if(getchar()!='x')return 8;
  plain_t request={0};request.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0);request.h.msgh_remote_port=msg.response.endpoint.name;request.h.msgh_size=sizeof(request);request.h.msgh_id=6003;request.marker=0x51a2;
  kern_return_t kr=mach_msg(&request.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(request),0,0,1000,0);
  printf("private_old_send_kr=%d\n",kr);return 0;
 }
 return 1;
}
