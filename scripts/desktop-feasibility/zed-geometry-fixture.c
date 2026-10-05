/* Synthetic numeric layout for the pinned geometry probe; no vendor code. */
#include <stdint.h>
#include <string.h>

__attribute__((noinline)) void synthetic_dispatch(void *window) {
    __asm__ volatile("" : : "r"(window) : "memory");
}

int main(void) {
    unsigned char window[8192] = {0};
    unsigned char boxes[48 * 1024] = {0};
    const float target[8] = {10, 20, 40, 20, 0, 0, 200, 200};
    const float blocker[8] = {0, 0, 200, 200, 0, 0, 200, 200};
    const float outside[8] = {-10, -20, 3, 4, -10, -20, 3, 4};
    const float pointer[2] = {30, 30};
    const float viewport[2] = {200, 200};
    const uintptr_t base = (uintptr_t)boxes;
    const uint64_t count = 1024;
    memcpy(boxes + 8, target, sizeof(target));
    memcpy(boxes + 48 + 8, outside, sizeof(outside));
    memcpy(boxes + 48 * 1023 + 8, blocker, sizeof(blocker));
    boxes[48 * 1023 + 40] = 1;
    memcpy(window + 592, &base, sizeof(base));
    memcpy(window + 600, &count, sizeof(count));
    memcpy(window + 7840, pointer, sizeof(pointer));
    memcpy(window + 6416, viewport, sizeof(viewport));
    synthetic_dispatch(window);
    return 0;
}
