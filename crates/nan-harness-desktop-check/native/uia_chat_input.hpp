#pragma once
#include <optional>
#include <string>
#include <cstdint>
#include <istream>

enum class UiaChatInputValue { Ready, Pending, Rejected };
inline UiaChatInputValue uia_chat_focus_state(bool retained_valid,bool query_valid,
        bool same_focus,bool focused_attached,bool guard_valid) {
    if(!retained_valid || !query_valid || !focused_attached || !guard_valid)
        return UiaChatInputValue::Rejected;
    return same_focus?UiaChatInputValue::Ready:UiaChatInputValue::Pending;
}
inline UiaChatInputValue uia_chat_input_value(const std::wstring& observed,
        const std::wstring& expected, const std::wstring& prior, bool valid, bool exact_focus) {
    if (!valid || !exact_focus || observed.size()>1024) return UiaChatInputValue::Rejected;
    if (observed==expected) return UiaChatInputValue::Ready;
    return observed==prior || expected.compare(0,observed.size(),observed)==0
        ? UiaChatInputValue::Pending : UiaChatInputValue::Rejected;
}
template<class Query, class Within, class Pause>
inline bool uia_chat_settle(Query query, Within within, Pause pause) {
    while (within()) {
        const auto result=query();
        if (!within() || result==UiaChatInputValue::Rejected) return false;
        if (result==UiaChatInputValue::Ready) return true;
        pause();
    }
    return false;
}
// Only clipboard lock acquisition may settle. The caller owns a successful
// lock (even if its call crossed the cutoff) and must close it without mutation.
template<class Open, class Within, class Guard, class Pause>
inline bool uia_chat_acquire_clipboard(Open open, Within within, Guard guard, Pause pause) {
    while (within() && guard()) {
        if (open()) return true;
        if (!within()) return false;
        pause();
    }
    return false;
}
inline std::optional<std::string> uia_chat_frame(std::istream& input) {
    char wire[16385]{};
    if (!input.getline(wire,sizeof(wire)) || input.eof()
        || input.peek()!=std::char_traits<char>::eof()) return std::nullopt;
    return std::string(wire);
}
inline std::optional<std::string> uia_chat_hex(const std::string& input) {
    if(input.empty() || input.size()>2048 || input.size()%2) return std::nullopt;
    std::string result;
    const auto digit=[](char value)->int {
        if(value>='0'&&value<='9')return value-'0';
        if(value>='a'&&value<='f')return value-'a'+10;
        return -1;
    };
    for(std::size_t index=0;index<input.size();index+=2) {
        const int high=digit(input[index]),low=digit(input[index+1]);
        if(high<0||low<0||(high==0&&low==0))return std::nullopt;
        result.push_back(static_cast<char>(high*16+low));
    }
    return result;
}
