/* Minimal check that DYLD interpose actually fires on this host.
 * Build: clang -O0 -o hook_smoke hook_smoke.c
 */
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(void) {
    const char *dir = getenv("A2S_HOOK_DIR");
    if (!dir) return 2;
    char tmp[4096], hosts[4096], log[4096];
    snprintf(tmp, sizeof(tmp), "%s/hosts.json.tmp-smoke", dir);
    snprintf(hosts, sizeof(hosts), "%s/hosts.json", dir);
    snprintf(log, sizeof(log), "%s/.commit.json", dir);
    int fd = open(tmp, O_CREAT | O_WRONLY, 0600);
    if (fd >= 0) close(fd);
    if (rename(tmp, hosts) != 0) perror("rename");
    fd = open(log, O_CREAT | O_WRONLY, 0600);
    if (fd >= 0) close(fd);
    if (unlink(log) != 0) perror("unlink");
    puts("smoke completed (hook did not fire)");
    return 0;
}
