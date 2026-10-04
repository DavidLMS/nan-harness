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
    unsigned images = 0, observations = 0;
    auto check = [&](bool creation, State first, State last, bool image) {
        images = 0; observations = 0;
        return retained_target_identity(creation,
            [&] { return observations++ == 0 ? first : last; },
            [&] { ++images; return image; });
    };
    assert(check(false, State::Exited, State::Exited, true) == Identity::CreationRejected);
    assert(images == 0 && observations == 0);
    assert(check(true, State::Exited, State::Exited, false) == Identity::Terminal);
    assert(images == 0 && observations == 1);
    assert(check(true, State::Unavailable, State::Exited, true) == Identity::StateRejected);
    assert(images == 0);
    assert(check(true, State::Live, State::Exited, false) == Identity::Terminal);
    assert(images == 1 && observations == 2);
    assert(check(true, State::Live, State::Exited, true) == Identity::Terminal);
    assert(check(true, State::Live, State::Live, false) == Identity::ImageRejected);
    assert(check(true, State::Live, State::Unavailable, true) == Identity::StateRejected);
    assert(check(true, State::Live, State::Live, true) == Identity::Live);
    // Terminal admission never supplies the live image proof required to act.
    unsigned terminations = 0;
    for (auto identity : {Identity::Terminal, Identity::CreationRejected,
        Identity::StateRejected, Identity::ImageRejected}) {
        assert(!terminate_verified_handle({true,true,true,identity == Identity::Live},
            [&] { ++terminations; return true; }));
    }
    assert(terminations == 0);
}
static void retained_image_contract() {
    const RetainedImageIdentity expected{1,2,3,4,5,6,7};
    assert(retained_image_mismatch( expected, expected) == nullptr);
    struct Field { std::uint32_t RetainedImageIdentity::*member; const char* reason; };
    for (const auto field : {Field{&RetainedImageIdentity::volume,"target-image-volume"},
        Field{&RetainedImageIdentity::index_high,"target-image-file-id"},
        Field{&RetainedImageIdentity::index_low,"target-image-file-id"},
        Field{&RetainedImageIdentity::size_high,"target-image-size"},
        Field{&RetainedImageIdentity::size_low,"target-image-size"},
        Field{&RetainedImageIdentity::write_high,"target-image-write-time"},
        Field{&RetainedImageIdentity::write_low,"target-image-write-time"}}) {
        auto changed = expected; ++(changed.*field.member);
        assert(std::string(retained_image_mismatch( expected, changed)) == field.reason);
        unsigned terminations = 0;
        assert(!terminate_verified_handle({true,true,true,
            retained_image_mismatch(expected,changed)==nullptr},
            [&] { ++terminations; return true; }));
        assert(terminations == 0);
    }
}
static void original_image_alias_contract() {
    OriginalImageFile original{0x100000001ULL,{}};original.id[0]=7;original.id[15]=19;
    // Two lookup names may refer to the same retained object, never a copy.
    std::map<std::string,OriginalImageFile> files{{"pinned-original",original},{"same-object-alias",original}};
    auto copied=original;copied.id[15]=20;files["byte-identical-copy"]=copied;
    assert(same_original_image_file(files.at("pinned-original"),files.at("same-object-alias")));
    assert(!same_original_image_file(original,files.at("byte-identical-copy")));
    for(unsigned byte=0;byte<16;++byte) {
        auto changed=original;changed.id[byte]^=1;
        assert(!same_original_image_file(original,changed));
    }
    auto other_volume=original;other_volume.volume=1;
    assert(!same_original_image_file(original,other_volume));
    unsigned terminations=0;
    // Matching object identity cannot waive original process ownership/creation.
    for(auto proof : {CleanupProof{true,false,true,true},CleanupProof{true,true,false,true},
        CleanupProof{true,true,true,same_original_image_file(original,copied)}})
        assert(!terminate_verified_handle(proof,[&]{++terminations;return true;}));
    assert(terminations==0);
}
int main() {
    retained_identity_contract();
    retained_image_contract();
    original_image_alias_contract();
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
    unsigned terminations=0;
    for(const auto proof : std::vector<CleanupProof>{{false,true,true,true},{true,false,true,true},
        {true,true,false,true},{true,true,true,false}}) {
        assert(!terminate_verified_handle(proof,[&] { ++terminations;return true; }));
    }
    assert(terminations==0);
    assert(terminate_verified_handle({true,true,true,true},[&] { ++terminations;return true; }));
    assert(terminations==1);

}
