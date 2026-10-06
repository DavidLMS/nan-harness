#include "uia_request_frame.hpp"
#include <sstream>

int main() {
    const std::string payload = "1 7 10 20 810 620 3000";
    std::istringstream valid(payload + "\n");
    const auto parsed = uia_request_frame(valid);
    if (!parsed || *parsed != payload) return 1;
    for (const auto& invalid : {payload, payload + "\n\n", payload + "\nPRIVATE",
                               std::string(257, '1') + "\n", std::string()}) {
        std::istringstream input(invalid);
        if (uia_request_frame(input)) return 2;
    }
    std::istringstream maximum(std::string(256, '1') + "\n");
    if (!uia_request_frame(maximum)) return 3;
    return 0;
}
