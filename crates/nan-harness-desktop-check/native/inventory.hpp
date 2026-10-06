#pragma once

#include <CoreFoundation/CoreFoundation.h>

#include <sys/types.h>

// Closed, metadata-free classification for synthetic tests and the failure-only
// Claude diagnostic. The caller retains ownership of inventory.
const char* classify_window_inventory(CFArrayRef inventory, pid_t expected_pid);
