#pragma once
#include <algorithm>
#include <array>
#include <cstdint>
#include <vector>

struct CorrelationEntry { std::uint32_t pid; std::uint32_t parent; bool matching; };

// Snapshot parent references alone cannot certify ancestry: every live link
// must have a stable parent and a creation time not newer than its child.
template<class Query>
bool historical_descendant(const std::vector<CorrelationEntry>& rows,
    const std::vector<CorrelationEntry>& confirm, std::uint32_t child,
    std::uint32_t launcher, std::uint64_t launcher_time,
    std::uint64_t child_time, Query query) {
    std::uint32_t current = child;
    const auto initial_time = child_time;
    std::vector<std::uint32_t> seen;
    for (unsigned depth = 0; depth < 16; ++depth) {
        if (std::find(seen.begin(), seen.end(), current) != seen.end()) return false;
        seen.push_back(current);
        const auto found = std::find_if(rows.begin(), rows.end(), [current](const auto& e) { return e.pid == current; });
        const auto stable = std::find_if(confirm.begin(), confirm.end(), [current](const auto& e) { return e.pid == current; });
        if (found == rows.end() || stable == confirm.end() || found->parent == 0
            || stable->parent != found->parent) return false;
        std::uint64_t parent_time = 0;
        if (!query(found->parent, parent_time) || parent_time > child_time) return false;
        if (found->parent == launcher) {
            std::uint64_t final_child = 0;
            return parent_time == launcher_time && query(child, final_child) && final_child == initial_time;
        }
        current = found->parent;
        child_time = parent_time;
    }
    return false;
}

struct CleanupProof {
    bool before_deadline;
    bool owner_alive;
    bool creation_matches;
    bool image_matches;
};
// Only a retained handle can be passed to the callback. No PID lookup belongs
// in this decision, and an uncertain proof never invokes termination.
template<class Terminate>
bool terminate_verified_handle(const CleanupProof& proof, Terminate terminate) {
    return proof.before_deadline && proof.owner_alive && proof.creation_matches
        && proof.image_matches && terminate();
}

// A signaled original process handle is terminal and cannot authorize an action.
// Creation equality remains mandatory even when its image is no longer queryable.
enum class RetainedProcessState { Live, Exited, Unavailable };
enum class RetainedTargetIdentity { Live, Terminal, CreationRejected, StateRejected, ImageRejected };
template<class State, class Image>
RetainedTargetIdentity retained_target_identity(bool creation_matches, State state, Image image) {
    if (!creation_matches) return RetainedTargetIdentity::CreationRejected;
    const auto initial = state();
    if (initial == RetainedProcessState::Exited) return RetainedTargetIdentity::Terminal;
    if (initial != RetainedProcessState::Live) return RetainedTargetIdentity::StateRejected;
    const bool image_matches = image();
    // The image query may race ordinary renderer shutdown. Recheck only this
    // retained handle; never look up another PID or extend the original cutoff.
    const auto final = state();
    if (final == RetainedProcessState::Exited) return RetainedTargetIdentity::Terminal;
    if (final != RetainedProcessState::Live) return RetainedTargetIdentity::StateRejected;
    return image_matches ? RetainedTargetIdentity::Live : RetainedTargetIdentity::ImageRejected;
}

struct RetainedImageIdentity {
    std::uint32_t volume, index_high, index_low, size_high, size_low, write_high, write_low;
};
inline const char* retained_image_mismatch(
    const RetainedImageIdentity& expected, const RetainedImageIdentity& actual) {
    if (actual.volume != expected.volume) return "target-image-volume";
    if (actual.index_high != expected.index_high || actual.index_low != expected.index_low)
        return "target-image-file-id";
    if (actual.size_high != expected.size_high || actual.size_low != expected.size_low)
        return "target-image-size";
    if (actual.write_high != expected.write_high || actual.write_low != expected.write_low)
        return "target-image-write-time";
    return nullptr;
}

// The original hashed executable remains open, so its identity cannot be reused.
// Win32 aliases are lookup names; these kernel identities bind the opened object.
struct OriginalImageFile {
    std::uint64_t volume;
    std::array<unsigned char,16> id;
};
inline bool same_original_image_file(const OriginalImageFile& original, const OriginalImageFile& candidate) {
    return original.volume == candidate.volume && original.id == candidate.id;
}
