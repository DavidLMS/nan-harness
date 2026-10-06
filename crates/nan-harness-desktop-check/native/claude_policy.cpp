// Source-pinned managed-policy absence only. Never changes Registry state.
#include <iostream>
#include <initializer_list>
#ifdef _WIN32
#include <windows.h>

int claude_policy_absence() {
    // The pinned bootstrap uses app.getName() || "Claude". Inspect both known
    // product names conservatively, in both registry views, including empty keys.
    for (HKEY hive : {HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE}) {
        for (const wchar_t* key : {L"SOFTWARE\\Policies\\Claude", L"SOFTWARE\\Policies\\Claude-3p"}) {
            for (REGSAM view : {KEY_WOW64_32KEY, KEY_WOW64_64KEY}) {
                HKEY opened = nullptr;
                const LSTATUS status = RegOpenKeyExW(hive, key, 0, KEY_READ | view, &opened);
                if (status == ERROR_SUCCESS) {
                    RegCloseKey(opened);
                    std::cout << "present\n";
                    return 0;
                }
                if (status != ERROR_FILE_NOT_FOUND && status != ERROR_PATH_NOT_FOUND) {
                    std::cout << "unreadable\n";
                    return 0;
                }
            }
        }
    }
    std::cout << "not-found\n";
    return 0;
}
#else
int claude_policy_absence() { return 5; }
#endif
