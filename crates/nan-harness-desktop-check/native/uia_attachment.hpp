#pragma once
#include "uia_runtime_ids.hpp"
#include <optional>
#include <vector>

// Follow fresh parent links to the exact retained root. Keeping every element
// alive allows identity-based cycle rejection without exporting any identity.
template<class Node, class Parent, class Equal, class Owned, class Within, class Identify>
bool uia_attached_to_root(Node current, const Node& root, Parent parent,
                          Equal equal, Owned owned, Within within, Identify identify) {
    std::vector<Node> visited;
    UiaRuntimeIds identities;
    for (unsigned depth = 0; depth <= 32; ++depth) {
        if (!within() || !owned(current) || !within()) return false;
        // CompareElements defines identity through runtime IDs. Keep a fresh
        // path-local index for cycles; still prove the exact retained root and
        // keep every queried ancestor alive until this walk finishes.
        auto identity = identify(current);
        if (!identity || !within()
            || identities.insert(std::move(*identity)) != UiaRuntimeIdResult::Unique) return false;
        const auto is_root = equal(current, root);
        if (!is_root || !within()) return false;
        if (*is_root) return true;
        if (depth == 32) return false;
        visited.push_back(current);
        const auto next = parent(current);
        if (!next || !within()) return false;
        current = *next;
    }
    return false;
}
