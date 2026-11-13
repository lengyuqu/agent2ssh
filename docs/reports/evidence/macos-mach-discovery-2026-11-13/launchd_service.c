// Temporary test-only launchd MachServices receiver. No token or daemon data.
#include <mach/mach.h>
#include <servers/bootstrap.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

typedef struct {mach_msg_header_t h;unsigned marker;} msg_t;
int main(int argc,char **argv){
 if(argc!=3)return 1;
 FILE *log=fopen(argv[2],"a");if(!log)return 2;setvbuf(log,NULL,_IONBF,0);
 mach_port_t receive=MACH_PORT_NULL;kern_return_t checkin=bootstrap_check_in(bootstrap_port,argv[1],&receive);
 kern_return_t guard=checkin?checkin:mach_port_guard_with_flags(mach_task_self(),receive,0xa522,MPG_IMMOVABLE_RECEIVE);
 fprintf(log,"start pid=%d checkin=%d guard=%d\n",getpid(),checkin,guard);
 if(checkin||guard){fclose(log);return 3;}
 struct {msg_t msg;char trailer[128];} message={0};
 kern_return_t kr=mach_msg(&message.msg.h,MACH_RCV_MSG|MACH_RCV_TIMEOUT,0,sizeof(message),receive,10000,0);
 fprintf(log,"message pid=%d kr=%d id=%d marker=%u\n",getpid(),kr,kr?0:message.msg.h.msgh_id,kr?0:message.msg.marker);
 fclose(log);return kr?4:0;
}
