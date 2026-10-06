#include "process_correlation.hpp"
#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <map>
#include <string>
static void retained_identity_contract() {
    using State = RetainedProcessState;
    using Identity = RetainedTargetIdentity;
    unsigned observations = 0;
    auto check = [&](bool creation, bool ancestry, State state) {
        observations = 0;
        return retained_target_identity(creation, ancestry, [&] { ++observations; return state; });
    };
    assert(check(false, true, State::Live) == Identity::CreationRejected);
    assert(observations == 0);
    assert(check(true, false, State::Live) == Identity::AncestryRejected);
    assert(observations == 0);
    assert(check(true, true, State::Exited) == Identity::Terminal);
    assert(check(true, true, State::Unavailable) == Identity::StateRejected);
    assert(check(true, true, State::Live) == Identity::Live);
    unsigned terminations = 0;
    for (auto identity : {Identity::Terminal, Identity::CreationRejected,
        Identity::AncestryRejected, Identity::StateRejected}) {
        assert(!terminate_verified_handle({true,true,true,identity == Identity::Live},
            [&] { ++terminations; return true; }));
    }
    assert(terminations == 0);
}
static void owned_helper_contract() {
    // Executable labels are not cleanup authority. Only the original retained
    // handle with birth and pre-stop ancestry proof reaches the action callback.
    struct Held { unsigned handle; std::uint64_t birth; const char* image; };
    const Held original{73,30,"installed-app"}, helper{74,31,"different-helper"};
    unsigned acted = 0;
    for (const auto& target : {original, helper}) {
        const auto identity = retained_target_identity(target.birth > 0, true,
            [] { return RetainedProcessState::Live; });
        assert(identity == RetainedTargetIdentity::Live);
        assert(terminate_verified_handle({true,true,true,true},[&] { acted = target.handle; return true; }));
        assert(acted == target.handle);
    }
    assert(retained_target_identity(false,true,[]{return RetainedProcessState::Live;}) == RetainedTargetIdentity::CreationRejected);
    assert(retained_target_identity(true,false,[]{return RetainedProcessState::Live;}) == RetainedTargetIdentity::AncestryRejected);
}
int main() {
    retained_identity_contract();
    owned_helper_contract();
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
    const std::vector<RetainedProcessBinding> held{{3,30}};
    assert(matches_retained_inventory(rows,held,query));
    auto new_process=rows;new_process.push_back({4,2,true});times[4]=31;
    assert(!matches_retained_inventory(new_process,held,query));
    times[3]=31;assert(!matches_retained_inventory(rows,held,query));
    times.erase(3);assert(!matches_retained_inventory(rows,held,query));
    times[3]=30;
    auto foreign=rows;foreign[2].parent=4;
    assert(!historical_descendant(foreign,foreign,3,1,10,30,query));
    auto missing=rows; missing.erase(missing.begin()+1);
    assert(!historical_descendant(missing,missing,3,1,10,30,query));
    std::vector<CorrelationEntry> deep;
    for(std::uint32_t pid=1;pid<=18;++pid) { deep.push_back({pid,pid-1,true}); times[pid]=pid; }
    assert(!historical_descendant(deep,deep,18,1,1,18,query));
    times[1]=10; times[2]=20; times[3]=30;
    auto cycle=rows; cycle[1].parent=3;
    assert(!historical_descendant(cycle,cycle,3,1,10,30,query));
    unsigned terminations=0;
    for(const auto proof : std::vector<CleanupProof>{{false,true,true,true},{true,false,true,true},
        {true,true,false,true},{true,true,true,false}}) {
        assert(!terminate_verified_handle(proof,[&] { ++terminations;return true; }));
    }
    assert(terminations==0);
    assert(terminate_verified_handle({true,true,true,true},[&] { ++terminations;return true; }));
    assert(terminations==1);

}
