#include "../codex_activation_identity.hpp"
#include <cassert>
#include <limits>
int main() {
    const CodexMainIdentity held{42,100,500,0,10,20,600,400};
    assert(same_codex_main(held,held));
    assert(codex_inventory_admitted(true,1,true,false,false,true));
    assert(!codex_inventory_admitted(false,1,true,false,false,true));
    assert(!codex_inventory_admitted(true,0,false,false,false,true));
    assert(!codex_inventory_admitted(true,2,true,false,false,true));
    assert(!codex_inventory_admitted(true,1,false,false,false,true));
    assert(!codex_inventory_admitted(true,1,true,true,false,true));
    assert(!codex_inventory_admitted(true,1,true,false,true,true));
    assert(!codex_inventory_admitted(true,1,true,false,false,false));
    auto changed=held;changed.id++;assert(!same_codex_main(held,changed));
    changed=held;changed.pid++;assert(!same_codex_main(held,changed));
    changed=held;changed.seconds++;assert(!same_codex_main(held,changed));
    changed=held;changed.micros++;assert(!same_codex_main(held,changed));
    changed=held;changed.width++;assert(!same_codex_main(held,changed));
    changed=held;changed.x++;assert(!same_codex_main(held,changed));
    changed=held;changed.height=0;assert(!same_codex_main(changed,changed));
    changed=held;changed.x=std::numeric_limits<double>::quiet_NaN();assert(!same_codex_main(changed,changed));
    changed=held;changed.width=std::numeric_limits<double>::infinity();assert(!same_codex_main(changed,changed));
    changed=held;changed.micros=1000000;assert(!same_codex_main(changed,changed));
}
