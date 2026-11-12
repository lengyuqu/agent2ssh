// macOS peer-credential / process-identity probe for agent2ssh issue #25.
// Read-only w.r.t. the repository; creates only transient sockets and one child.
#include <errno.h>
#include <libproc.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/proc_info.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

static const char *errname(int e) {
  switch (e) {
  case 0: return "0";
  case ESRCH: return "ESRCH";
  case EACCES: return "EACCES";
  case EPERM: return "EPERM";
  case EINVAL: return "EINVAL";
  case ENOTSUP: return "ENOTSUP";
  case ENOENT: return "ENOENT";
  case EBADF: return "EBADF";
  default: return "other";
  }
}

static int peer_pid(int fd) {
  int pid = -1;
  socklen_t len = sizeof(pid);
  errno = 0;
  int rc = getsockopt(fd, SOL_LOCAL, LOCAL_PEERPID, &pid, &len);
  if (rc != 0) return -errno;
  return pid;
}

static void probe_peer_pid(void) {
  printf("== E1 socketpair LOCAL_PEERPID (same process, two fds)\n");
  int sv[2];
  if (socketpair(AF_UNIX, SOCK_STREAM, 0, sv) != 0) { perror("socketpair"); return; }
  int a = peer_pid(sv[0]), b = peer_pid(sv[1]);
  printf("   peer_pid(sv[0]) = %d (self=%d) match=%d\n", a, getpid(), a == getpid());
  printf("   peer_pid(sv[1]) = %d (self=%d) match=%d\n", b, getpid(), b == getpid());

  printf("== E1b LOCAL_PEERCRED / LOCAL_PEERTOKEN on the same pair\n");
  struct xucred cr;
  socklen_t clen = sizeof(cr);
  errno = 0;
  if (getsockopt(sv[0], SOL_LOCAL, LOCAL_PEERCRED, &cr, &clen) == 0) {
    printf("   LOCAL_PEERCRED: version=%u uid=%u ngroups=%d\n", cr.cr_version,
           (unsigned)cr.cr_uid, cr.cr_ngroups);
  } else {
    printf("   LOCAL_PEERCRED errno=%s\n", errname(errno));
  }
  unsigned char token[64];
  socklen_t tlen = sizeof(token);
  memset(token, 0, sizeof(token));
  errno = 0;
  if (getsockopt(sv[0], SOL_LOCAL, LOCAL_PEERTOKEN, token, &tlen) == 0) {
    printf("   LOCAL_PEERTOKEN: ok, %d bytes, first8=%02x%02x%02x%02x%02x%02x%02x%02x\n",
           (int)tlen, token[0], token[1], token[2], token[3], token[4], token[5],
           token[6], token[7]);
  } else {
    printf("   LOCAL_PEERTOKEN errno=%s\n", errname(errno));
  }
  close(sv[0]);
  close(sv[1]);
}

// Does LOCAL_PEERPID follow the most recent accessor after SCM_RIGHTS hand-off?
static void probe_fd_handoff(void) {
  printf("== E2 SCM_RIGHTS hand-off: does LOCAL_PEERPID follow the latest writer?\n");
  int sv[2];
  if (socketpair(AF_UNIX, SOCK_STREAM, 0, sv) != 0) { perror("socketpair"); return; }
  int sibling[2];
  if (socketpair(AF_UNIX, SOCK_STREAM, 0, sibling) != 0) { perror("socketpair2"); return; }

  pid_t child = fork();
  if (child == 0) {
    // Child: receive the connected fd from the parent, then write one byte.
    close(sibling[0]);
    char byte = 0;
    struct iovec iov = {.iov_base = &byte, .iov_len = 1};
    char cbuf[CMSG_SPACE(sizeof(int))];
    memset(cbuf, 0, sizeof(cbuf));
    struct msghdr msg;
    memset(&msg, 0, sizeof(msg));
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cbuf;
    msg.msg_controllen = sizeof(cbuf);
    if (recvmsg(sibling[1], &msg, 0) <= 0) _exit(10);
    struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
    if (!c) _exit(11);
    int fd = *(int *)CMSG_DATA(c);
    // send on the received socket so `last_pid` becomes this child
    ssize_t n = send(fd, "x", 1, 0);
    (void)n;
    _exit(0);
  }
  close(sibling[1]);
  // Parent: sv[1] is the "server" side; sv[0] is the "client" side.
  int before = peer_pid(sv[1]);
  // Send sv[0] to the child.
  char byte = 0;
  struct iovec iov = {.iov_base = &byte, .iov_len = 1};
  char cbuf[CMSG_SPACE(sizeof(int))];
  memset(cbuf, 0, sizeof(cbuf));
  struct msghdr msg;
  memset(&msg, 0, sizeof(msg));
  msg.msg_iov = &iov;
  msg.msg_iovlen = 1;
  msg.msg_control = cbuf;
  msg.msg_controllen = sizeof(cbuf);
  struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
  c->cmsg_level = SOL_SOCKET;
  c->cmsg_type = SCM_RIGHTS;
  c->cmsg_len = CMSG_LEN(sizeof(int));
  int dupfd = sv[0];
  memcpy(CMSG_DATA(c), &dupfd, sizeof(int));
  if (sendmsg(sibling[0], &msg, 0) < 0) perror("sendmsg");
  close(sv[0]);
  char rb[4];
  ssize_t got = recv(sv[1], rb, sizeof(rb), 0);
  int after = peer_pid(sv[1]);
  int st = 0;
  waitpid(child, &st, 0);
  printf("   before hand-off: peer_pid=%d (self=%d)\n", before, getpid());
  printf("   after child send: peer_pid=%d (child=%d) bytes=%zd\n", after, (int)child, got);
  printf("   CONCLUSION: last_pid tracks the accessor, not the connector = %d\n",
         after == (int)child);
  close(sv[1]);
  close(sibling[0]);
}

static void probe_kevent(void) {
  printf("== E3 kqueue EVFILT_PROC NOTE_EXIT registration\n");
  char path[256];
  snprintf(path, sizeof(path), "/tmp/a2s-kevent-%d", getpid());
  unlink(path);
  int kq = kqueue();
  if (kq < 0) { perror("kqueue"); return; }

  struct {
    const char *label;
    pid_t pid;
  } cases[4];
  cases[0].label = "nonexistent pid (self+400000)";
  cases[0].pid = getpid() + 400000;
  cases[1].label = "pid 1 (launchd, other uid)";
  cases[1].pid = 1;
  cases[2].label = "self";
  cases[2].pid = getpid();

  // own child, sleeping
  pid_t kid = fork();
  if (kid == 0) {
    sleep(30);
    _exit(0);
  }
  cases[3].label = "own child";
  cases[3].pid = kid;

  for (int i = 0; i < 4; i++) {
    struct kevent changes[1], events[2];
    EV_SET(&changes[0], (uintptr_t)cases[i].pid, EVFILT_PROC, EV_ADD | EV_RECEIPT | EV_ONESHOT,
            NOTE_EXIT, 0, NULL);
    memset(events, 0, sizeof(events));
    errno = 0;
    int n = kevent(kq, changes, 1, events, 2, NULL);
    int saved = errno;
    if (n < 0) {
      printf("   [%s] kevent()=-1 errno=%s(%d)\n", cases[i].label, errname(saved), saved);
    } else {
      int reported_error = 0, err_val = 0;
      for (int k = 0; k < n; k++) {
        if (events[k].flags & EV_ERROR) { reported_error = 1; err_val = (int)events[k].data; }
      }
      printf("   [%s] kevent()=%d EV_ERROR=%d err=%s(%d)\n", cases[i].label, n,
             reported_error, errname(err_val), err_val);
    }
  }
  kill(kid, SIGKILL);
  waitpid(kid, NULL, 0);
  close(kq);
  (void)path;
}

static void probe_kill(void) {
  printf("== E4 kill(pid,0) permission matrix (uid=%d)\n", getuid());
  pid_t targets[3] = {getpid(), 1, getpid() + 400000};
  const char *labels[3] = {"self", "pid 1 (root)", "nonexistent"};
  for (int i = 0; i < 3; i++) {
    errno = 0;
    int rc = kill(targets[i], 0);
    printf("   [%s] kill(%d,0)=%d errno=%s(%d)\n", labels[i], (int)targets[i], rc,
           errname(errno), errno);
  }
}

static void probe_proc_pidinfo(void) {
  printf("== E5 proc_pidinfo flavors\n");
  pid_t targets[2] = {getpid(), 1};
  const char *labels[2] = {"self", "pid 1 (root)"};
  for (int i = 0; i < 2; i++) {
    struct proc_bsdinfo bi;
    memset(&bi, 0, sizeof(bi));
    errno = 0;
    int n = proc_pidinfo(targets[i], PROC_PIDTBSDINFO, 0, &bi, sizeof(bi));
    if (n == (int)sizeof(bi)) {
      printf("   [%s] PROC_PIDTBSDINFO ok: pbi_pid=%u uid=%u comm=%s start=%llu.%06llu\n",
             labels[i], bi.pbi_pid, bi.pbi_uid, bi.pbi_comm,
             (unsigned long long)bi.pbi_start_tvsec,
             (unsigned long long)bi.pbi_start_tvusec);
    } else {
      printf("   [%s] PROC_PIDTBSDINFO rc=%d errno=%s(%d)\n", labels[i], n, errname(errno),
             errno);
    }
    // flavor 17 is what XNU names PROC_PIDUNIQIDENTIFIERINFO; not in the public SDK.
    unsigned char buf[128];
    memset(buf, 0, sizeof(buf));
    errno = 0;
    int m = proc_pidinfo(targets[i], 17, 0, buf, sizeof(buf));
    if (m > 0) {
      unsigned long long u0 = 0, u1 = 0;
      memcpy(&u0, buf + 16, 8); // p_uniqueid after the 16-byte uuid
      memcpy(&u1, buf + 24, 8); // p_puniqueid
      printf("   [%s] flavor17 ok: size=%d p_uniqueid=%llu p_puniqueid=%llu\n", labels[i], m,
             u0, u1);
    } else {
      printf("   [%s] flavor17 rc=%d errno=%s(%d)\n", labels[i], m, errname(errno), errno);
    }
  }
}

int main(void) {
  printf("macOS probe pid=%d\n", getpid());
  probe_peer_pid();
  probe_fd_handoff();
  probe_kevent();
  probe_kill();
  probe_proc_pidinfo();
  return 0;
}
