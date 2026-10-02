# Native synthetic fixtures

The Claude inventory fixture uses only synthetic CoreFoundation dictionaries;
it never calls CoreGraphics window enumeration or AppKit process APIs. On macOS,
run it from the repository root with:

```sh
test_dir=$(mktemp -d /private/tmp/nan-claude-inventory-test.XXXXXX)
clang++ -std=c++17 -x objective-c++ \
  crates/nan-harness-desktop-check/native/tests/claude_inventory.mm \
  crates/nan-harness-desktop-check/native/windows.cpp \
  -framework CoreGraphics -framework AppKit -framework ApplicationServices -o "$test_dir/fixture" && \
  "$test_dir/fixture"
```

The focus fixture also uses only synthetic dictionaries and pure identity
classification; it never queries accessibility, windows or running processes.

```sh
clang++ -std=c++17 -x objective-c++ \
  crates/nan-harness-desktop-check/native/tests/window_focus.mm \
  crates/nan-harness-desktop-check/native/windows.cpp \
  -framework CoreGraphics -framework AppKit -framework ApplicationServices -o "$test_dir/focus" && \
  "$test_dir/focus"
```
