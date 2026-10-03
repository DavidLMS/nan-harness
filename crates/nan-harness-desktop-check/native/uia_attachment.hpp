#pragma once
#include <optional>
#include <vector>

// Follow fresh parent links to the exact retained root. Keeping every element
// alive allows identity-based cycle rejection without exporting any identity.
template<class Node, class Parent, class Equal, class Owned, class Within>
bool uia_attached_to_root(Node current, const Node& root, Parent parent,
                          Equal equal, Owned owned, Within within) {
    std::vector<Node> visited;
    for (unsigned depth = 0; depth <= 32; ++depth) {
        if (!within() || !owned(current) || !within()) return false;
        for (const auto& prior : visited) {
            if (!within()) return false;
            const auto same = equal(current, prior);
            if (!same || *same || !within()) return false;
        }
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
