#ifndef _RICA_RVMODEL_MACROS_H
#define _RICA_RVMODEL_MACROS_H

// Rica's hardware harness has a memory-backed tohost mailbox and no console.
#define RVMODEL_DATA_SECTION                                      \
  .pushsection .tohost,"aw",@progbits;                            \
  .balign 8; .global tohost; tohost: .dword 0;                    \
  .balign 8; .global fromhost; fromhost: .dword 0;                \
  .popsection

#define RVMODEL_BOOT
#define RVMODEL_BOOT_TO_MMODE

#define RVMODEL_HALT_PASS                                        \
  li x1, 1;                                                       \
  la t0, tohost;                                                  \
write_tohost_pass:                                                \
  sw x1, 0(t0);                                                   \
  sw x0, 4(t0);                                                   \
  j write_tohost_pass;

#define RVMODEL_HALT_FAIL                                        \
  li x1, 3;                                                       \
  la t0, tohost;                                                  \
write_tohost_fail:                                                \
  sw x1, 0(t0);                                                   \
  sw x0, 4(t0);                                                   \
  j write_tohost_fail;

#define RVMODEL_IO_INIT(_R1, _R2, _R3)
#define RVMODEL_IO_WRITE_STR(_R1, _R2, _R3, _STR_PTR)

// Interrupt tests are not selected for this RV64I unprivileged catalog, but
// ACT4 requires every DUT environment to define their control hooks.
#define RVMODEL_INTERRUPT_LATENCY 1
#define RVMODEL_TIMER_INT_SOON_DELAY 100
#define RVMODEL_MAX_CYCLES_PER_TIMER_TICK 1
#define RVMODEL_SET_MEXT_INT(_R1, _R2)
#define RVMODEL_CLR_MEXT_INT(_R1, _R2)
#define RVMODEL_SET_MSW_INT(_R1, _R2)
#define RVMODEL_CLR_MSW_INT(_R1, _R2)
#define RVMODEL_SET_SEXT_INT(_R1, _R2)
#define RVMODEL_CLR_SEXT_INT(_R1, _R2)
#define RVMODEL_SET_SSW_INT(_R1, _R2)
#define RVMODEL_CLR_SSW_INT(_R1, _R2)

#endif
