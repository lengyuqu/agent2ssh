// Separate from the caller so the empty body is not inlined over the
// daemon's strong definition. A release signature is not involved.
__attribute__((weak, noinline)) void a2s_mach_control_notify_shutdown(void) {}
