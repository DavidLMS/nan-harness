#include "../uia_current_mode.hpp"
#include <cassert>
int main() {
    assert(uia_current_page(L"current=page;label=Chat") == true);
    assert(uia_current_page(L"label=a\\;b;current=page") == true);
    assert(uia_current_page(L"") == false);
    assert(uia_current_page(L"current=false") == false);
    for (const auto& value : {L"current=page;current=page", L"current=page;current=false", L"current=page;", L"current", L"current=pa\\ge", L"label=x\\"}) assert(!uia_current_page(value));
    assert(!uia_current_page(std::wstring(2049, L'x')));
    const UiaModeCounts chat{1, 1, 1, 1, 0};
    assert(std::string(uia_mode_status(chat, true, true)) == "chat");
    assert(std::string(uia_mode_status({1, 1, 1, 0, 1}, true, true)) == "cowork");
    assert(std::string(uia_mode_status({2, 1, 1, 1, 0}, true, true)) == "ambiguous");
    assert(std::string(uia_mode_status({1, 1, 1, 1, 1}, true, true)) == "ambiguous");
    assert(std::string(uia_mode_status({}, true, true)) == "missing");
    assert(std::string(uia_mode_status(chat, false, true)) == "unavailable");
    assert(std::string(uia_mode_status(chat, true, false)) == "changed");
}
