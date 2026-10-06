#pragma once
#include <cstdint>
#include <cmath>
struct CodexMainIdentity {
    std::uint64_t id, pid, seconds, micros;
    double x, y, width, height;
};
inline bool valid_codex_main(const CodexMainIdentity& value) {
    return value.id>0&&value.pid>1&&value.seconds>0&&value.micros<1000000
        &&std::isfinite(value.x)&&std::isfinite(value.y)
        &&std::isfinite(value.width)&&std::isfinite(value.height)
        &&value.width>=300&&value.height>=200;
}
inline bool same_codex_main(const CodexMainIdentity& held,const CodexMainIdentity& fresh) {
    return valid_codex_main(held)&&valid_codex_main(fresh)
        &&held.id==fresh.id&&held.pid==fresh.pid&&held.seconds==fresh.seconds&&held.micros==fresh.micros
        &&held.x==fresh.x&&held.y==fresh.y&&held.width==fresh.width&&held.height==fresh.height;
}

inline bool codex_inventory_admitted(bool complete,unsigned candidates,bool held_present,
    bool other_owned_normal,bool overlapping_ahead,bool fully_displayed,
    bool activation_only=false,bool owned_overlap=false) {
    return complete&&candidates==1&&held_present&&!other_owned_normal
        &&!owned_overlap&&(activation_only||!overlapping_ahead)&&fully_displayed;
}

// Preserve the first failed inventory observation unless the caller cutoff expired.
inline const char* codex_inventory_failure_reason(const char* incomplete_reason,
    bool complete,bool other_owned_normal,bool overlapping_ahead,bool fully_displayed,bool timely) {
    if(!timely)return "deadline";
    if(!complete)return incomplete_reason;
    if(other_owned_normal)return "other-owned-normal";
    if(overlapping_ahead)return "overlapping-ahead";
    return fully_displayed?"identity":"off-display";
}

// AXWindow is required for descendants of a window, not necessarily the window
// itself. Exact held-window equality still requires the expected owner process.
inline bool codex_hit_is_held_window(bool owner_matches,bool hit_is_main,
    bool enclosing_window_matches) {
    return owner_matches&&(hit_is_main||enclosing_window_matches);
}
