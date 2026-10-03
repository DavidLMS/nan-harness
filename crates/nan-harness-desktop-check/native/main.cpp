// Private native boundary for bounded clipboard data, window metadata and offline OCR.
#include <tesseract/baseapi.h>
#include <cstdint>
#include <iostream>
#include <memory>
#include <sstream>
#include <string>
#include <vector>
#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

int list_windows(bool include_foreground);
int windows_focus(const std::string& request);
int fit_window(const std::string& request);
int window_state(const std::string& request);
int activate_window(const std::string& request);
int observe_claude();
int claude_known_folders();
int windows_claude_storage();
int process_presence(bool claude);
int clipboard_operation(const std::string& operation);

int main(int argc, char** argv) {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    if (argc == 2 && std::string(argv[1]) == "--clipboard-read") return clipboard_operation("read");
    if (argc == 2 && std::string(argv[1]) == "--clipboard-write") return clipboard_operation("write");
    if (argc == 2 && std::string(argv[1]) == "--clipboard-clear") return clipboard_operation("clear");
    if (argc == 2 && std::string(argv[1]).rfind("--window-state ", 0) == 0)
        return window_state(std::string(argv[1]).substr(15));
    if (argc == 2 && std::string(argv[1]).rfind("--fit-window ", 0) == 0)
        return fit_window(std::string(argv[1]).substr(13));
    if (argc == 2 && std::string(argv[1]).rfind("--activate-window ", 0) == 0)
        return activate_window(std::string(argv[1]).substr(18));
    if (argc == 2 && std::string(argv[1]).rfind("--windows-focus ", 0) == 0)
        return windows_focus(std::string(argv[1]).substr(16));
    if (argc == 2 && std::string(argv[1]) == "--claude-process-presence") return process_presence(true);
    if (argc == 2 && std::string(argv[1]) == "--codex-process-presence") return process_presence(false);
    if (argc == 2 && std::string(argv[1]) == "--windows") return list_windows(true);
    if (argc == 2 && std::string(argv[1]) == "--windows-absence") return list_windows(false);
    if (argc == 2 && std::string(argv[1]) == "--claude-known-folders") return claude_known_folders();
    if (argc == 2 && std::string(argv[1]) == "--windows-claude-storage") return windows_claude_storage();
    if (argc == 2 && std::string(argv[1]) == "--claude-observation") return observe_claude();
    if (argc == 2 && std::string(argv[1]) == "--version") {
        std::cout << "nanh-desktop-native tesseract-" << tesseract::TessBaseAPI::Version() << '\n';
        return 0;
    }
    if (argc != 2) return 2;
    char header[64] = {};
    if (!std::cin.getline(header, sizeof(header))) return 2;
    std::istringstream dimensions(header);
    std::uint32_t width = 0, height = 0;
    std::string extra;
    if (!(dimensions >> width >> height) || (dimensions >> extra) || width == 0 || height == 0
        || width > 8192 || height > 8192 || std::uint64_t(width) * height > 16 * 1024 * 1024) return 2;
    std::vector<unsigned char> pixels(std::size_t(width) * height * 4);
    if (!std::cin.read(reinterpret_cast<char*>(pixels.data()), pixels.size())) return 2;
    if (std::cin.peek() != std::char_traits<char>::eof()) return 2;
    tesseract::TessBaseAPI api;
    if (api.Init(argv[1], "eng", tesseract::OEM_LSTM_ONLY) != 0) return 3;
#ifdef _WIN32
    api.SetVariable("debug_file", "NUL");
#else
    api.SetVariable("debug_file", "/dev/null");
#endif
    api.SetPageSegMode(tesseract::PSM_SPARSE_TEXT);
    api.SetImage(pixels.data(), width, height, 4, width * 4);
    api.SetSourceResolution(144);
    std::unique_ptr<char[]> text(api.GetTSVText(0));
    if (!text) return 3;
    std::string output(text.get());
    if (output.size() > 512 * 1024) return 4;
    std::cout << output;
    return std::cout ? 0 : 4;
}
