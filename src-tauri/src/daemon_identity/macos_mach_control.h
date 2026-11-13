#ifndef AGENT2SSH_MACOS_MACH_CONTROL_H
#define AGENT2SSH_MACOS_MACH_CONTROL_H
#include <stdint.h>

// Code signature is intentionally not an input. Binding is the private Mach
// port plus the kernel audit token and a post-registration task recheck.
int a2s_mach_control_start(const char *service, const char *instance, const char *token);

// 0 = accepted and NOTE_EXIT observed. 1 = refused before the token was sent.
// 2 = the stop may have been sent and the exit was not observed. *token_sent
// is 0 or 1. A dead-name notification is not treated as process exit.
int a2s_mach_control_stop(const char *service, const char *instance, uint32_t pid,
                          const char *token, int *token_sent);
#endif
