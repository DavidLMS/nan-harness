#pragma once
#include <array>
#include <string_view>
enum class CodexOccluderKind : unsigned {
    ControlCenter, NotificationCenter, SystemUIServer, Dock, WindowServer,
    LauncherOwned, CheckerOwned, Other, Unobserved
};
inline CodexOccluderKind codex_occluder_kind(std::string_view path,bool stable,
    bool chain_complete,bool launcher_owned,bool checker_owned) {
    if(!stable)return CodexOccluderKind::Unobserved;
    constexpr std::array<std::string_view,5> paths={
        "/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter",
        "/System/Library/CoreServices/NotificationCenter.app/Contents/MacOS/NotificationCenter",
        "/System/Library/CoreServices/SystemUIServer.app/Contents/MacOS/SystemUIServer",
        "/System/Library/CoreServices/Dock.app/Contents/MacOS/Dock",
        "/System/Library/PrivateFrameworks/SkyLight.framework/Versions/A/Resources/WindowServer"};
    for(unsigned i=0;i<paths.size();++i)if(path==paths[i])return CodexOccluderKind(i);
    if(!chain_complete)return CodexOccluderKind::Unobserved;
    if(launcher_owned&&checker_owned)return CodexOccluderKind::LauncherOwned;
    if(checker_owned)return CodexOccluderKind::CheckerOwned;
    return CodexOccluderKind::Other;
}
// Optional subpartition only: these paths remain in legacy Other.
inline unsigned codex_other_public_executable(std::string_view path) {
    constexpr std::array<std::string_view,3> paths={
        "/System/Library/CoreServices/CoreServicesUIAgent.app/Contents/MacOS/CoreServicesUIAgent",
        "/System/Library/CoreServices/TextInputMenuAgent.app/Contents/MacOS/TextInputMenuAgent",
        "/System/Library/Frameworks/Security.framework/Versions/A/MachServices/SecurityAgent.bundle/Contents/MacOS/SecurityAgent"};
    for(unsigned i=0;i<paths.size();++i)if(path==paths[i])return i;
    return unsigned(paths.size());
}
