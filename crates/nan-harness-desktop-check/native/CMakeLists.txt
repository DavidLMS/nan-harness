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
