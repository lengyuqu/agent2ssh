// Real kernel Mach sender audit token -> public Security.framework code check.
// Only this test's own child and synthetic signing requirements are used.
#include <mach/mach.h>
#include <mach/task.h>
#include <mach/task_special_ports.h>
#include <Security/Security.h>
#include <CoreFoundation/CoreFoundation.h>
#include <bsm/libbsm.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifndef A2S_VARIANT
#define A2S_VARIANT "original"
#endif

typedef struct { mach_msg_header_t h; mach_msg_body_t body; mach_msg_port_descriptor_t token; int marker; } msg_t;
typedef struct { msg_t m; mach_msg_audit_trailer_t trailer; unsigned char extra[128]; } recv_t;
static void ok(const char *where,kern_return_t kr){if(kr){fprintf(stderr,"%s: %d %s\n",where,kr,mach_error_string(kr));exit(1);}}
static void send_byte(int fd,char c){if(write(fd,&c,1)!=1){perror("write");exit(1);}}
static char read_byte(int fd){char c;if(read(fd,&c,1)!=1){perror("read");exit(1);}return c;}
static OSStatus lookup_code(audit_token_t token,SecCodeRef *code) {
 CFDataRef data=CFDataCreate(kCFAllocatorDefault,(const UInt8 *)&token,sizeof(token));
 const void *keys[]={kSecGuestAttributeAudit}, *values[]={data};
 CFDictionaryRef attrs=CFDictionaryCreate(kCFAllocatorDefault,keys,values,1,&kCFTypeDictionaryKeyCallBacks,&kCFTypeDictionaryValueCallBacks);
 OSStatus result=SecCodeCopyGuestWithAttributes(NULL,attrs,kSecCSDefaultFlags,code);
 CFRelease(attrs); CFRelease(data); return result;
}
static OSStatus check_code(audit_token_t token,const char *requirement) {
 SecCodeRef code=NULL; OSStatus result=lookup_code(token,&code);
 if(result==errSecSuccess) {
  CFStringRef text=CFStringCreateWithCString(kCFAllocatorDefault,requirement,kCFStringEncodingUTF8);
  SecRequirementRef req=NULL; result=SecRequirementCreateWithString(text,kSecCSDefaultFlags,&req);
  if(result==errSecSuccess) { result=SecCodeCheckValidity(code,kSecCSDefaultFlags,req); CFRelease(req); }
  CFRelease(text); CFRelease(code);
 }
 return result;
}
int main(int argc,char **argv){
 setvbuf(stdout,NULL,_IONBF,0);
 if(argc==3 && strcmp(argv[1],"--helper")==0){int fd=atoi(argv[2]);printf("helper_variant=%s\n",A2S_VARIANT);send_byte(fd,'e');(void)read_byte(fd);return 0;}
 if(argc!=2 && argc!=4){fprintf(stderr,"expected requirement [replacement replacement-requirement]\n");return 1;}
 char wrong[]="identifier \"org.agent2ssh.definitely-wrong\"";
 char wrong_hash[]="identifier \"org.agent2ssh.research.audit\" and cdhash H\"0000000000000000000000000000000000000000\"";
 mach_port_t service=0,old=0; int sock[2]; if(socketpair(AF_UNIX,SOCK_STREAM,0,sock)){perror("socketpair");return 1;}
 ok("allocate",mach_port_allocate(mach_task_self(),MACH_PORT_RIGHT_RECEIVE,&service)); ok("insert",mach_port_insert_right(mach_task_self(),service,service,MACH_MSG_TYPE_MAKE_SEND));
 ok("get bootstrap",task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&old)); ok("set bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,service));
 pid_t child=fork(); if(child<0){perror("fork");return 1;}
 if(!child){close(sock[0]);mach_port_t dest=0;task_id_token_t token=0;if(task_get_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,&dest)||task_create_identity_token(mach_task_self(),&token))_exit(2);
  msg_t m={0};m.h.msgh_bits=MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND,0)|MACH_MSGH_BITS_COMPLEX;m.h.msgh_remote_port=dest;m.h.msgh_size=sizeof(m);m.h.msgh_id=4601;m.body.msgh_descriptor_count=1;m.token.name=token;m.token.disposition=MACH_MSG_TYPE_MOVE_SEND;m.token.type=MACH_MSG_PORT_DESCRIPTOR;m.marker=46;
  if(mach_msg(&m.h,MACH_SEND_MSG|MACH_SEND_TIMEOUT,sizeof(m),0,0,2000,0))_exit(3);
  send_byte(sock[1],'a');if(read_byte(sock[1])!='e')_exit(4);char fd[24];snprintf(fd,sizeof(fd),"%d",sock[1]);const char *next=argc==4?argv[2]:argv[0];execl(next,next,"--helper",fd,(char*)NULL);_exit(5);
 }
 close(sock[1]);ok("restore bootstrap",task_set_special_port(mach_task_self(),TASK_BOOTSTRAP_PORT,old));
 recv_t msg={0};mach_msg_option_t opts=MACH_RCV_MSG|MACH_RCV_TIMEOUT|MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0)|MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT);
 ok("receive",mach_msg(&msg.m.h,opts,0,sizeof(msg),service,4000,0));
 if(!(msg.m.h.msgh_bits&MACH_MSGH_BITS_COMPLEX)||msg.m.body.msgh_descriptor_count!=1||msg.m.marker!=46){fprintf(stderr,"bad message\n");return 2;}
 mach_msg_audit_trailer_t *tr=(void *)((char*)&msg.m+round_msg(msg.m.h.msgh_size));if(tr->msgh_trailer_size<sizeof(*tr)){fprintf(stderr,"short trailer\n");return 2;}
 audit_token_t audit=tr->msgh_audit; if(read_byte(sock[0])!='a')return 2;
 task_name_t name=0;ok("name port",task_identity_token_get_task_port(msg.m.token.name,TASK_FLAVOR_NAME,&name));
 audit_token_t task={0};mach_msg_type_number_t n=TASK_AUDIT_TOKEN_COUNT;ok("task audit",task_info(name,TASK_AUDIT_TOKEN,(task_info_t)&task,&n));
 int match=memcmp(&audit,&task,sizeof(audit))==0;OSStatus correct=check_code(audit,argv[1]),incorrect=check_code(audit,wrong),incorrect_hash=check_code(audit,wrong_hash);
 SecCodeRef retained=NULL; OSStatus retained_lookup=lookup_code(audit,&retained);
 CFStringRef retained_text=CFStringCreateWithCString(kCFAllocatorDefault,argv[1],kCFStringEncodingUTF8);
 SecRequirementRef retained_req=NULL; OSStatus retained_req_status=SecRequirementCreateWithString(retained_text,kSecCSDefaultFlags,&retained_req);
 if(retained_lookup!=errSecSuccess || retained_req_status!=errSecSuccess) { fprintf(stderr,"retained code setup failed\n"); close(sock[0]); waitpid(child,NULL,0); return 2; }
 printf("child_pid=%d sender_pid=%d pidversion=%d task_audit_match=%d correct_status=%d wrong_status=%d wrong_hash_status=%d\n",child,audit_token_to_pid(audit),audit_token_to_pidversion(audit),match,(int)correct,(int)incorrect,(int)incorrect_hash);
 send_byte(sock[0],'e');if(read_byte(sock[0])!='e')return 2;
 OSStatus old_status=check_code(audit,argv[1]);audit_token_t current={0};socklen_t size=sizeof(current);if(getsockopt(sock[0],SOL_LOCAL,LOCAL_PEERTOKEN,&current,&size)||size!=sizeof(current))return 2;
 OSStatus new_status=check_code(current,argv[1]);OSStatus replacement_status=argc==4?check_code(current,argv[3]):new_status;
 OSStatus retained_status=SecCodeCheckValidity(retained,kSecCSDefaultFlags,retained_req);
 printf("retained_code_after_exec_status=%d\n",(int)retained_status);
 printf("after_exec_pid=%d pidversion=%d old_audit_status=%d new_audit_status=%d waitpid_nohang=%d replacement_status=%d replacement_mode=%d\n",audit_token_to_pid(current),audit_token_to_pidversion(current),(int)old_status,(int)new_status,waitpid(child,NULL,WNOHANG),(int)replacement_status,argc==4);
 send_byte(sock[0],'x');close(sock[0]);int status=0;if(waitpid(child,&status,0)!=child||!WIFEXITED(status)||WEXITSTATUS(status))return 2;
 CFRelease(retained_req); CFRelease(retained_text); CFRelease(retained);
 (void)mach_port_deallocate(mach_task_self(),name);(void)mach_port_deallocate(mach_task_self(),msg.m.token.name);(void)mach_port_deallocate(mach_task_self(),old);(void)mach_port_mod_refs(mach_task_self(),service,MACH_PORT_RIGHT_RECEIVE,-1);(void)mach_port_deallocate(mach_task_self(),service);
 return match && correct==errSecSuccess && incorrect!=errSecSuccess && incorrect_hash!=errSecSuccess && old_status!=errSecSuccess && replacement_status==errSecSuccess && (argc==4 ? new_status!=errSecSuccess : new_status==errSecSuccess) ? 0 : 6;
}
