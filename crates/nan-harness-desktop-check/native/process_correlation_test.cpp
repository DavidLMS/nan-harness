#include "process_correlation.hpp"
#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <map>
int main() {
    std::vector<CorrelationEntry> rows{{1,0,false},{2,1,false},{3,2,true}};
    std::map<std::uint32_t,std::uint64_t> times{{1,10},{2,20},{3,30}};
    auto query = [&](std::uint32_t pid, std::uint64_t& time) {
        auto found=times.find(pid); if(found==times.end()) return false;
        time=found->second; return true;
    };
    assert(historical_descendant(rows,rows,3,1,10,30,query));
    auto changed=rows; changed[2].parent=1;
    assert(!historical_descendant(rows,changed,3,1,10,30,query));
    times[2]=40; assert(!historical_descendant(rows,rows,3,1,10,30,query));
    times[2]=20; times.erase(1); assert(!historical_descendant(rows,rows,3,1,10,30,query));
    times[1]=11; assert(!historical_descendant(rows,rows,3,1,10,30,query));
    times[1]=10; times[3]=31; assert(!historical_descendant(rows,rows,3,1,10,30,query));
    times[3]=30;
    auto missing=rows; missing.erase(missing.begin()+1);
    assert(!historical_descendant(missing,missing,3,1,10,30,query));
    std::vector<CorrelationEntry> deep;
    for(std::uint32_t pid=1;pid<=18;++pid) { deep.push_back({pid,pid-1,true}); times[pid]=pid; }
    assert(!historical_descendant(deep,deep,18,1,1,18,query));
    times[1]=10; times[2]=20; times[3]=30;
    auto cycle=rows; cycle[1].parent=3;
    assert(!historical_descendant(cycle,cycle,3,1,10,30,query));
}
