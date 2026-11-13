#include "macos_exit_observer.h"
#include "macos_codesign.h"
#include <mach/task.h>
#include <sys/event.h>
#include <fcntl.h>
#include <stdlib.h>
#include <unistd.h>

struct a2s_exit_observer {
    int queue;
    pid_t pid;
    mach_port_t task_name;
};

int a2s_exit_observer_create(const audit_token_t *audit, mach_port_t task_identity,
                             pid_t expected_pid, a2s_exit_observer **out) {
    if (!out) return -1;
    *out = NULL;
    if (!audit || !MACH_PORT_VALID(task_identity) || expected_pid <= 0) return -1;
    task_name_t before = MACH_PORT_NULL, after = MACH_PORT_NULL;
    int queue = -1;
    a2s_exit_observer *observer = NULL;
    if (task_identity_token_get_task_port(task_identity, TASK_FLAVOR_NAME, &before) != KERN_SUCCESS ||
        !a2s_task_audit_matches(audit, before, expected_pid)) goto fail;
    (void)mach_port_deallocate(mach_task_self(), before);
    before = MACH_PORT_NULL;
    queue = kqueue();
    if (queue < 0) goto fail;
    int flags = fcntl(queue, F_GETFD);
    if (flags < 0 || fcntl(queue, F_SETFD, flags | FD_CLOEXEC) < 0) goto fail;
    struct kevent registration;
    EV_SET(&registration, (uintptr_t)expected_pid, EVFILT_PROC, EV_ADD | EV_ENABLE | EV_CLEAR, NOTE_EXIT, 0, NULL);
    if (kevent(queue, &registration, 1, NULL, 0, NULL) < 0) goto fail;
    if (task_identity_token_get_task_port(task_identity, TASK_FLAVOR_NAME, &after) != KERN_SUCCESS ||
        !a2s_task_audit_matches(audit, after, expected_pid)) goto fail;
    observer = calloc(1, sizeof(*observer));
    if (!observer) goto fail;
    observer->queue = queue;
    observer->pid = expected_pid;
    observer->task_name = after;
    *out = observer;
    return 0;
fail:
    if (MACH_PORT_VALID(before)) (void)mach_port_deallocate(mach_task_self(), before);
    if (MACH_PORT_VALID(after)) (void)mach_port_deallocate(mach_task_self(), after);
    if (queue >= 0) (void)close(queue);
    free(observer);
    return -1;
}

int a2s_exit_observer_wait(a2s_exit_observer *observer, uint32_t timeout_ms) {
    if (!observer || observer->queue < 0) return -1;
    struct timespec timeout = { .tv_sec = (time_t)(timeout_ms / 1000), .tv_nsec = (long)(timeout_ms % 1000) * 1000000L };
    struct kevent event = {0};
    int result = kevent(observer->queue, NULL, 0, &event, 1, &timeout);
    if (result == 0) return 0;
    if (result != 1 || event.filter != EVFILT_PROC || event.ident != (uintptr_t)observer->pid ||
        (event.flags & EV_ERROR) || !(event.fflags & NOTE_EXIT)) return -1;
    return 1;
}

void a2s_exit_observer_destroy(a2s_exit_observer *observer) {
    if (!observer) return;
    if (observer->queue >= 0) (void)close(observer->queue);
    if (MACH_PORT_VALID(observer->task_name)) (void)mach_port_deallocate(mach_task_self(), observer->task_name);
    free(observer);
}
