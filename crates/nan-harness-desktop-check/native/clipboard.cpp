// Native clipboard boundary. Only bounded UTF-8 crosses the private pipe.
#include <string>
#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <array>
#include <vector>

namespace {
constexpr std::size_t input_limit = 1024;
constexpr std::size_t output_limit = 65536;

template<class T> struct PrivateBuffer {
    T value;
    ~PrivateBuffer() {
        if (!value.empty()) SecureZeroMemory(value.data(), value.size() * sizeof(value[0]));
    }
};

struct Clipboard {
    HWND window = CreateWindowExW(0, L"STATIC", L"", 0, 0, 0, 0, 0,
                                  HWND_MESSAGE, nullptr, GetModuleHandleW(nullptr), nullptr);
    bool opened = false;
    ~Clipboard() {
        if (opened) CloseClipboard();
        if (window) DestroyWindow(window);
    }
    bool open() { opened = window && OpenClipboard(window); return opened; }
    bool close() {
        if (!CloseClipboard()) return false;
        opened = false;
        return true;
    }
};

struct Allocation {
    HGLOBAL handle;
    void* locked = nullptr;
    std::size_t bytes;
    explicit Allocation(std::size_t size) : handle(GlobalAlloc(GMEM_MOVEABLE, size)), bytes(size) {}
    ~Allocation() {
        if (!handle) return; // SetClipboardData transferred ownership.
        if (!locked) locked = GlobalLock(handle);
        if (locked) {
            SecureZeroMemory(locked, bytes);
            GlobalUnlock(handle);
        }
        GlobalFree(handle);
    }
};

int write_text(const char* input, int length) {
    int units = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, input, length, nullptr, 0);
    if (units <= 0) return 2;
    Allocation data((static_cast<std::size_t>(units) + 1) * sizeof(wchar_t));
    if (!data.handle || !(data.locked = GlobalLock(data.handle))) return 5;
    auto text = static_cast<wchar_t*>(data.locked);
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, input, length, text, units) != units) return 2;
    text[units] = L'\0';
    GlobalUnlock(data.handle);
    data.locked = nullptr;
    Clipboard clipboard;
    if (!clipboard.open() || !EmptyClipboard()) return 5;
    if (!SetClipboardData(CF_UNICODETEXT, data.handle)) return 5;
    data.handle = nullptr;
    return clipboard.close() ? 0 : 5;
}

int read_text() {
    Clipboard clipboard;
    if (!clipboard.open()) return 5;
    if (!IsClipboardFormatAvailable(CF_UNICODETEXT)) return clipboard.close() ? 0 : 5;
    HGLOBAL handle = GetClipboardData(CF_UNICODETEXT);
    if (!handle) return 5;
    SIZE_T size = GlobalSize(handle);
    if (size < sizeof(wchar_t) || size > (output_limit + 1) * sizeof(wchar_t)
        || size % sizeof(wchar_t)) return 4;
    auto text = static_cast<const wchar_t*>(GlobalLock(handle));
    if (!text) return 5;
    std::size_t units = 0;
    while (units < size / sizeof(wchar_t) && text[units] != L'\0') ++units;
    if (units == size / sizeof(wchar_t)) { GlobalUnlock(handle); return 4; }
    int length = units == 0 ? 0 : WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS,
        text, static_cast<int>(units), nullptr, 0, nullptr, nullptr);
    if (length < 0 || (units != 0 && length == 0) || length > static_cast<int>(output_limit)) {
        GlobalUnlock(handle); return 4;
    }
    PrivateBuffer<std::vector<char>> output{std::vector<char>(static_cast<std::size_t>(length))};
    bool converted = length == 0 || WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS,
        text, static_cast<int>(units), output.value.data(), length, nullptr, nullptr) == length;
    GlobalUnlock(handle);
    if (!converted) return 4;
    if (!clipboard.close()) return 5;
    DWORD written = 0;
    if (length && (!WriteFile(GetStdHandle(STD_OUTPUT_HANDLE), output.value.data(),
                             static_cast<DWORD>(length), &written, nullptr)
                   || written != static_cast<DWORD>(length))) return 4;
    return 0;
}
}

int clipboard_operation(const std::string& operation) {
    PrivateBuffer<std::array<char, input_limit + 1>> input{};
    std::size_t length = 0;
    while (length < input.value.size()) {
        DWORD received = 0;
        if (!ReadFile(GetStdHandle(STD_INPUT_HANDLE), input.value.data() + length,
                      static_cast<DWORD>(input.value.size() - length), &received, nullptr)) {
            if (GetLastError() == ERROR_BROKEN_PIPE) break;
            return 2;
        }
        if (received == 0) break;
        length += received;
    }
    if (length > input_limit) return 2;
    for (std::size_t index = 0; index < length; ++index) if (input.value[index] == '\0') return 2;
    if (operation == "write") {
        if (length == 0) return 2;
        return write_text(input.value.data(), static_cast<int>(length));
    }
    if (length != 0) return 2;
    if (operation == "read") return read_text();
    if (operation == "clear") {
        Clipboard clipboard;
        if (!clipboard.open() || !EmptyClipboard()) return 5;
        return clipboard.close() ? 0 : 5;
    }
    return 2;
}
#else
int clipboard_operation(const std::string&) { return 5; }
#endif
