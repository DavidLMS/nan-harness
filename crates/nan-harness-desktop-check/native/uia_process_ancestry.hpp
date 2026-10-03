#pragma once
#include "process_correlation.hpp"

enum class UiaAncestry { Owned, Foreign, Unavailable };

// Foreign is evidence only when the complete stable live chain ends elsewhere.
// Missing, reused, exited or capped links cannot classify an element's owner.
template<class Query>
UiaAncestry classify_uia_ancestry(const std::vector<CorrelationEntry>& rows,
    const std::vector<CorrelationEntry>& confirm, std::uint32_t child,
    std::uint32_t root, std::uint64_t root_time, std::uint64_t child_time,
    Query query) {
    if (rows.size() > 4096 || confirm.size() > 4096 || !child || !root
        || child == root || !root_time || !child_time) return UiaAncestry::Unavailable;
    if (historical_descendant(rows, confirm, child, root, root_time, child_time, query))
        return UiaAncestry::Owned;
    std::vector<std::uint32_t> seen;
    auto current = child;
    auto time = child_time;
    for (unsigned depth = 0; depth < 16; ++depth) {
        if (std::find(seen.begin(), seen.end(), current) != seen.end())
            return UiaAncestry::Unavailable;
        seen.push_back(current);
        const auto first = std::find_if(rows.begin(), rows.end(), [current](const auto& row) { return row.pid == current; });
        const auto second = std::find_if(confirm.begin(), confirm.end(), [current](const auto& row) { return row.pid == current; });
        if (first == rows.end() || second == confirm.end() || first->parent != second->parent)
            return UiaAncestry::Unavailable;
        if (first->parent == 0) {
            std::uint64_t final_child = 0;
            return query(child, final_child) && final_child == child_time
                ? UiaAncestry::Foreign : UiaAncestry::Unavailable;
        }
        std::uint64_t parent_time = 0;
        if (!query(first->parent, parent_time) || !parent_time || parent_time > time
            || first->parent == root) return UiaAncestry::Unavailable;
        current = first->parent;
        time = parent_time;
    }
    return UiaAncestry::Unavailable;
}

// Only identities admitted through the complete ancestry proof may reach this
// final check. The caller queries retained HANDLEs, never reopens these PIDs.
struct UiaRetainedIdentity { std::uint32_t pid; std::uint64_t creation; };
template<class Query>
bool uia_collection_identity(std::uint32_t root, std::uint64_t root_time,
    const std::vector<UiaRetainedIdentity>& children, Query query) {
    if (!root || !root_time || children.size() > 64) return false;
    std::uint64_t actual = 0;
    if (!query(root, actual) || actual != root_time) return false;
    std::vector<std::uint32_t> seen;
    for (const auto& child : children) {
        if (!child.pid || child.pid == root || !child.creation
            || std::find(seen.begin(), seen.end(), child.pid) != seen.end()) return false;
        seen.push_back(child.pid);
        if (!query(child.pid, actual) || actual != child.creation) return false;
    }
    return query(root, actual) && actual == root_time;
}
