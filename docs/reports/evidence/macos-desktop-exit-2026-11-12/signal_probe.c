// Disposable macOS daemon stand-in for identifying a shutdown signal sender.
// Only binds 127.0.0.1:7722 and writes to the explicit test config directory.
#include <errno.h>
#include <limits.h>
#include <netinet/in.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

static volatile sig_atomic_t signal_seen, signal_sender, signal_sender_uid;
static void on_signal(int sig, siginfo_t *info, void *context) {
    (void)context;
    signal_seen = sig;
    signal_sender = info ? info->si_pid : -1;
    signal_sender_uid = info ? info->si_uid : -1;
}
static void fail(const char *message) { perror(message); exit(1); }
int main(void) {
    const char *config = getenv("AGENT2SSH_CONFIG_DIR");
    if (!config || strncmp(config, "/tmp/a2s-signal-check.", 22) != 0) {
        fprintf(stderr, "refusing to run outside disposable test config\n"); return 2;
    }
    char resolved[PATH_MAX], path[PATH_MAX];
    if (!realpath(config, resolved)) fail("realpath");
    struct sigaction action = {0};
    action.sa_sigaction = on_signal; action.sa_flags = SA_SIGINFO;
    sigemptyset(&action.sa_mask);
    if (sigaction(SIGTERM, &action, NULL) || sigaction(SIGINT, &action, NULL)) fail("sigaction");
    signal(SIGHUP, SIG_IGN);
    int listener = socket(AF_INET, SOCK_STREAM, 0); if (listener < 0) fail("socket");
    int one = 1; setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    struct sockaddr_in addr = {0}; addr.sin_family = AF_INET; addr.sin_port = htons(7722); addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (bind(listener, (struct sockaddr *)&addr, sizeof(addr)) || listen(listener, 16)) fail("bind/listen");
    snprintf(path, sizeof(path), "%s/daemon.pid", config);
    FILE *f = fopen(path, "w"); if (!f) fail("daemon.pid"); fprintf(f, "%d", getpid()); fclose(f);
    snprintf(path, sizeof(path), "%s/daemon.token", config);
    f = fopen(path, "w"); if (!f) fail("daemon.token"); fputs("synthetic-test-token\n", f); fclose(f);
    while (!signal_seen) {
        fd_set readset; FD_ZERO(&readset); FD_SET(listener, &readset);
        struct timeval wait = {0, 250000};
        int ready = select(listener + 1, &readset, NULL, NULL, &wait);
        if (ready < 0) { if (errno == EINTR) continue; fail("select"); }
        if (!ready) continue;
        int client = accept(listener, NULL, NULL); if (client < 0) { if (errno == EINTR) continue; fail("accept"); }
        struct timeval timeout = {1,0}; setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
        char request[4096] = {0}; ssize_t n = read(client, request, sizeof(request)-1);
        if (n > 0) {
            char body[PATH_MAX+256];
            if (strncmp(request, "GET /health ", 12) == 0) {
                snprintf(body, sizeof(body), "{\"ok\":true,\"pid\":%d,\"config_dir\":\"%s\",\"version\":\"probe\"}", getpid(), resolved);
            } else if (strncmp(request, "GET /gate ", 10) == 0) {
                snprintf(body, sizeof(body), "{\"mode\":\"active\"}");
            } else {
                snprintf(body, sizeof(body), "[]");
            }
            char header[256]; int len = snprintf(header, sizeof(header), "HTTP/1.1 200 OK\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n", strlen(body));
            (void)write(client, header, (size_t)len); (void)write(client, body, strlen(body));
        }
        close(client);
    }
    snprintf(path, sizeof(path), "%s/signal.txt", config);
    f = fopen(path, "w"); if (!f) fail("signal.txt");
    fprintf(f, "signal=%d sender_pid=%d sender_uid=%d target_pid=%d\n", signal_seen, signal_sender, signal_sender_uid, getpid()); fclose(f);
    snprintf(path, sizeof(path), "%s/daemon.pid", config); unlink(path);
    close(listener);
    return 0;
}
