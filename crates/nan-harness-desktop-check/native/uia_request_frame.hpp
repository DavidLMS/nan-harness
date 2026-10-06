#pragma once
#include <istream>
#include <optional>
#include <string>

// Transport sends exactly one bounded line, then closes stdin. No trailing
// byte is accepted, including a second newline from a duplicated delimiter.
inline std::optional<std::string> uia_request_frame(std::istream& input) {
    char wire[257] = {};
    if (!input.getline(wire, sizeof(wire)) || input.eof()
        || input.peek() != std::char_traits<char>::eof()) return std::nullopt;
    return std::string(wire);
}
