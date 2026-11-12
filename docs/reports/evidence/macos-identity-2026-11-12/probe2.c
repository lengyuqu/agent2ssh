#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/event.h>
#include <sys/types.h>
#include <unistd.h>
static const char *en(int e){switch(e){case 0:return "0";case ESRCH:return "ESRCH";case EACCES:return "EACCES";case EPERM:return "EPERM";case EINVAL:return "EINVAL";default:return "other";}}
static void one(const char *label, pid_t pid, uint32_t flags, uint32_t fflags){
  int kq=kqueue(); struct kevent ch[1], ev[2]; memset(ev,0,sizeof(ev));
  EV_SET(&ch[0],(uintptr_t)pid,EVFILT_PROC,flags,fflags,0,NULL);
  errno=0; int n=kevent(kq,ch,1,ev,2,NULL); int e=errno;
  if(n<0){printf("  %-46s kevent=-1 errno=%s(%d)\n",label,en(e),e);}
  else{int ee=0,data=0;for(int i=0;i<n;i++){if(ev[i].flags&EV_ERROR){ee=1;data=(int)ev[i].data;}}
       printf("  %-46s n=%d EV_ERROR=%d data=%s(%d)\n",label,n,ee,en(data),data);}
  close(kq);
}
int main(void){
  printf("== E3b registration shapes (EV_RECEIPT vs plain)\n");
  one("no-EV_RECEIPT / nonexistent",getpid()+400000,EV_ADD|EV_ONESHOT,NOTE_EXIT);
  one("EV_RECEIPT / nonexistent",getpid()+400000,EV_ADD|EV_RECEIPT|EV_ONESHOT,NOTE_EXIT);
  one("EV_RECEIPT / pid 1 NOTE_EXIT",1,EV_ADD|EV_RECEIPT|EV_ONESHOT,NOTE_EXIT);
  one("EV_RECEIPT / pid 1 NOTE_EXITSTATUS",1,EV_ADD|EV_RECEIPT|EV_ONESHOT,NOTE_EXITSTATUS);
  one("EV_RECEIPT / self NOTE_EXITSTATUS",getpid(),EV_ADD|EV_RECEIPT|EV_ONESHOT,NOTE_EXITSTATUS);
  one("EV_RECEIPT / bad flags NOTE_FORK|NOTE_EXIT",getpid(),EV_ADD|EV_RECEIPT|EV_ONESHOT,NOTE_FORK);
  return 0;
}
