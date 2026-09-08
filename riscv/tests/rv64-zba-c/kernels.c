#include <stdint.h>
#include "inputs.h"

__attribute__((noinline))
static uint64_t gather(const uint16_t *table, const uint32_t *indices, unsigned count) {
    uint64_t sum = 0;
    for (unsigned repeat = 0; repeat < 8; ++repeat)
        for (unsigned i = 0; i < count; ++i)
            sum += table[indices[i]];
    return sum;
}

__attribute__((noinline))
static uint64_t addresses(const uint64_t *indices, unsigned count, uint64_t base) {
    uint64_t sum = 0;
    for (unsigned repeat = 0; repeat < 8; ++repeat)
        for (unsigned i = 0; i < count; ++i) {
            uint64_t index = indices[i];
            sum ^= base + ((uint64_t)(uint32_t)index << 3);
            sum += base + (index << 2);
        }
    return sum;
}

uint64_t run(void) {
#ifdef GATHER
    return gather(table, indices32, 128) == EXPECTED_GATHER ? 1 : 3;
#else
    return addresses(indices64, 128, UINT64_C(0x1234567800000100)) == EXPECTED_ADDRESSES ? 1 : 5;
#endif
}
