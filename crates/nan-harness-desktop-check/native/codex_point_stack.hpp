#pragma once
#include <vector>
// Facts from one retained CGWindowList snapshot, never public owner identities.
struct CodexPointStackRow {
    bool identity_valid=false,held=false,cursor=false,held_metadata_valid=false,
        held_unchanged=false,alpha_valid=false,transparent=false,bounds_valid=false,
        point_contained=false;
};
enum class CodexPointStackResult {
    Clear, PointOccluded, StackUnavailable, MetadataInvalid,
    HeldWindowMissing, HeldWindowChanged
};
inline CodexPointStackResult codex_point_stack_result(bool available,
    const std::vector<CodexPointStackRow>& rows) {
    if(!available||rows.size()>1024)return CodexPointStackResult::StackUnavailable;
    for(const auto& row:rows) {
        if(!row.identity_valid)return CodexPointStackResult::MetadataInvalid;
        if(row.held) {
            if(!row.held_metadata_valid)return CodexPointStackResult::MetadataInvalid;
            return row.held_unchanged?CodexPointStackResult::Clear:
                CodexPointStackResult::HeldWindowChanged;
        }
        if(row.cursor)continue;
        if(!row.alpha_valid)return CodexPointStackResult::MetadataInvalid;
        if(row.transparent)continue;
        if(!row.bounds_valid)return CodexPointStackResult::MetadataInvalid;
        if(row.point_contained)return CodexPointStackResult::PointOccluded;
    }
    return CodexPointStackResult::HeldWindowMissing;
}
inline const char* codex_point_stack_reason(CodexPointStackResult result) {
    switch(result) {
        case CodexPointStackResult::Clear:return "measured";
        case CodexPointStackResult::PointOccluded:return "point-occluded";
        case CodexPointStackResult::StackUnavailable:return "stack-unavailable";
        case CodexPointStackResult::MetadataInvalid:return "metadata-invalid";
        case CodexPointStackResult::HeldWindowMissing:return "held-window-missing";
        case CodexPointStackResult::HeldWindowChanged:return "held-window-changed";
    }
    return "metadata-invalid";
}
