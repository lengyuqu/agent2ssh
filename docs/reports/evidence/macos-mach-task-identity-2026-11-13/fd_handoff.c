// Isolated socket hand-off experiment: all processes, bytes and FDs are created
// by this probe. No credentials, daemon, external process or signal is used.
#include <bsm/libbsm.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

static void die(const char *s) { perror(s); exit(1); }
static void put(int fd, char c) { if (write(fd, &c, 1) != 1) die("write"); }
static char get(int fd) { char c; if (read(fd, &c, 1) != 1) die("read"); return c; }
static pid_t print_peer(const char *phase, int fd) {
    audit_token_t token = {0}; socklen_t len = sizeof(token);
    int pid = -1; socklen_t plen = sizeof(pid);
    errno=0; int rc=getsockopt(fd,SOL_LOCAL,LOCAL_PEERTOKEN,&token,&len); int err=errno;
    int prc=getsockopt(fd,SOL_LOCAL,LOCAL_PEERPID,&pid,&plen);
    printf("%s audit_rc=%d errno=%d audit_len=%u audit_pid=%d pidversion=%d peer_pid_rc=%d peer_pid=%d\n",phase,rc,err,len,rc ? -1 : audit_token_to_pid(token),rc ? -1 : audit_token_to_pidversion(token),prc,pid);
    if (rc || prc || len != sizeof(token) || plen != sizeof(pid) || pid != audit_token_to_pid(token)) return -1;
    return pid;
}
int main(void) {
    setvbuf(stdout,NULL,_IONBF,0);
    int sock[2], ready[2], done[2], arm[2], finish[2], release[2];
    if (socketpair(AF_UNIX,SOCK_STREAM,0,sock) || pipe(ready) || pipe(done) || pipe(arm) || pipe(finish) || pipe(release)) die("create IPC");
    pid_t server=fork(); if(server<0) die("fork server");
    if(!server) {
        close(sock[0]); close(ready[0]); close(done[0]); close(arm[1]); close(finish[1]); close(release[1]);
        put(sock[1],'a');
        pid_t worker=fork(); if(worker<0) die("fork worker");
        if(!worker) {
            close(release[0]);
            pid_t self=getpid();
            if(write(ready[1],&self,sizeof(self))!=(ssize_t)sizeof(self)) die("ready");
            (void)get(arm[0]);
            char byte=get(sock[1]);
            printf("worker_pid=%d received_synthetic_byte=%c\n",getpid(),byte);
            if(byte!='T') _exit(3);
            put(done[1],'d'); (void)get(finish[0]); _exit(0);
        }
        close(ready[1]); close(done[1]); close(arm[0]); close(finish[0]);
        (void)get(release[0]);
        int worker_status=0;
        if(waitpid(worker,&worker_status,0)!=worker || !WIFEXITED(worker_status) || WEXITSTATUS(worker_status)) die("wait worker");
        close(sock[1]); _exit(0);
    }
    close(sock[1]); close(ready[1]); close(done[1]); close(arm[0]); close(finish[0]); close(release[0]);
    if(get(sock[0])!='a') die("greeting");
    pid_t worker=0; if(read(ready[0],&worker,sizeof(worker))!=(ssize_t)sizeof(worker)) die("read ready");
    printf("server_pid=%d worker_pid=%d\n",server,worker);
    pid_t before=print_peer("before_client_sends",sock[0]);
    put(arm[1],'x');
    put(sock[0],'T');
    if(get(done[0])!='d') die("worker done");
    pid_t after=print_peer("after_worker_read",sock[0]);
    put(finish[1],'x'); put(release[1],'x');
    int status=0; if(waitpid(server,&status,0)!=server || !WIFEXITED(status) || WEXITSTATUS(status)) die("wait server");
    close(sock[0]); close(ready[0]); close(done[0]); close(arm[1]); close(finish[1]); close(release[1]);
    if (before != server || after != worker) { fprintf(stderr,"unexpected peer observation\n"); return 2; }
    return 0;
}
