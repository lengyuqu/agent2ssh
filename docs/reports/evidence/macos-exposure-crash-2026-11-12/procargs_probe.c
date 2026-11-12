/*
 * CNB issue #28 - macOS cross-process argv/environ exposure probe.
 *
 * Reads KERN_PROCARGS2 for a given pid and prints every NUL-separated string
 * in the returned buffer (exec path, argv, then environ). No credential is
 * used; the caller supplies synthetic markers and greps for them.
 *
 * Build: clang -O0 -o procargs_probe procargs_probe.c
 * Usage: ./procargs_probe <pid>
 */
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/sysctl.h>

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <pid>\n", argv[0]);
        return 2;
    }
    int pid = atoi(argv[1]);
    int name[3] = {CTL_KERN, KERN_PROCARGS2, pid};
    size_t len = 0;
    if (sysctl(name, 3, NULL, &len, NULL, 0) == -1) {
        printf("ERROR size errno=%d (%s)\n", errno, strerror(errno));
        return 0;
    }
    if (len == 0) {
        printf("ERROR empty buffer\n");
        return 0;
    }
    char *buf = calloc(1, len + 1);
    if (!buf) {
        printf("ERROR calloc\n");
        return 1;
    }
    if (sysctl(name, 3, buf, &len, NULL, 0) == -1) {
        printf("ERROR read errno=%d (%s)\n", errno, strerror(errno));
        free(buf);
        return 0;
    }
    int target_argc = 0;
    memcpy(&target_argc, buf, sizeof(int));
    printf("len=%zu argc=%d\n", len, target_argc);

    size_t i = sizeof(int);
    while (i < len) {
        size_t start = i;
        while (i < len && buf[i] != '\0') i++;
        if (i > start) {
            /* Print only printable strings; never dump binary bytes. */
            int printable = 1;
            for (size_t j = start; j < i; j++) {
                unsigned char c = (unsigned char)buf[j];
                if (c < 0x20 || c > 0x7e) { printable = 0; break; }
            }
            if (printable) printf("S: %.*s\n", (int)(i - start), buf + start);
        }
        i++;
    }
    free(buf);
    return 0;
}
