#include <stdint.h>
#include "inputs.h"

static uint64_t rol64(uint64_t x, unsigned n) {
    n &= 63;
    return (x << n) | (x >> ((-n) & 63));
}
static uint32_t ror32(uint32_t x, unsigned n) {
    n &= 31;
    return (x >> n) | (x << ((-n) & 31));
}

__attribute__((noinline))
static uint64_t counts(const uint64_t *words, unsigned count) {
    uint64_t sum = 0;
    for (unsigned repeat = 0; repeat < 8; ++repeat)
        for (unsigned i = 0; i < count; ++i) {
            uint64_t x = words[i];
            sum += __builtin_popcountll(x) + __builtin_clzll(x | 1)
                + __builtin_ctzll(x | UINT64_C(0x8000000000000000))
                + __builtin_popcount((uint32_t)x) + __builtin_clz((uint32_t)x | 1)
                + __builtin_ctz((uint32_t)x | 0x80000000U);
        }
    return sum;
}

__attribute__((noinline))
static uint64_t rotate(const uint64_t *words, unsigned count) {
    uint64_t hash = UINT64_C(0x1234567887654321);
    for (unsigned repeat = 0; repeat < 8; ++repeat)
        for (unsigned i = 0; i < count; ++i) {
            hash = rol64(hash ^ words[i], i * 7 + 13);
            hash += ror32((uint32_t)words[i], i * 11 + 3);
        }
    return hash;
}

__attribute__((noinline))
static uint64_t bytes(const uint64_t *words, unsigned count) {
    uint64_t sum = 0;
    for (unsigned repeat = 0; repeat < 8; ++repeat)
        for (unsigned i = 0; i < count; ++i) {
            int64_t x = (int64_t)words[i];
            int64_t lo = x < -1234567 ? x : -1234567;
            int64_t hi = x > 7654321 ? x : 7654321;
            sum += __builtin_bswap64((uint64_t)x) + (uint64_t)lo + (uint64_t)hi;
            sum ^= (uint64_t)(int64_t)(int8_t)x + (uint64_t)(int64_t)(int16_t)x + (uint16_t)x;
        }
    return sum;
}

uint64_t run(void) {
#if defined(COUNTS)
    return counts(words, 128) == EXPECTED_COUNTS ? 1 : 3;
#elif defined(ROTATE)
    return rotate(words, 128) == EXPECTED_ROTATE ? 1 : 5;
#else
    return bytes(words, 128) == EXPECTED_BYTES ? 1 : 7;
#endif
}
