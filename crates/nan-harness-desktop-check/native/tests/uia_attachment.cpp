#include "../uia_attachment.hpp"
#include <cassert>
#include <map>
#include <set>
#include <optional>

struct Fixture {
    std::map<int, int> parents;
    std::set<int> foreign;
    unsigned parent_queries{}, ownership_queries{}, identity_queries{}, comparisons{}, remaining = 10000;
    bool parent_error{}, compare_error{}, identity_error{}, expire_after_parent{}, expire_after_identity{};
    std::map<int, std::vector<std::int32_t>> identities;
    bool attached(int child, int root) {
        return uia_attached_to_root(child, root,
            [&](int value) -> std::optional<int> {
                ++parent_queries;
                if (expire_after_parent) remaining = 0;
                const auto found = parents.find(value);
                if (parent_error || found == parents.end()) return std::nullopt;
                return found->second;
            },
            [&](int left, int right) -> std::optional<bool> {
                ++comparisons;
                if (compare_error) return std::nullopt;
                return left == right;
            },
            [&](int value) { ++ownership_queries; return !foreign.count(value); },
            [&] { return remaining && --remaining; },
            [&](int value) -> std::optional<std::vector<std::int32_t>> {
                ++identity_queries;
                if (expire_after_identity) remaining = 0;
                if (identity_error) return std::nullopt;
                const auto found = identities.find(value);
                return found == identities.end() ? std::vector<std::int32_t>{value} : found->second;
            });
    }
};
int main() {
    Fixture valid; valid.parents = {{3, 2}, {2, 1}};
    assert(valid.attached(3, 1));
    assert(valid.parent_queries == 2);
    Fixture detached; detached.parents = {{3, 4}};
    assert(!detached.attached(3, 1));
    Fixture foreign; foreign.parents = {{3, 2}, {2, 1}}; foreign.foreign.insert(2);
    assert(!foreign.attached(3, 1)); assert(foreign.parent_queries == 1);
    Fixture cycle; cycle.parents = {{3, 2}, {2, 3}};
    assert(!cycle.attached(3, 1)); assert(cycle.parent_queries == 2);
    Fixture error; error.parents = {{3, 1}}; error.parent_error = true;
    assert(!error.attached(3, 1));
    Fixture comparison; comparison.compare_error = true;
    assert(!comparison.attached(3, 1)); assert(comparison.parent_queries == 0);
    Fixture unavailable; unavailable.identity_error = true;
    assert(!unavailable.attached(3, 1)); assert(unavailable.comparisons == 0);
    for (const auto& invalid : {std::vector<std::int32_t>{}, std::vector<std::int32_t>(65, 1)}) {
        Fixture malformed; malformed.identities[3] = invalid;
        assert(!malformed.attached(3, 1)); assert(malformed.comparisons == 0);
    }
    Fixture alias; alias.parents = {{3, 2}, {2, 1}};
    alias.identities[3] = {42}; alias.identities[2] = {42};
    assert(!alias.attached(3, 1)); assert(alias.parent_queries == 1);
    Fixture late_identity; late_identity.expire_after_identity = true;
    assert(!late_identity.attached(3, 1)); assert(late_identity.comparisons == 0);
    Fixture expired; expired.remaining = 0;
    assert(!expired.attached(3, 1));
    assert(expired.parent_queries == 0 && expired.ownership_queries == 0);
    Fixture late; late.parents = {{3, 1}}; late.expire_after_parent = true;
    assert(!late.attached(3, 1));
    assert(late.parent_queries == 1 && late.ownership_queries == 1);
    Fixture limit;
    for (int i = 0; i < 33; ++i) limit.parents[i] = i + 1;
    assert(!limit.attached(0, 33)); assert(limit.parent_queries == 32);
    Fixture boundary;
    for (int i = 0; i < 32; ++i) boundary.parents[i] = i + 1;
    assert(boundary.attached(0, 32));
    assert(boundary.comparisons == 33 && boundary.identity_queries == 33);
    // Each independent observation gets a new ID index and fresh parent links.
    assert(boundary.attached(0, 32)); assert(boundary.comparisons == 66);
    // A retained old group ceases to prove attachment when its fresh parent
    // chain stops reaching the original root, even if its properties survive.
    valid.parents.erase(2);
    assert(!valid.attached(3, 1));
}
