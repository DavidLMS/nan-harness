#pragma once

enum class MacOwnedWindowState { Ready, PendingFocus, Rejected };
enum class MacFocusRead { ReadyHeld, CannotComplete, Rejected };
// Independent native proofs remain mandatory even while AX only permits waiting.
struct MacWindowSafety {
    bool foreground_same, application_ready, held_unique, held_geometry_same;
    bool held_normal, display_contained, stack_clear;
};
struct MacWindowObservation {
    const char* failure = "unmeasured";
    const char* focus_status = "unqueried";
    const char* query_stage = "unqueried";
    const char* query_error = "unqueried";
};
inline const char* mac_window_rejection(MacWindowSafety safety) {
    if (!safety.foreground_same) return "foreground";
    if (!safety.application_ready) return "application";
    if (!safety.held_unique) return "identity";
    if (!safety.held_geometry_same) return "geometry";
    if (!safety.held_normal) return "layer";
    if (!safety.display_contained) return "display";
    if (!safety.stack_clear) return "stack";
    return "unmeasured";
}
inline MacOwnedWindowState classify_mac_owned_window(MacFocusRead focus, MacWindowSafety safety) {
    if (!safety.foreground_same || !safety.application_ready || !safety.held_unique
        || !safety.held_geometry_same || !safety.held_normal || !safety.display_contained || !safety.stack_clear)
        return MacOwnedWindowState::Rejected;
    if (focus == MacFocusRead::ReadyHeld) return MacOwnedWindowState::Ready;
    return focus == MacFocusRead::CannotComplete ? MacOwnedWindowState::PendingFocus : MacOwnedWindowState::Rejected;
}
