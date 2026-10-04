// Pure public-path and private identity evidence; no native queries.
#include "../codex_occluder_kind.hpp"
#include <cassert>
int main() {
    using K=CodexOccluderKind;
    const char* control="/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter";
    assert(codex_occluder_kind(control,true,false,false,false)==K::ControlCenter);
    assert(codex_occluder_kind(control,false,true,true,true)==K::Unobserved);
    assert(codex_occluder_kind("/private/ControlCenter",true,true,false,false)==K::Other);
    assert(codex_occluder_kind("/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter.fake",true,true,false,false)==K::Other);
    assert(codex_occluder_kind("/private/app",true,true,true,true)==K::LauncherOwned);
    assert(codex_occluder_kind("/private/app",true,true,false,true)==K::CheckerOwned);
    assert(codex_occluder_kind("/private/app",true,false,true,true)==K::Unobserved);
    assert(codex_occluder_kind("/private/app",false,true,false,false)==K::Unobserved);
    assert(codex_occluder_kind("/private/app",true,true,true,false)==K::Other);
}
