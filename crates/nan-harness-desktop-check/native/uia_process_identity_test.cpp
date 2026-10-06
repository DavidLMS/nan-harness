#include "uia_process_identity.hpp"
#include <cstring>
#include <limits>
#include <initializer_list>

int main() {
    for (bool root : {true, false}) {
        if (uia_process_identity_failure(7, 7, root)) return 1;
        const auto matches = [&](int reported, const char* expected) {
            const char* actual=uia_process_identity_failure(reported,7,root);
            return actual && std::strcmp(actual,expected)==0;
        };
        if (!matches(0,root?"root-process-zero":"descendant-process-zero")) return 2;
        if (!matches(-1,root?"root-process-invalid":"descendant-process-invalid")) return 3;
        if (!matches(8,root?"root-process-mismatch":"descendant-process-mismatch")) return 4;
        if (!uia_process_identity_failure(std::numeric_limits<int>::max(),
                                         std::numeric_limits<std::uint32_t>::max(),root)) return 5;
    }
    return 0;
}
