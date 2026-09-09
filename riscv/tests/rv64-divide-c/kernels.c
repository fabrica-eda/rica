#include <stdint.h>
#include "inputs.h"

uint64_t run(void) {
    uint64_t checksum = 0;
    for (unsigned repeat = 0; repeat < 2; ++repeat) {
        for (unsigned i = 0; i < 128; ++i) {
#ifdef WORDS
            int32_t a = (int32_t)pairs[i][0];
            int32_t b = (int32_t)pairs[i][1];
            checksum += (uint64_t)(int64_t)(a / b);
            checksum += (uint64_t)(int64_t)(a % b);
#else
            uint64_t a = pairs[i][0];
            uint64_t b = pairs[i][1];
            checksum += a / b;
            checksum += a % b;
#endif
        }
    }
    return checksum == EXPECTED ? 1 : 3;
}
