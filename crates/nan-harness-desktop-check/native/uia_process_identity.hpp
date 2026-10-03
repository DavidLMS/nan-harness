#pragma once
#include <cstdint>

// UIA defaults an unreported ProcessId property to zero. Neither zero nor a
// different positive identity proves ownership; distinguish them without
// publishing identities or accepting a partially collected tree.
inline const char* uia_process_identity_failure(int reported, std::uint32_t expected, bool root) {
    if (reported == 0) return root ? "root-process-zero" : "descendant-process-zero";
    if (reported < 0) return root ? "root-process-invalid" : "descendant-process-invalid";
    if (static_cast<std::uint32_t>(reported) != expected)
        return root ? "root-process-mismatch" : "descendant-process-mismatch";
    return nullptr;
}
