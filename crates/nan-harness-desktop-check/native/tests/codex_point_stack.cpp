#include "../codex_point_stack.hpp"
#include <cassert>
int main() {
    using R=CodexPointStackResult;
    CodexPointStackRow held{true,true,false,true,true,false,false,true,true};
    CodexPointStackRow away{true,false,false,false,false,true,false,true,false};
    assert(codex_point_stack_result(true,{away,held})==R::Clear);
    auto covering=away;covering.point_contained=true;
    assert(codex_point_stack_result(true,{covering,held})==R::PointOccluded);
    assert(codex_point_stack_result(false,{held})==R::StackUnavailable);
    assert(codex_point_stack_result(true,std::vector<CodexPointStackRow>(1025,held))==R::StackUnavailable);
    assert(codex_point_stack_result(true,{away})==R::HeldWindowMissing);
    auto changed=held;changed.held_unchanged=false;
    assert(codex_point_stack_result(true,{away,changed})==R::HeldWindowChanged);
    auto broken=away;broken.alpha_valid=false;
    assert(codex_point_stack_result(true,{broken,held})==R::MetadataInvalid);
    broken=held;broken.held_metadata_valid=false;
    assert(codex_point_stack_result(true,{broken})==R::MetadataInvalid);
    broken=away;broken.identity_valid=false;
    assert(codex_point_stack_result(true,{broken,held})==R::MetadataInvalid);
    broken=away;broken.bounds_valid=false;
    assert(codex_point_stack_result(true,{broken,held})==R::MetadataInvalid);
    auto transparent=covering;transparent.transparent=true;transparent.bounds_valid=false;
    assert(codex_point_stack_result(true,{transparent,held})==R::Clear);
    auto cursor=covering;cursor.cursor=true;cursor.alpha_valid=false;
    assert(codex_point_stack_result(true,{cursor,held})==R::Clear);
    assert(codex_point_stack_result(true,{held,covering})==R::Clear);
}
