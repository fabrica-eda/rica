#include <stdint.h>
#include "inputs.h"

__attribute__((noinline))
static uint64_t mutate(const uint64_t *words, unsigned count) {
    uint64_t sum = 0;
    for (unsigned repeat = 0; repeat < 8; ++repeat)
        for (unsigned i = 0; i < count; ++i) {
            uint64_t x = words[i];
            x |= UINT64_C(1) << ((i * 3 + 5) & 63);
            x &= ~(UINT64_C(1) << ((i * 7 + 11) & 63));
            x ^= UINT64_C(1) << ((i * 13 + 3) & 63);
            sum += x;
        }
    return sum;
}

__attribute__((noinline))
static uint64_t extract(const uint64_t *words, unsigned count) {
    uint64_t sum = 0;
    for (unsigned repeat = 0; repeat < 8; ++repeat)
        for (unsigned i = 0; i < count; ++i) {
            uint64_t x = words[i];
            sum += ((x >> (i & 63)) & 1)
                + ((x >> ((i * 7 + 13) & 63)) & 1)
                + ((x >> ((i * 11 + 31) & 63)) & 1);
        }
    return sum;
}

__attribute__((noinline))
static uint64_t immediate(const uint64_t *words, unsigned count) {
    uint64_t sum = 0;
    for (unsigned repeat = 0; repeat < 8; ++repeat)
        for (unsigned i = 0; i < count; ++i) {
            uint64_t x = words[i];
            sum += ((x >> 37) & 1) + ((x >> 19) & 1) + ((x >> 5) & 1);
            sum ^= (x | (UINT64_C(1) << 47)) & ~(UINT64_C(1) << 29);
        }
    return sum;
}

uint64_t run(void) {
#if defined(MUTATE)
    return mutate(words, 128) == EXPECTED_MUTATE ? 1 : 3;
#elif defined(EXTRACT)
    return extract(words, 128) == EXPECTED_EXTRACT ? 1 : 5;
#else
    return immediate(words, 128) == EXPECTED_IMMEDIATE ? 1 : 7;
#endif
}
