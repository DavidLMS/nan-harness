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
    assert(codex_other_public_executable("/System/Library/CoreServices/CoreServicesUIAgent.app/Contents/MacOS/CoreServicesUIAgent")==0);
    assert(codex_other_public_executable("/System/Library/CoreServices/TextInputMenuAgent.app/Contents/MacOS/TextInputMenuAgent")==1);
    assert(codex_other_public_executable("/System/Library/Frameworks/Security.framework/Versions/A/MachServices/SecurityAgent.bundle/Contents/MacOS/SecurityAgent")==2);
    assert(codex_other_public_executable("/private/SecurityAgent")==3);
    assert(codex_other_public_executable("/System/Library/CoreServices/CoreServicesUIAgent.app/Contents/MacOS/CoreServicesUIAgent.fake")==3);
    assert(codex_occluder_kind("/private/app",true,true,true,false)==K::Other);
}
