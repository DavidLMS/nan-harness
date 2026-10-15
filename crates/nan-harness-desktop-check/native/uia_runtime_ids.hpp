#pragma once
#include <cstdint>
#include <set>
#include <utility>
#include <vector>

enum class UiaRuntimeIdResult { Unique, Duplicate, Invalid, Limit };

// IDs are opaque and desktop-local. Keep this index only for one bounded
// inventory, while its native elements remain retained; never reuse it across
// snapshots or expose IDs in diagnostic output.
class UiaRuntimeIds {
    std::set<std::vector<std::int32_t>> ids;
public:
    UiaRuntimeIdResult insert(std::vector<std::int32_t> id) {
        if (id.empty() || id.size()>64) return UiaRuntimeIdResult::Invalid;
        if (ids.find(id)!=ids.end()) return UiaRuntimeIdResult::Duplicate;
        if (ids.size()>=1024) return UiaRuntimeIdResult::Limit;
        ids.insert(std::move(id));
        return UiaRuntimeIdResult::Unique;
    }
};
