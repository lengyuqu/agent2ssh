#ifndef AGENT2SSH_MACOS_CODESIGN_H
#define AGENT2SSH_MACOS_CODESIGN_H
#include <mach/mach.h>

// A successful result is only code-signing evidence for this audit token. It
// does not bind a control endpoint, prove liveness or authorize a request. The
// caller must provide a requirement from a trusted, immutable release policy.
int a2s_verify_running_code(const audit_token_t *audit, const char *requirement);
#endif
