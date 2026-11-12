// macOS #25 follow-up. Uses only child processes and a private temporary socket.
#include <errno.h>
#include <limits.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

static void die(const char *s) { perror(s); exit(1); }
static void byte_write(int fd, char value) { if (write(fd, &value, 1) != 1) die("write"); }
static char byte_read(int fd) { char c; if (read(fd, &c, 1) != 1) die("read"); return c; }
static int peer_pid(int fd) {
    int pid = -1; socklen_t size = sizeof(pid);
    if (getsockopt(fd, SOL_LOCAL, LOCAL_PEERPID, &pid, &size)) die("LOCAL_PEERPID");
    return pid;
}
static void kq_case(const char *name, pid_t pid, uint32_t notes) {
    int q = kqueue(); if (q < 0) die("kqueue");
    struct kevent change, result;
    EV_SET(&change, (uintptr_t)pid, EVFILT_PROC, EV_ADD | EV_RECEIPT | EV_ONESHOT, notes, 0, NULL);
    errno = 0; int n = kevent(q, &change, 1, &result, 1, NULL); int saved = errno;
    if (n == 1) printf("kqueue target=%s pid=%d notes=0x%x returned=%d ev_error=%d data=%lld\n", name, pid, notes, n, !!(result.flags & EV_ERROR), (long long)result.data);
    else printf("kqueue target=%s pid=%d notes=0x%x returned=%d errno=%d\n", name, pid, notes, n, saved);
    close(q);
}
static void signal_check(const char *name, pid_t pid) {
    errno=0; int n=kill(pid, 0); int saved=errno;
    printf("kill0 target=%s pid=%d returned=%d errno=%d\n", name, pid, n, saved);
}
int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    printf("self pid=%d uid=%d NOTE_EXIT=0x%x NOTE_EXITSTATUS=0x%x\n", getpid(), getuid(), NOTE_EXIT, NOTE_EXITSTATUS);
    int sync[2]; if (pipe(sync)) die("pipe");
    pid_t child=fork(); if (child < 0) die("fork");
    if (!child) { close(sync[1]); (void)byte_read(sync[0]); _exit(0); }
    close(sync[0]);
    pid_t ids[]={getpid(),child,1,INT_MAX};
    const char *names[]={"self","child","pid1","missing"};
    for (size_t i=0;i<4;i++) {
        signal_check(names[i],ids[i]);
        kq_case(names[i],ids[i],NOTE_EXIT);
        kq_case(names[i],ids[i],NOTE_EXITSTATUS);
        kq_case(names[i],ids[i],NOTE_EXIT|NOTE_EXITSTATUS);
    }
    byte_write(sync[1], 'x'); close(sync[1]); if (waitpid(child,NULL,0)<0) die("waitpid");

    char dir[]="/tmp/a2s-peer-XXXXXX"; if (!mkdtemp(dir)) die("mkdtemp");
    char path[sizeof(((struct sockaddr_un *)0)->sun_path)];
    if (snprintf(path,sizeof(path),"%s/sock",dir) >= (int)sizeof(path)) die("path");
    int listener=socket(AF_UNIX,SOCK_STREAM,0); if(listener<0) die("socket");
    struct sockaddr_un addr={0}; addr.sun_family=AF_UNIX; strlcpy(addr.sun_path,path,sizeof(addr.sun_path));
    if(bind(listener,(struct sockaddr *)&addr,sizeof(addr)) || listen(listener,1)) die("bind/listen");
    int ready[2]; if(pipe(ready)) die("pipe");
    pid_t server=fork(); if(server<0) die("fork server");
    if(!server) {
        close(ready[0]);
        int conn=accept(listener,NULL,NULL); if(conn<0) die("accept");
        printf("server pid=%d peer_pid=%d expected_client=%d\n",getpid(),peer_pid(conn),getppid());
        byte_write(ready[1], 'a');
        (void)byte_read(conn); byte_write(conn,'s');
        (void)byte_read(conn);
        int release[2]; if(pipe(release)) die("pipe worker");
        pid_t worker=fork(); if(worker<0) die("fork worker");
        if(!worker) { close(release[1]); byte_write(conn,'w'); (void)byte_read(release[0]); _exit(0); }
        close(release[0]);
        printf("server pid=%d inherited_fd_writer=%d\n",getpid(),worker);
        byte_write(ready[1], 'b');
        // Keep the server and accepted socket alive while the client observes.
        (void)byte_read(conn);
        byte_write(release[1], 'x'); close(release[1]);
        if(waitpid(worker,NULL,0)<0) die("wait worker");
        close(conn); close(listener); close(ready[1]); _exit(0);
    }
    close(ready[1]); close(listener);
    int client=socket(AF_UNIX,SOCK_STREAM,0); if(client<0) die("socket client");
    if(connect(client,(struct sockaddr *)&addr,sizeof(addr))) die("connect");
    (void)byte_read(ready[0]);
    printf("client pid=%d expected_server=%d peer_after_accept=%d\n",getpid(),server,peer_pid(client));
    byte_write(client,'q'); if(byte_read(client)!='s') die("server reply");
    printf("client expected_server=%d peer_after_server_reply=%d\n",server,peer_pid(client));
    byte_write(client,'q'); if(byte_read(client)!='w') die("worker reply");
    (void)byte_read(ready[0]);
    printf("client expected_server=%d peer_after_inherited_fd_reply=%d\n",server,peer_pid(client));
    byte_write(client,'x'); close(client); close(ready[0]);
    int status; if(waitpid(server,&status,0)<0 || !WIFEXITED(status) || WEXITSTATUS(status)) die("wait server");
    if(unlink(path) || rmdir(dir)) die("cleanup");
    return 0;
}
