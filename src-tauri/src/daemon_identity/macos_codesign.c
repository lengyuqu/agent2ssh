#include "macos_codesign.h"
#include <Security/Security.h>
#include <CoreFoundation/CoreFoundation.h>
#include <bsm/libbsm.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>

_Static_assert(sizeof(audit_token_t) == sizeof(uint32_t) * 8, "unexpected Darwin audit token size");
_Static_assert(_Alignof(audit_token_t) == _Alignof(uint32_t), "unexpected Darwin audit token alignment");

int a2s_task_audit_matches(const audit_token_t *audit, mach_port_t name, pid_t expected_pid) {
    if (!audit || !MACH_PORT_VALID(name) || expected_pid <= 0 ||
        audit_token_to_euid(*audit) != geteuid() || audit_token_to_pid(*audit) != expected_pid) return 0;
    audit_token_t task_audit = {0};
    mach_msg_type_number_t count = TASK_AUDIT_TOKEN_COUNT;
    if (task_info(name, TASK_AUDIT_TOKEN, (task_info_t)&task_audit, &count) != KERN_SUCCESS ||
        count != TASK_AUDIT_TOKEN_COUNT) return 0;
    return memcmp(audit, &task_audit, sizeof(*audit)) == 0;
}

// A token is meaningful only when the caller got it from the kernel for the
// relevant message. A matching code signature alone is not endpoint binding.
int a2s_verify_running_code(const audit_token_t *audit, const char *requirement) {
    if (!audit || !requirement || !*requirement) return 0;
    CFDataRef data = NULL;
    CFDictionaryRef attrs = NULL, info = NULL;
    CFStringRef text = NULL;
    SecCodeRef code = NULL;
    SecRequirementRef req = NULL;
    int good = 0;

    data = CFDataCreate(NULL, (const UInt8 *)audit, sizeof(*audit));
    if (!data) goto done;
    const void *keys[] = { kSecGuestAttributeAudit }, *values[] = { data };
    attrs = CFDictionaryCreate(NULL, keys, values, 1, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    if (!attrs || SecCodeCopyGuestWithAttributes(NULL, attrs, kSecCSDefaultFlags, &code) != errSecSuccess || !code) goto done;
    text = CFStringCreateWithCString(NULL, requirement, kCFStringEncodingUTF8);
    if (!text || SecRequirementCreateWithString(text, kSecCSDefaultFlags, &req) != errSecSuccess || !req) goto done;
    if (SecCodeCheckValidity(code, kSecCSDefaultFlags, req) != errSecSuccess) goto done;
    if (SecCodeCopySigningInformation(code, kSecCSSigningInformation | kSecCSDynamicInformation, &info) != errSecSuccess || !info) goto done;

    CFNumberRef f = CFDictionaryGetValue(info, kSecCodeInfoFlags);
    CFNumberRef s = CFDictionaryGetValue(info, kSecCodeInfoStatus);
    int64_t flags = 0, status = 0;
    if (!f || CFGetTypeID(f) != CFNumberGetTypeID() || !CFNumberGetValue(f, kCFNumberSInt64Type, &flags) || !(flags & kSecCodeSignatureRuntime) ||
        !s || CFGetTypeID(s) != CFNumberGetTypeID() || !CFNumberGetValue(s, kCFNumberSInt64Type, &status) || !(status & kSecCodeStatusValid) || (status & kSecCodeStatusDebugged)) goto done;

    CFDictionaryRef ent = CFDictionaryGetValue(info, kSecCodeInfoEntitlementsDict);
    if (ent && CFGetTypeID(ent) == CFDictionaryGetTypeID()) {
        const CFStringRef forbidden[] = {
            CFSTR("com.apple.security.cs.allow-dyld-environment-variables"),
            CFSTR("com.apple.security.cs.disable-library-validation"),
            CFSTR("com.apple.security.cs.allow-unsigned-executable-memory"),
            CFSTR("com.apple.security.cs.allow-jit"),
            CFSTR("com.apple.security.cs.disable-executable-page-protection"),
            CFSTR("com.apple.security.get-task-allow"),
        };
        for (unsigned i = 0; i < sizeof(forbidden) / sizeof(forbidden[0]); i++) {
            CFTypeRef value = CFDictionaryGetValue(ent, forbidden[i]);
            if (value && (CFGetTypeID(value) != CFBooleanGetTypeID() || CFBooleanGetValue(value))) goto done;
        }
    } else if (ent || CFDictionaryContainsKey(info, kSecCodeInfoEntitlements)) {
        goto done;
    }
    good = 1;
done:
    if (info) CFRelease(info);
    if (req) CFRelease(req);
    if (text) CFRelease(text);
    if (code) CFRelease(code);
    if (attrs) CFRelease(attrs);
    if (data) CFRelease(data);
    return good;
}
