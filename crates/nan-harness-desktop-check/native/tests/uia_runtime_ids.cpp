#include "../uia_runtime_ids.hpp"
#include <cassert>
#include <limits>

int main() {
    UiaRuntimeIds ids;
    assert(ids.insert({})==UiaRuntimeIdResult::Invalid);
    assert(ids.insert(std::vector<std::int32_t>(65,1))==UiaRuntimeIdResult::Invalid);
    // Compare all opaque components, including signed values and prefixes.
    assert(ids.insert({-1,0,1})==UiaRuntimeIdResult::Unique);
    assert(ids.insert({-1,0,1})==UiaRuntimeIdResult::Duplicate);
    assert(ids.insert({-1,0})==UiaRuntimeIdResult::Unique);
    assert(ids.insert({-1,0,2})==UiaRuntimeIdResult::Unique);
    assert(ids.insert({std::numeric_limits<std::int32_t>::min()})==UiaRuntimeIdResult::Unique);
    for (std::int32_t index=0;index<1020;++index)
        assert(ids.insert({42,index})==UiaRuntimeIdResult::Unique);
    assert(ids.insert({42,1019})==UiaRuntimeIdResult::Duplicate);
    assert(ids.insert({42,1020})==UiaRuntimeIdResult::Limit);
    UiaRuntimeIds next_snapshot;
    assert(next_snapshot.insert({42,0})==UiaRuntimeIdResult::Unique);
}
