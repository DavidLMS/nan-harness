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
    bool other_owned_normal,bool overlapping_ahead,bool fully_displayed) {
    return complete&&candidates==1&&held_present&&!other_owned_normal
        &&!overlapping_ahead&&fully_displayed;
}
