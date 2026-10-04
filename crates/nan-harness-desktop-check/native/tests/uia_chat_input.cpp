#include "../uia_chat_input.hpp"
#include <cassert>
#include <sstream>
static void clipboard_acquisition_contract() {
    unsigned opens=0,pauses=0,mutations=0,closes=0;bool within=true,guard=true;
    auto run=[&](unsigned failures,bool late,bool lose_guard) {
        opens=pauses=mutations=closes=0;within=guard=true;
        const bool acquired=uia_chat_acquire_clipboard([&] {
            ++opens;if(opens<=failures)return false;
            if(late)within=false;return true;
        },[&]{return within;},[&]{return guard;},[&]{
            ++pauses;if(lose_guard)guard=false;if(pauses>=3)within=false;
        });
        if(acquired) {if(within&&guard)++mutations;++closes;}
        return acquired;
    };
    assert(run(0,false,false));assert(opens==1&&pauses==0&&mutations==1&&closes==1);
    assert(run(2,false,false));assert(opens==3&&pauses==2&&mutations==1&&closes==1);
    assert(!run(10,false,false));assert(opens==3&&mutations==0&&closes==0);
    assert(!run(2,false,true));assert(opens==1&&mutations==0&&closes==0);
    assert(run(0,true,false));assert(opens==1&&mutations==0&&closes==1);
    opens=0;
    assert(!uia_chat_acquire_clipboard([&]{++opens;return true;},[]{return false;},[]{return true;},[]{}));
    assert(opens==0);
    assert(!uia_chat_acquire_clipboard([&]{++opens;return true;},[]{return true;},[]{return false;},[]{}));
    assert(opens==0);
}
int main() {
    clipboard_acquisition_contract();
    assert(uia_chat_focus_state(true,true,true,true,true)==UiaChatInputValue::Ready);
    assert(uia_chat_focus_state(true,true,false,true,true)==UiaChatInputValue::Pending);
    for(unsigned index=0;index<4;++index) {
        const bool retained=index!=0,query=index!=1,attached=index!=2,guard=index!=3;
        assert(uia_chat_focus_state(retained,query,true,attached,guard)==UiaChatInputValue::Rejected);
    }
    assert(uia_chat_input_value(L"",L"prompt",L"old draft",true,true)==UiaChatInputValue::Pending);
    assert(uia_chat_input_value(L"pro",L"prompt",L"old draft",true,true)==UiaChatInputValue::Pending);
    assert(uia_chat_input_value(L"prompt",L"prompt",L"old draft",true,true)==UiaChatInputValue::Ready);
    assert(uia_chat_input_value(L"other",L"prompt",L"old draft",true,true)==UiaChatInputValue::Rejected);
    assert(uia_chat_input_value(L"prompt",L"prompt",L"old draft",true,false)==UiaChatInputValue::Rejected);
    assert(uia_chat_input_value(L"old draft",L"prompt",L"old draft",true,true)==UiaChatInputValue::Pending);
    assert(uia_chat_input_value(L"foreign",L"prompt",L"old draft",true,true)==UiaChatInputValue::Rejected);
    unsigned queries=0,keys=0;
    if(uia_chat_settle([&] {assert(keys==0);return ++queries==1?UiaChatInputValue::Pending:UiaChatInputValue::Ready;},
                      []{return true;},[]{}))++keys;
    assert(queries==2&&keys==1);
    queries=keys=0;
    assert(!uia_chat_settle([&]{++queries;return UiaChatInputValue::Ready;},[&]{return queries==0;},[]{}));
    assert(keys==0&&queries==1);
    for(const std::string frame:{"one\nextra","one\n\n","one"}) {
        std::istringstream input(frame);assert(!uia_chat_frame(input));
    }
    std::istringstream framed("one\n");assert(uia_chat_frame(framed)=="one");
    assert(uia_chat_hex("70726f6d7074")=="prompt");
    for(const std::string hex:{"00","A0","x0","1",""})assert(!uia_chat_hex(hex));
}
