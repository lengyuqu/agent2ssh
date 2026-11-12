/*
 * Test-only fault injection for the two-file hosts.json + secrets.enc commit.
 * Compiled and loaded into the real CLI process by
 * `tests/store_crash_injection.rs` (and by the evidence script for issue #28).
 * No production code is modified.
 *
 * The library interposes two libc entry points. At a selected point of the
 * commit it writes `A2S_HOOK_DIR/<stage>.fired` and pauses; the driver sends
 * SIGKILL from outside the process. SIGKILL is uncatchable, so the pause never
 * returns - the process dies with the exact on-disk state of that point.
 *
 * Stages (A2S_HOOK_STAGE):
 *   before-rename  immediately before rename(tmp, hosts.json),
 *                  i.e. after store_commit::begin()
 *   before-finish  immediately before unlink(.commit.json),
 *                  i.e. after the hosts rename but before finish()
 *
 * The replacement functions call the equivalent *at() entry points rather
 * than resolving the interposed symbols with dlsym: RTLD_NEXT is NULL for an
 * inserted image on macOS, which produced a SIGSEGV in the smoke check.
 */
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static const char *base_of(const char *path) {
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

static void fire_if_selected(const char *stage) {
    const char *want = getenv("A2S_HOOK_STAGE");
    const char *dir = getenv("A2S_HOOK_DIR");
    if (!want || !dir || strcmp(want, stage) != 0) return;

    char path[4096];
    snprintf(path, sizeof(path), "%s/%s.fired", dir, stage);
    int fd = open(path, O_CREAT | O_WRONLY | O_TRUNC, 0600);
    if (fd >= 0) {
        (void)write(fd, "1", 1);
        close(fd);
    }
    for (;;) pause(); /* the driver sends SIGKILL; this never returns */
}

int a2s_rename(const char *from, const char *to) {
    if (strcmp(base_of(to), "hosts.json") == 0) fire_if_selected("before-rename");
    return renameat(AT_FDCWD, from, AT_FDCWD, to);
}

int a2s_unlink(const char *path) {
    if (strcmp(base_of(path), ".commit.json") == 0) fire_if_selected("before-finish");
    return unlinkat(AT_FDCWD, path, 0);
}

__attribute__((used)) static struct {
    const void *replacement;
    const void *replacee;
} a2s_interpose[] __attribute__((section("__DATA,__interpose"))) = {
    {(const void *)(uintptr_t)a2s_rename, (const void *)(uintptr_t)rename},
    {(const void *)(uintptr_t)a2s_unlink, (const void *)(uintptr_t)unlink},
};
