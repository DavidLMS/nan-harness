#include "../uia_chat_capability.hpp"
#include <cassert>
#include <string>
int main() {
    assert(std::string(uia_chat_capability_status(1,1,true,true,true)) == "observed");
    assert(std::string(uia_chat_capability_status(2,1,true,true,true)) == "ambiguous");
    assert(std::string(uia_chat_capability_status(1,1,false,true,true)) == "missing");
    assert(std::string(uia_chat_capability_status(1,1,true,false,true)) == "unavailable");
    assert(std::string(uia_chat_capability_status(1,1,true,true,false)) == "changed");
    UiaChatCapability readable;
    readable.value_pattern=true; readable.keyboard_focusable=true; readable.invoke_pattern=true;
    readable.read_only=false; readable.value_empty=true;
    assert(uia_chat_capability_valid(readable));
    auto changed = readable; changed.value_empty = false;
    assert(!(readable == changed));
    changed=readable; changed.editor_bounds[0]=1;
    assert(!(readable == changed));
    changed=readable; changed.start_enabled=true;
    assert(!(readable == changed));
    readable.value_pattern = false;
    assert(!uia_chat_capability_valid(readable));
    readable.read_only.reset(); readable.value_empty.reset();
    assert(uia_chat_capability_valid(readable));
}
