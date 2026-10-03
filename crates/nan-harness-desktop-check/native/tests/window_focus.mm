// Synthetic metadata only; never queries applications, windows or accessibility.
#import <CoreGraphics/CoreGraphics.h>
#import <ApplicationServices/ApplicationServices.h>
#include <cassert>
#include <cstdint>
#include <string>

std::uint64_t match_focus_window(CFArrayRef, pid_t, CGRect, unsigned&);
const char* classify_focus_agreement(bool, unsigned);
const char* classify_focus_change(bool, bool, bool, bool);
static CFDictionaryRef record(std::int64_t pid, std::int64_t id, std::int64_t layer, CGRect bounds) {
    const void* keys[] = {kCGWindowOwnerPID, kCGWindowNumber, kCGWindowLayer, kCGWindowBounds};
    auto owner = CFNumberCreate(nullptr, kCFNumberSInt64Type, &pid);
    auto number = CFNumberCreate(nullptr, kCFNumberSInt64Type, &id);
    auto level = CFNumberCreate(nullptr, kCFNumberSInt64Type, &layer);
    auto rectangle = CGRectCreateDictionaryRepresentation(bounds);
    const void* values[] = {owner, number, level, rectangle};
    auto result = CFDictionaryCreate(nullptr, keys, values, 4, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    CFRelease(owner); CFRelease(number); CFRelease(level); CFRelease(rectangle);
    return result;
}
const char* classify_ax_error(AXError);
bool chat_current_page(CFTypeRef);
bool chat_control_agreement(unsigned, unsigned, bool, CGRect, CGRect, CGRect);
int main() {
    assert(classify_focus_change(true, true, true, true) == nullptr);
    assert(std::string(classify_focus_change(false, false, false, false)) == "foreground-changed");
    assert(std::string(classify_focus_change(true, false, false, false)) == "after-proof-unready");
    assert(std::string(classify_focus_change(true, true, false, false)) == "window-element-changed");
    assert(std::string(classify_focus_change(true, true, true, false)) == "geometry-changed");
    assert(chat_current_page(CFSTR("page")));
    assert(!chat_current_page(CFSTR("true")));
    assert(!chat_current_page(CFSTR("false")));
    assert(!chat_current_page(CFSTR("PRIVATE")));
    assert(!chat_current_page(kCFBooleanTrue));
    assert(!chat_current_page(nullptr));
    auto chat = CGRectMake(20, 30, 60, 30);
    auto main_window = CGRectMake(10, 20, 800, 600);
    assert(chat_control_agreement(1, 1, true, chat, chat, main_window));
    assert(!chat_control_agreement(2, 1, true, chat, chat, main_window));
    assert(!chat_control_agreement(1, 2, true, chat, chat, main_window));
    assert(!chat_control_agreement(1, 1, false, chat, chat, main_window));
    assert(!chat_control_agreement(1, 1, true, chat, CGRectMake(21, 30, 60, 30), main_window));
    assert(!chat_control_agreement(1, 1, true, chat, chat, CGRectMake(50, 50, 100, 100)));
    assert(!chat_control_agreement(1, 1, true, CGRectZero, CGRectZero, main_window));
    assert(std::string(classify_ax_error(kAXErrorAttributeUnsupported)) == "attribute-unsupported");
    assert(std::string(classify_ax_error(kAXErrorCannotComplete)) == "cannot-complete");
    assert(std::string(classify_ax_error(kAXErrorAPIDisabled)) == "api-disabled");
    assert(std::string(classify_ax_error(static_cast<AXError>(-999))) == "other");
    CGRect bounds = CGRectMake(10, 20, 800, 600);
    auto owned = record(7, 1, 0, bounds);
    auto duplicate = record(7, 2, 0, bounds);
    auto panel = record(7, 3, 3, bounds);
    auto foreign = record(8, 4, 0, bounds);
    const void* unique[] = {owned, panel, foreign};
    auto inventory = CFArrayCreate(nullptr, unique, 3, &kCFTypeArrayCallBacks);
    unsigned matches = 0;
    assert(match_focus_window(inventory, 7, bounds, matches) == 1 && matches == 1);
    assert(std::string(classify_focus_agreement(true, matches)) == "proved");
    assert(std::string(classify_focus_agreement(false, matches)) == "identity-changed");
    auto changed = bounds; changed.origin.x += 0.001;
    assert(match_focus_window(inventory, 7, changed, matches) == 0 && matches == 0);
    assert(std::string(classify_focus_agreement(true, matches)) == "no-match");
    CFRelease(inventory);
    const void* ambiguous[] = {owned, duplicate};
    inventory = CFArrayCreate(nullptr, ambiguous, 2, &kCFTypeArrayCallBacks);
    assert(match_focus_window(inventory, 7, bounds, matches) == 0 && matches == 2);
    assert(std::string(classify_focus_agreement(true, matches)) == "ambiguous");
    CFRelease(inventory); CFRelease(owned); CFRelease(duplicate); CFRelease(panel); CFRelease(foreign);
}
