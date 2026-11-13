#ifndef AGENT2SSH_MACOS_EXIT_OBSERVER_H
#define AGENT2SSH_MACOS_EXIT_OBSERVER_H
#include <mach/mach.h>
#include <stdint.h>
#include <sys/types.h>

typedef struct a2s_exit_observer a2s_exit_observer;

// Register NOTE_EXIT and re-check the original task identity. Neither a
// successful result nor an event alone proves endpoint binding or permits a
// signal/record cleanup. The input token and audit are borrowed.
int a2s_exit_observer_create(const audit_token_t *audit, mach_port_t task_identity,
                             pid_t expected_pid, a2s_exit_observer **out);
// 1 = matching NOTE_EXIT observed, 0 = timeout, -1 = error/unknown.
int a2s_exit_observer_wait(a2s_exit_observer *observer, uint32_t timeout_ms);
void a2s_exit_observer_destroy(a2s_exit_observer *observer);
#endif
