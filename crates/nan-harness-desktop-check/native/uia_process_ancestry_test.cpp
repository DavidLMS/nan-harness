#include "uia_process_ancestry.hpp"
#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <map>
int main() {
    std::vector<CorrelationEntry> rows{{1,0,false},{2,1,false},{3,2,false},{4,0,false},{5,4,false}};
    std::map<std::uint32_t,std::uint64_t> times{{1,10},{2,20},{3,30},{4,10},{5,30}};
    auto query = [&](std::uint32_t pid, std::uint64_t& value) {
        const auto entry = times.find(pid);
        if (entry == times.end()) return false;
        value = entry->second; return true;
    };
    assert(classify_uia_ancestry(rows,rows,3,1,10,30,query)==UiaAncestry::Owned);
    assert(classify_uia_ancestry(rows,rows,5,1,10,30,query)==UiaAncestry::Foreign);
    auto unstable=rows;unstable[2].parent=4;
    assert(classify_uia_ancestry(rows,unstable,3,1,10,30,query)==UiaAncestry::Unavailable);
    times[2]=40;
    assert(classify_uia_ancestry(rows,rows,3,1,10,30,query)==UiaAncestry::Unavailable);
    times[2]=20;times.erase(3);
    assert(classify_uia_ancestry(rows,rows,3,1,10,30,query)==UiaAncestry::Unavailable);
    times[3]=31;
    assert(classify_uia_ancestry(rows,rows,3,1,10,30,query)==UiaAncestry::Unavailable);
    times[3]=30;
    std::vector<CorrelationEntry> deep;
    for(std::uint32_t pid=10;pid<28;++pid){deep.push_back({pid,pid==10?0:pid-1,false});times[pid]=pid;}
    assert(classify_uia_ancestry(deep,deep,27,1,10,27,query)==UiaAncestry::Unavailable);
    auto overflow=rows;overflow.resize(4097);
    assert(classify_uia_ancestry(overflow,rows,3,1,10,30,query)==UiaAncestry::Unavailable);
    unsigned calls=0;
    auto expired=[&](std::uint32_t,std::uint64_t&){++calls;return false;};
    assert(classify_uia_ancestry(rows,rows,3,1,10,30,expired)==UiaAncestry::Unavailable);
    assert(calls>0);

    // A complete owned chain permits passive collection only while the exact
    // admitted root and child creations stay live through its final check.
    std::vector<UiaRetainedIdentity> admitted;
    unsigned counted = 0;
    auto collect = [&](std::uint32_t pid) {
        std::uint64_t creation = 0;
        if (!query(pid, creation)
            || classify_uia_ancestry(rows, rows, pid, 1, 10, creation, query) != UiaAncestry::Owned)
            return false;
        admitted.push_back({pid, creation}); ++counted; return true;
    };
    assert(!collect(5)); assert(counted == 0); // foreign ancestry never counted
    assert(collect(3)); assert(counted == 1);
    assert(uia_collection_identity(1,10,admitted,query));
    times.erase(3);
    assert(!uia_collection_identity(1,10,admitted,query)); // exit suppresses complete receipt
    times[3]=31;
    assert(!uia_collection_identity(1,10,admitted,query)); // reused PID is not held identity
    times[3]=30;times[1]=11;
    assert(!uia_collection_identity(1,10,admitted,query));
    times[1]=10;
    auto duplicate=admitted;duplicate.push_back(admitted.front());
    assert(!uia_collection_identity(1,10,duplicate,query));
    auto capped=admitted;capped.resize(65,{3,30});
    assert(!uia_collection_identity(1,10,capped,query));
    assert(!uia_collection_identity(1,10,admitted,expired));
}
