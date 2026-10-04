use celox::{NativeBackend, Simulator, SimulatorBuilder};
use std::{collections::BTreeMap, fs, path::Path};
fn get(s: &mut Simulator<NativeBackend>, n: &str) -> u64 {
    s.get(s.signal(n))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}
fn set(s: &mut Simulator<NativeBackend>, n: &str, v: u64) {
    // Drive both acknowledgments in the shared-memory fixture.
    if n == "memory_done" {
        set(s, "store_done", v);
    }
    let p = s.signal(n);
    s.modify(|io| {
        if matches!(n, "address" | "source_value" | "memory_value") {
            io.set(p, v);
        } else {
            io.set(p, v as u8);
        }
    })
    .unwrap();
}
fn tick(s: &mut Simulator<NativeBackend>) {
    let c = s.event("clk");
    s.tick(c).unwrap();
}
fn transaction(
    s: &mut Simulator<NativeBackend>,
    mem: &mut BTreeMap<u64, u64>,
    op: u64,
    word: bool,
    address: u64,
    value: u64,
) -> (u64, u64, usize) {
    set(s, "start", 1);
    set(s, "consume", 0);
    set(s, "operation", op);
    set(s, "word_op", u64::from(word));
    set(s, "address", address);
    set(s, "source_value", value);
    set(s, "memory_done", 1);
    tick(s);
    let mut delay = 0;
    let mut requests = 0;
    for _ in 0..40 {
        if get(s, "done") != 0 {
            let result = get(s, "result");
            let cause = if get(s, "fault") != 0 {
                get(s, "fault_cause")
            } else {
                0
            };
            // An unconsumed response must not issue another access.
            tick(s);
            assert_eq!(get(s, "issue"), 0);
            assert_eq!(get(s, "result"), result);
            set(s, "start", 0);
            set(s, "consume", 1);
            tick(s);
            // Prevent a successor or board tohost observer seeing stale store state.
            assert_eq!(get(s, "written"), 0);
            assert_eq!(get(s, "done"), 0);
            set(s, "consume", 0);
            return (result, cause, requests);
        }
        if get(s, "issue") != 0 {
            requests += 1;
            set(s, "memory_done", 0);
            // written becomes visible on the issue edge, together with the token.
            tick(s);
            if get(s, "written") != 0 {
                // A stale load acknowledgment must not complete a store.
                let load_ack = s.signal("memory_done");
                s.modify(|io| io.set(load_ack, 1_u8)).unwrap();
                let v = get(s, "write_value");
                let old = *mem.get(&address).unwrap_or(&0);
                mem.insert(
                    address,
                    if word {
                        (old & 0xffff_ffff_0000_0000) | (v & 0xffff_ffff)
                    } else {
                        v
                    },
                );
            } else {
                // A stale store acknowledgment must not complete a load.
                set(s, "store_done", 1);
                set(s, "memory_value", *mem.get(&address).unwrap_or(&0));
            }
            delay = 3;
        } else {
            if delay > 0 {
                assert_eq!(get(s, "done"), 0, "wrong acknowledgment retired an atomic");
                delay -= 1;
                if delay == 0 {
                    set(s, "memory_done", 1);
                }
            }
            tick(s);
        }
    }
    panic!("atomic transaction timed out");
}
#[test]
fn native_and_mapped_lsu_reservations_faults_and_transaction_boundaries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let source = ["AtomicAlu.veryl", "AtomicLsu.veryl"]
        .map(|n| fs::read_to_string(root.join(n)).unwrap())
        .join("\n");
    let native = SimulatorBuilder::new(&source, "AtomicLsu")
        .build_native()
        .unwrap();
    let rtl =
        struo_frontend_veryl::analyze_and_lower(&source, "atomic_lsu_test", "AtomicLsu").unwrap();
    let mapped = struo_target_ecp5::map_to_ecp5_with_options(
        &struo_synth::synthesize(&rtl).unwrap().netlist,
        struo_target_ecp5::MappingOptions {
            timing_goal_mhz: 125,
            ..Default::default()
        },
    )
    .unwrap();
    let mapped = struo_celox::ecp5_simulator(&mapped)
        .unwrap()
        .build_native()
        .unwrap();
    for mut s in [native, mapped] {
        set(&mut s, "rst_n", 0);
        tick(&mut s);
        set(&mut s, "rst_n", 1);
        let a = 0x8000_1000;
        let b = a + 8;
        let mut mem = BTreeMap::from([(a, 0x1234_5678_8000_0000), (b, 9)]);
        assert_eq!(transaction(&mut s, &mut mem, 3, false, a, 7), (1, 0, 0));
        assert_eq!(
            transaction(&mut s, &mut mem, 2, true, a, 0),
            ((-2147483648_i64) as u64, 0, 1)
        );
        assert_eq!(transaction(&mut s, &mut mem, 3, true, a, 7), (0, 0, 1));
        assert_eq!(mem[&a], 0x1234_5678_0000_0007);
        assert_eq!(transaction(&mut s, &mut mem, 3, true, a, 8), (1, 0, 0));
        assert_eq!(transaction(&mut s, &mut mem, 0, false, b, 4), (9, 0, 2));
        assert_eq!(mem[&b], 13);
        // Every legal AMO must take the read/modify/write path, including
        // funct5 values with nonzero high bits. It must also consume an older
        // LR reservation at a different address. Check both transfer widths.
        let old = 0xffff_ffff_8000_0003;
        for word in [false, true] {
            for (op, expected) in [
                (0, 0xffff_ffff_8000_0008),
                (1, 5),
                (4, 0xffff_ffff_8000_0006),
                (8, 0xffff_ffff_8000_0007),
                (12, 1),
                (16, old),
                (20, 5),
                (24, 5),
                (28, old),
            ] {
                mem.insert(b, old);
                transaction(&mut s, &mut mem, 2, false, a, 0);
                assert_eq!(transaction(&mut s, &mut mem, op, word, b, 5), (old, 0, 2));
                assert_eq!(
                    mem[&b],
                    if word {
                        (old & 0xffff_ffff_0000_0000) | (expected & 0xffff_ffff)
                    } else {
                        expected
                    }
                );
                assert_eq!(transaction(&mut s, &mut mem, 3, false, a, 7), (1, 0, 0));
            }
        }
        transaction(&mut s, &mut mem, 2, false, a, 0);
        set(&mut s, "invalidate", 1);
        tick(&mut s);
        set(&mut s, "invalidate", 0);
        assert_eq!(transaction(&mut s, &mut mem, 3, false, a, 0), (1, 0, 0));
        transaction(&mut s, &mut mem, 2, false, a, 0);
        assert_eq!(transaction(&mut s, &mut mem, 3, false, b, 0), (1, 0, 0));
        assert_eq!(transaction(&mut s, &mut mem, 3, false, a, 0), (1, 0, 0));
        // A compact reservation tag must retain every RAM word-address bit.
        for offset in [0x1_0000, 0x2_0000] {
            transaction(&mut s, &mut mem, 2, false, a, 0);
            assert_eq!(
                transaction(&mut s, &mut mem, 3, false, a + offset, 0),
                (1, 0, 0)
            );
        }
        // Identical low address bits outside RAM must fault, even with a
        // live reservation; the faulting SC must also consume that reservation.
        for offset in [0x4_0000, 0x1_0000_0000] {
            transaction(&mut s, &mut mem, 2, false, a, 0);
            let old = mem.clone();
            assert_eq!(
                transaction(&mut s, &mut mem, 3, false, a + offset, 0),
                (0, 7, 0)
            );
            assert_eq!(mem, old);
            assert_eq!(transaction(&mut s, &mut mem, 3, false, a, 0), (1, 0, 0));
        }
        transaction(&mut s, &mut mem, 2, false, a, 0);
        set(&mut s, "rst_n", 0);
        tick(&mut s);
        set(&mut s, "rst_n", 1);
        assert_eq!(transaction(&mut s, &mut mem, 3, false, a, 0), (1, 0, 0));
        for (op, address, cause) in [
            (2, a + 1, 4),
            (3, a + 1, 6),
            (0, a + 1, 6),
            (2, 0x0200_bff8, 5),
            (3, 0x0200_4000, 7),
            (0, 0x1_8000_0000, 7),
            (0, 0x8003_f000, 7),
        ] {
            let old = mem.clone();
            assert_eq!(
                transaction(&mut s, &mut mem, op, false, address, 0),
                (0, cause, 0)
            );
            assert_eq!(mem, old);
        }
    }
}
