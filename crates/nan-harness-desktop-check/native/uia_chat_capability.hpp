#pragma once
#include <optional>
#include <array>
struct UiaChatCapability {
    std::array<long,4> editor_bounds{}, start_bounds{};
    bool editor_enabled{}, start_enabled{};
    bool value_pattern{}, password{}, keyboard_focusable{}, invoke_pattern{};
    std::optional<bool> read_only, value_empty;
    bool operator==(const UiaChatCapability& other) const {
        return editor_bounds == other.editor_bounds && start_bounds == other.start_bounds
            && editor_enabled == other.editor_enabled && start_enabled == other.start_enabled
            && value_pattern == other.value_pattern && password == other.password
            && keyboard_focusable == other.keyboard_focusable && invoke_pattern == other.invoke_pattern
            && read_only == other.read_only && value_empty == other.value_empty;
    }
};
inline bool uia_chat_capability_valid(const UiaChatCapability& value) {
    return value.value_pattern == value.read_only.has_value()
        && value.value_pattern == value.value_empty.has_value();
}
inline const char* uia_chat_capability_status(unsigned editors, unsigned starts, bool chat,
        bool available, bool unchanged) {
    if (!available) return "unavailable";
    if (!unchanged) return "changed";
    if (editors > 1 || starts > 1) return "ambiguous";
    if (!chat || editors != 1 || starts != 1) return "missing";
    return "observed";
}
