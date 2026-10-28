#pragma once
#include <string_view>

enum class MacOwnedWindowState { Ready, PendingFocus, Rejected };
enum class MacFocusRead { ReadyHeld, CannotComplete, FreshInputSnapshot, Rejected };
enum class MacFocusPolicy { Strict, CopyEntry };
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
// Only Copy entry may discard this unbound, local AXFocusedUIElement snapshot.
// Retained controls and every other phase keep the strict error policy.
inline MacFocusRead classify_mac_focus_read(const MacWindowObservation& observation,
                                           bool ready_held, MacFocusPolicy policy) {
    const auto status = std::string_view(observation.focus_status);
    if (status == "ready" && ready_held) return MacFocusRead::ReadyHeld;
    const auto error = std::string_view(observation.query_error);
    if (status == "query-error" && error == "cannot-complete") return MacFocusRead::CannotComplete;
    if (policy == MacFocusPolicy::CopyEntry && status == "query-error" && error == "invalid-element"
        && std::string_view(observation.query_stage) == "input-window") return MacFocusRead::FreshInputSnapshot;
    return MacFocusRead::Rejected;
}
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
    return focus == MacFocusRead::CannotComplete || focus == MacFocusRead::FreshInputSnapshot
        ? MacOwnedWindowState::PendingFocus : MacOwnedWindowState::Rejected;
}
