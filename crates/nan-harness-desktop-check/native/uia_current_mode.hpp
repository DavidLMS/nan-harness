#pragma once
#include <optional>
#include <string>
#include <vector>

// Chromium UIA AriaProperties uses semicolon-delimited escaped key/value pairs.
// Only the exact public aria-current token "page" identifies this mode.
inline std::optional<bool> uia_current_page(const std::wstring& properties) {
    if (properties.size() > 2048) return std::nullopt;
    bool current = false, seen = false, escaped = false;
    std::wstring token;
    const auto consume = [&]() {
        const auto equal = token.find(L'=');
        if (token.empty() || equal == std::wstring::npos || equal == 0) return false;
        if (token.substr(0, equal) == L"current") {
            if (seen) return false;
            seen = true;
            current = token.substr(equal + 1) == L"page";
        }
        token.clear();
        return true;
    };
    if (properties.empty()) return false;
    for (wchar_t character : properties) {
        if (escaped) {
            if (character != L'\\' && character != L';' && character != L'=') return std::nullopt;
            token.push_back(L'\\'); token.push_back(character); escaped = false;
        } else if (character == L'\\') escaped = true;
        else if (character == L';') { if (!consume()) return std::nullopt; }
        else token.push_back(character);
    }
    if (escaped || !consume()) return std::nullopt;
    return current;
}
struct UiaModeCounts {
    unsigned groups{}, chat{}, cowork{}, current_chat{}, current_cowork{};
    bool operator==(const UiaModeCounts& other) const {
        return groups == other.groups && chat == other.chat && cowork == other.cowork
            && current_chat == other.current_chat && current_cowork == other.current_cowork;
    }
};
inline const char* uia_mode_status(const UiaModeCounts& counts, bool available, bool unchanged) {
    if (!available) return "unavailable";
    if (!unchanged) return "changed";
    if (counts.groups > 1 || counts.chat > 1 || counts.cowork > 1
        || counts.current_chat + counts.current_cowork > 1) return "ambiguous";
    if (counts.groups != 1 || counts.chat != 1 || counts.cowork != 1
        || counts.current_chat + counts.current_cowork != 1) return "missing";
    return counts.current_chat ? "chat" : "cowork";
}
