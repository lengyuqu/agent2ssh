// Harmless marker for an isolated DYLD_INSERT_LIBRARIES experiment.
#include <unistd.h>
__attribute__((constructor)) static void marker(void) {
    static const char text[] = "A2S_SYNTHETIC_INJECTED_LIBRARY\n";
    (void)write(STDOUT_FILENO, text, sizeof(text) - 1);
}
