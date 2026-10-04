#pragma once

enum class MacOwnedWindowState { Ready, PendingFocus, Rejected };
enum class MacFocusRead { ReadyHeld, CannotComplete, Rejected };
// Independent native proofs remain mandatory even while AX only permits waiting.
struct MacWindowSafety {
    bool foreground_same, application_ready, held_unique, held_geometry_same;
    bool held_normal, display_contained, stack_clear;
};
inline MacOwnedWindowState classify_mac_owned_window(MacFocusRead focus, MacWindowSafety safety) {
    if (!safety.foreground_same || !safety.application_ready || !safety.held_unique
        || !safety.held_geometry_same || !safety.held_normal || !safety.display_contained || !safety.stack_clear)
        return MacOwnedWindowState::Rejected;
    if (focus == MacFocusRead::ReadyHeld) return MacOwnedWindowState::Ready;
    return focus == MacFocusRead::CannotComplete ? MacOwnedWindowState::PendingFocus : MacOwnedWindowState::Rejected;
}
