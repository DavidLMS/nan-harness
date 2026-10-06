#import <CoreGraphics/CoreGraphics.h>

#include <cassert>
#include <cstdint>
#include <initializer_list>
#include <string>
#include <vector>

#include "../inventory.hpp"

static CFDictionaryRef window(std::int64_t owner, bool on_screen) {
    const void* keys[] = {kCGWindowOwnerPID, kCGWindowIsOnscreen};
    auto owner_number = CFNumberCreate(nullptr, kCFNumberSInt64Type, &owner);
    const void* values[] = {owner_number, on_screen ? kCFBooleanTrue : kCFBooleanFalse};
    auto result = CFDictionaryCreate(nullptr, keys, values, 2, &kCFTypeDictionaryKeyCallBacks,
                                     &kCFTypeDictionaryValueCallBacks);
    CFRelease(owner_number);
    return result;
}

static CFArrayRef array(std::initializer_list<CFDictionaryRef> values) {
    std::vector<const void*> entries(values.begin(), values.end());
    return CFArrayCreate(nullptr, entries.data(), entries.size(), &kCFTypeArrayCallBacks);
}

static void expect(CFArrayRef values, pid_t pid, const char* state) {
    assert(std::string(classify_window_inventory(values, pid)) == state);
    if (values) CFRelease(values);
}

int main() {
    constexpr pid_t target = 42;
    auto unrelated = window(7, true);
    auto onscreen = window(target, true);
    auto offscreen = window(target, false);
    expect(array({}), target, "absent");
    expect(array({unrelated}), target, "absent");
    const void* unrelated_keys[] = {kCGWindowOwnerPID};
    auto unrelated_pid = static_cast<std::int64_t>(7);
    auto unrelated_owner = CFNumberCreate(nullptr, kCFNumberSInt64Type, &unrelated_pid);
    const void* unrelated_values[] = {unrelated_owner};
    auto unrelated_missing_visibility = CFDictionaryCreate(nullptr, unrelated_keys, unrelated_values,
                                                            1, &kCFTypeDictionaryKeyCallBacks,
                                                            &kCFTypeDictionaryValueCallBacks);
    CFRelease(unrelated_owner);
    expect(array({unrelated_missing_visibility}), target, "absent");
    expect(array({onscreen}), target, "present-onscreen");
    expect(array({offscreen}), target, "present-offscreen");
    expect(array({onscreen, offscreen}), target, "query-unavailable");

    const void* missing_owner_keys[] = {kCGWindowIsOnscreen};
    const void* missing_owner_values[] = {kCFBooleanTrue};
    auto missing_owner = CFDictionaryCreate(nullptr, missing_owner_keys, missing_owner_values, 1,
                                             &kCFTypeDictionaryKeyCallBacks,
                                             &kCFTypeDictionaryValueCallBacks);
    expect(array({missing_owner}), target, "query-unavailable");
    auto invalid_owner = CFStringCreateWithCString(nullptr, "42", kCFStringEncodingUTF8);
    const void* invalid_keys[] = {kCGWindowOwnerPID};
    const void* invalid_values[] = {invalid_owner};
    auto invalid = CFDictionaryCreate(nullptr, invalid_keys, invalid_values, 1,
                                      &kCFTypeDictionaryKeyCallBacks,
                                      &kCFTypeDictionaryValueCallBacks);
    expect(array({invalid}), target, "query-unavailable");
    const void* invalid_visibility_keys[] = {kCGWindowOwnerPID, kCGWindowIsOnscreen};
    auto target_number = CFNumberCreate(nullptr, kCFNumberSInt64Type, &target);
    const void* invalid_visibility_values[] = {target_number, invalid_owner};
    auto invalid_visibility = CFDictionaryCreate(nullptr, invalid_visibility_keys,
                                                 invalid_visibility_values, 2,
                                                 &kCFTypeDictionaryKeyCallBacks,
                                                 &kCFTypeDictionaryValueCallBacks);
    CFRelease(target_number);
    expect(array({invalid_visibility}), target, "query-unavailable");

    auto capped = CFArrayCreateMutable(nullptr, 0, &kCFTypeArrayCallBacks);
    for (int index = 0; index < 1025; ++index) CFArrayAppendValue(capped, unrelated);
    expect(capped, target, "query-unavailable");
    expect(nullptr, target, "query-unavailable");

    CFRelease(invalid_visibility);
    CFRelease(invalid);
    CFRelease(invalid_owner);
    CFRelease(missing_owner);
    CFRelease(offscreen);
    CFRelease(onscreen);
    CFRelease(unrelated);
}
