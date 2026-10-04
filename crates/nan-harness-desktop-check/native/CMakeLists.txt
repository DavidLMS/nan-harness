cmake_minimum_required(VERSION 3.20)
project(nanh_desktop_native LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
find_library(TESSERACT_LIBRARY NAMES tesseract tesseract55 PATHS "${NAN_OCR_PREFIX}/lib" NO_DEFAULT_PATH REQUIRED)
find_library(LEPTONICA_LIBRARY NAMES leptonica leptonica-1.87.0 PATHS "${NAN_OCR_PREFIX}/lib" NO_DEFAULT_PATH REQUIRED)
add_executable(nanh-desktop-native main.cpp windows.cpp clipboard.cpp claude_chat_turn.cpp windows_uia_inventory.cpp)
target_include_directories(nanh-desktop-native PRIVATE "${NAN_OCR_PREFIX}/include")
target_link_libraries(nanh-desktop-native PRIVATE "${TESSERACT_LIBRARY}" "${LEPTONICA_LIBRARY}")
if(APPLE)
  enable_language(OBJCXX)
  set_source_files_properties(windows.cpp claude_chat_turn.cpp PROPERTIES LANGUAGE OBJCXX)
  target_link_libraries(nanh-desktop-native PRIVATE "-framework AppKit" "-framework CoreGraphics" "-framework ApplicationServices")
  # Synthetic nodes and focus receipts only; no native app or input API calls.
  add_executable(nanh-claude-chat-turn-test tests/claude_chat_turn.mm)
  target_compile_options(nanh-claude-chat-turn-test PRIVATE -UNDEBUG)
  target_link_libraries(nanh-claude-chat-turn-test PRIVATE "-framework AppKit" "-framework CoreGraphics" "-framework ApplicationServices")
  add_custom_target(nanh-claude-chat-turn-contract ALL
    COMMAND $<TARGET_FILE:nanh-claude-chat-turn-test>
    DEPENDS nanh-claude-chat-turn-test
    COMMENT "Verify retained composer focus and source-bound chat selection")
elseif(WIN32)
  target_link_libraries(nanh-desktop-native PRIVATE user32 dwmapi shell32 ole32 oleaut32 uiautomationcore uuid bcrypt)
else()
  find_package(X11 REQUIRED)
  find_package(Threads REQUIRED)
  target_link_libraries(nanh-desktop-native PRIVATE X11::X11 Threads::Threads)
endif()
install(TARGETS nanh-desktop-native RUNTIME DESTINATION .)

# This contract calls only pure ancestry logic with synthetic identities.
add_executable(nanh-process-correlation-test process_correlation_test.cpp)
add_custom_target(nanh-process-correlation-contract ALL
  COMMAND $<TARGET_FILE:nanh-process-correlation-test>
  DEPENDS nanh-process-correlation-test
  COMMENT "Verify bounded process ancestry correlation")

# Pure framing contract runs during the native preinstallation build on Windows.
add_executable(nanh-uia-request-frame-test uia_request_frame_test.cpp)
add_custom_target(nanh-uia-request-frame-contract ALL
  COMMAND $<TARGET_FILE:nanh-uia-request-frame-test>
  DEPENDS nanh-uia-request-frame-test
  COMMENT "Verify bounded UIA request framing")

# Synthetic identities only; never creates or inspects a native window.
add_executable(nanh-uia-process-identity-test uia_process_identity_test.cpp)
add_custom_target(nanh-uia-process-identity-contract ALL
  COMMAND $<TARGET_FILE:nanh-uia-process-identity-test>
  DEPENDS nanh-uia-process-identity-test
  COMMENT "Verify closed UIA process identity rejection")

add_executable(nanh-uia-process-ancestry-test uia_process_ancestry_test.cpp)
add_custom_target(nanh-uia-process-ancestry-contract ALL
  COMMAND $<TARGET_FILE:nanh-uia-process-ancestry-test>
  DEPENDS nanh-uia-process-ancestry-test
  COMMENT "Verify passive UIA descendant ancestry classification")

# Public property grammar and source mode classification only; no UIA calls.
add_executable(nanh-uia-current-mode-test tests/uia_current_mode.cpp)
add_custom_target(nanh-uia-current-mode-contract ALL
  COMMAND $<TARGET_FILE:nanh-uia-current-mode-test>
  DEPENDS nanh-uia-current-mode-test
  COMMENT "Verify passive UIA current-mode classification")

add_executable(nanh-uia-chat-capability-test tests/uia_chat_capability.cpp)
add_custom_target(nanh-uia-chat-capability-contract ALL
  COMMAND $<TARGET_FILE:nanh-uia-chat-capability-test>
  DEPENDS nanh-uia-chat-capability-test
  COMMENT "Verify passive source-bound UIA Chat capability facts")

add_executable(nanh-uia-attachment-test tests/uia_attachment.cpp)
add_custom_target(nanh-uia-attachment-contract ALL
  COMMAND $<TARGET_FILE:nanh-uia-attachment-test>
  DEPENDS nanh-uia-attachment-test
  COMMENT "Verify retained UIA subtree attachment")

# Contracts use assert; release builds must execute their checks as well.
foreach(contract_target IN ITEMS nanh-process-correlation-test nanh-uia-request-frame-test
    nanh-uia-process-identity-test nanh-uia-process-ancestry-test nanh-uia-current-mode-test
    nanh-uia-attachment-test nanh-uia-chat-capability-test)
  if(MSVC)
    target_compile_options(${contract_target} PRIVATE /UNDEBUG)
  else()
    target_compile_options(${contract_target} PRIVATE -UNDEBUG)
  endif()
endforeach()

add_executable(nanh-uia-chat-input-test tests/uia_chat_input.cpp)
if(MSVC)
  target_compile_options(nanh-uia-chat-input-test PRIVATE /UNDEBUG)
else()
  target_compile_options(nanh-uia-chat-input-test PRIVATE -UNDEBUG)
endif()
add_custom_target(nanh-uia-chat-input-contract ALL
  COMMAND $<TARGET_FILE:nanh-uia-chat-input-test>
  DEPENDS nanh-uia-chat-input-test
  COMMENT "Verify private Windows Chat framing and passive readiness predicates")
