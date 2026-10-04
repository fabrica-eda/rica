use celox::SimulatorBuilder;
use std::{fs, path::Path};

#[test]
fn access_end_preserves_carries_across_physical_address_boundaries() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/PmpCheck.veryl"))
            .unwrap();
    let mut addresses: Vec<u64> = (0..1024).collect();
    // Exercise every bit boundary, not just the implementation's slice sizes.
    for bit in 1..=32 {
        for delta in -8_i64..=8 {
            addresses.push((1_u64 << bit).wrapping_add_signed(delta));
        }
    }
    let mut random = 0xf975_ab3c_1345_7811_u64;
    for _ in 0..4096 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        addresses.push(random);
    }
    for mapped in [false, true] {
        let mut sim = if mapped {
            let rtl =
                struo_frontend_veryl::analyze_and_lower(&source, "pmp_end_test", "PmpAccessEnd")
                    .unwrap();
            let netlist = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&netlist)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source, "PmpAccessEnd")
                .build_native()
                .unwrap()
        };
        let a = sim.signal("address");
        let s = sim.signal("size");
        let last = sim.signal("last");
        let first = sim.signal("first_last");
        for &address in &addresses {
            for size in 0_u8..4 {
                sim.modify(|io| {
                    io.set(a, address);
                    io.set(s, size);
                })
                .unwrap();
                let physical = address & 0xffff_ffff;
                assert_eq!(
                    sim.get(last).to_u64_digits().first().copied().unwrap_or(0),
                    physical + (1_u64 << size) - 1,
                    "mapped={mapped} address={address:x} size={size}"
                );
                assert_eq!(
                    sim.get(first).to_u64_digits().first().copied().unwrap_or(0),
                    physical + 1,
                    "first parcel mapped={mapped} address={address:x} size={size}"
                );
            }
        }
    }
}

#[test]
fn fetch_access_end_selects_instruction_length_after_wrapping_and_carries() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/PmpCheck.veryl"))
            .unwrap();
    let mut addresses: Vec<u64> = (0..1024).collect();
    for bit in 1..64 {
        for delta in -8_i64..=8 {
            addresses.push((1_u64 << bit).wrapping_add_signed(delta));
        }
    }
    let mut random = 0x86cd_a7b5_3150_521d_u64;
    for _ in 0..4096 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        addresses.push(random);
    }
    for mapped in [false, true] {
        let mut sim = if mapped {
            let rtl = struo_frontend_veryl::analyze_and_lower(
                &source,
                "pmp_fetch_end_test",
                "PmpFetchAccessEnd",
            )
            .unwrap();
            let netlist = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&netlist)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source, "PmpFetchAccessEnd")
                .build_native()
                .unwrap()
        };
        let input = sim.signal("address");
        let length = sim.signal("full_instruction");
        let last = sim.signal("last");
        let first = sim.signal("first_last");
        for &address in &addresses {
            for full in [false, true] {
                sim.modify(|io| {
                    io.set(input, address);
                    io.set(length, u8::from(full));
                })
                .unwrap();
                let physical = address & 0xffff_ffff;
                assert_eq!(
                    sim.get(last).to_u64_digits().first().copied().unwrap_or(0),
                    physical + if full { 3 } else { 1 },
                    "mapped={mapped} address={address:x} full={full}"
                );
                assert_eq!(
                    sim.get(first).to_u64_digits().first().copied().unwrap_or(0),
                    physical + 1,
                    "first mapped={mapped} address={address:x}"
                );
            }
        }
    }
}

#[test]
fn offset_access_end_matches_wrapped_effective_address_then_byte_extent() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/PmpCheck.veryl"))
            .unwrap();
    let mut cases = Vec::new();
    for offset in 0_u16..4096 {
        for base in [
            0,
            1,
            0x7ff,
            0xfff,
            0x7fff_ffff,
            0xffff_ffff,
            0x1_0000_0000,
            0x1_0000_07ff,
            u64::MAX - 0x7ff,
            u64::MAX,
        ] {
            cases.push((base, offset));
        }
    }
    for bit in 1..64 {
        for delta in -8_i64..=8 {
            for offset in [0, 1, 2047, 2048, 4095] {
                cases.push(((1_u64 << bit).wrapping_add_signed(delta), offset));
            }
        }
    }
    let mut random = 0x81f7_2359_aca7_3421_u64;
    for _ in 0..4096 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        cases.push((random, ((random >> 32) & 4095) as u16));
    }
    for mapped in [false, true] {
        let mut sim = if mapped {
            let rtl = struo_frontend_veryl::analyze_and_lower(
                &source,
                "pmp_offset_end_test",
                "PmpOffsetAccessEnd",
            )
            .unwrap();
            let netlist = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&netlist)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source, "PmpOffsetAccessEnd")
                .build_native()
                .unwrap()
        };
        let base_signal = sim.signal("base");
        let offset_signal = sim.signal("offset");
        let size_signal = sim.signal("size");
        let last_signal = sim.signal("last");
        let high_signal = sim.signal("high");
        for &(base, offset) in &cases {
            let signed_offset = (i64::from(offset) << 52) >> 52;
            for size in 0_u8..4 {
                sim.modify(|io| {
                    io.set(base_signal, base);
                    io.set(offset_signal, offset);
                    io.set(size_signal, size);
                })
                .unwrap();
                let effective = base.wrapping_add_signed(signed_offset);
                assert_eq!(
                    sim.get(last_signal)
                        .to_u64_digits()
                        .first()
                        .copied()
                        .unwrap_or(0),
                    (effective & 0xffff_ffff) + (1_u64 << size) - 1,
                    "mapped={mapped} base={base:x} offset={signed_offset} size={size}",
                );
                assert_eq!(
                    sim.get(high_signal)
                        .to_u64_digits()
                        .first()
                        .copied()
                        .unwrap_or(0),
                    u64::from(effective >> 32 != 0),
                    "high mapped={mapped} base={base:x} offset={signed_offset} size={size}",
                );
            }
        }
    }
}

fn oracle(address: u64, size: u8, user: bool, access: u8, cfg: [u8; 4], addr: [u32; 4]) -> bool {
    for i in 0..4 {
        let (lo, hi) = match (cfg[i] >> 3) & 3 {
            0 => continue,
            1 => (
                if i == 0 {
                    0
                } else {
                    u64::from(addr[i - 1]) * 4
                },
                u64::from(addr[i]) * 4,
            ),
            2 => (u64::from(addr[i]) * 4, u64::from(addr[i]) * 4 + 4),
            _ => {
                let n = addr[i].trailing_ones().min(29) + 3;
                let length = (1u64 << n).min(1u64 << 32);
                let lo = (u64::from(addr[i]) * 4) & !(length - 1);
                (lo, lo + length)
            }
        };
        let bytes: Vec<_> = (0..(1u64 << size))
            .map(|b| address.wrapping_add(b))
            .collect();
        if bytes.iter().any(|&b| lo <= b && b < hi) {
            return bytes.iter().all(|&b| lo <= b && b < hi)
                && ((!user && cfg[i] & 128 == 0) || cfg[i] & access == access);
        }
    }
    !user
}
#[test]
fn pmp_matches_bytewise_oracle_for_priority_boundaries_modes_and_permissions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let source = fs::read_to_string(root.join("PmpCheck.veryl")).unwrap()
        + r#"
module PmpTest(address: input logic<64>, size: input logic<2>, user_access: input logic,
access_bits: input logic<3>, cfg: input logic<32>, a0: input logic<30>, a1: input logic<30>,
a2: input logic<30>, a3: input logic<30>, allowed: output logic, first: output logic,
data_allowed: output logic) {
var configs: logic<8> [4]; var addresses: logic<30> [4];
always_comb { for i in 0..4 { configs[i]=cfg[i*8+:8]; }
addresses[0]=a0; addresses[1]=a1; addresses[2]=a2; addresses[3]=a3; }
inst p: PmpCheck(address,size,user_access,read_access:access_bits[0],write_access:access_bits[1],
execute_access:access_bits[2],pmp_config:configs,pmp_address:addresses,allowed,first_parcel_allowed:first);
var lower: logic<33> [4]; var upper: logic<33> [4]; var unused_first: logic;
inst ranges: PmpRanges(pmp_config:configs,pmp_address:addresses,lower,upper);
inst data: PmpPermission #(CHECK_FIRST_PARCEL: 0) (address,size,user_access,read_access:access_bits[0],write_access:access_bits[1],
execute_access:access_bits[2],pmp_config:configs,lower,upper,allowed:data_allowed,first_parcel_allowed:unused_first);
}"#;
    for mapped in [false, true] {
        let mut sim = if mapped {
            let rtl =
                struo_frontend_veryl::analyze_and_lower(&source, "pmp_test", "PmpTest").unwrap();
            let netlist = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&netlist)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source, "PmpTest")
                .build_native()
                .unwrap()
        };
        let mut seed = 0x819273645u64;
        for case in 0..5000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let cfg = if case % 5 == 0 {
                [0; 4]
            } else {
                seed.to_le_bytes()[..4].try_into().unwrap()
            };
            let mut addr = [
                0x20000000 + ((seed >> 8) & 0x3f) as u32,
                0x20000020,
                0x2000001f,
                0x2000003f,
            ];
            if case % 11 == 0 {
                addr = [0x3fffffff, 0x3ffffffe, 0, 1];
            }
            let address = match case % 11 {
                7 => 0xffff_fffe,
                8 => 0xffff_ffff,
                9 => 0,
                10 => 3,
                0 => 0x8000007e,
                1 => 0x8000007f,
                2 => 0x180000000,
                3 => 0x7fffffff,
                _ => 0x80000000 + (seed & 255),
            };
            let size = (seed >> 20) as u8 & 3;
            let user = case % 2 == 0;
            let access = 1 + ((seed >> 24) % 7) as u8;
            let signals = [
                "address",
                "size",
                "user_access",
                "access_bits",
                "cfg",
                "a0",
                "a1",
                "a2",
                "a3",
            ]
            .map(|n| sim.signal(n));
            sim.modify(|io| {
                io.set(signals[0], address);
                io.set(signals[1], size);
                io.set(signals[2], u8::from(user));
                io.set(signals[3], access);
                io.set(signals[4], u32::from_le_bytes(cfg));
                for i in 0..4 {
                    io.set(signals[i + 5], addr[i]);
                }
            })
            .unwrap();
            let got = sim
                .get(sim.signal("allowed"))
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0)
                != 0;
            assert_eq!(
                got,
                oracle(address, size, user, access, cfg, addr),
                "case={case} mapped={mapped} address={address:x} size={size} cfg={cfg:x?}"
            );
            let data_allowed = sim
                .get(sim.signal("data_allowed"))
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0)
                != 0;
            assert_eq!(
                data_allowed, got,
                "disabled parcel check case={case} mapped={mapped}"
            );
            let first = sim
                .get(sim.signal("first"))
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0)
                != 0;
            assert_eq!(
                first,
                oracle(address, 1, user, 4, cfg, addr),
                "first parcel case={case} mapped={mapped} address={address:x} cfg={cfg:x?}"
            );
        }
    }
}

#[test]
fn pmp_csr_decoder_exposes_only_even_rv64_cfg_and_sixteen_addresses() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let source = fs::read_to_string(root.join("PipelineInterfaces.veryl")).unwrap()
        + &fs::read_to_string(root.join("Decoder.veryl")).unwrap()
        + r#"
module UserDecodeTest(instr: input logic<32>, illegal: output logic, index: output logic<5>) {
inst b: DecodeBus; inst d: Decoder #(ENABLE_U:1,ENABLE_INTERRUPTS:1)(decode:b);
always_comb {b.instr=instr;illegal=b.illegal;index=b.csr_index;}
}"#;
    let mut sim = SimulatorBuilder::new(&source, "UserDecodeTest")
        .build_native()
        .unwrap();
    for csr in 0x3a0..=0x3ffu32 {
        let instruction = csr << 20 | 2 << 12 | 1 << 7 | 0x73;
        let signal = sim.signal("instr");
        sim.modify(|io| io.set(signal, instruction)).unwrap();
        let illegal = sim
            .get(sim.signal("illegal"))
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
            != 0;
        assert_eq!(
            illegal,
            !(csr == 0x3a0 || csr == 0x3a2 || (0x3b0..=0x3bf).contains(&csr)),
            "csr={csr:x}"
        );
    }
}

#[test]
fn registered_ranges_keep_configuration_and_bounds_together_during_updates() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/PmpCheck.veryl"),
    )
    .unwrap()
        + r#"
module RegisteredPmpTest(clk: input clock, rst_n: input reset,
cfg: input logic<32>, a0: input logic<30>, a1: input logic<30>,
a2: input logic<30>, a3: input logic<30>, entry: input logic<2>,
config_out: output logic<8>, lo: output logic<33>, hi: output logic<33>, nonempty_out: output logic) {
var configs: logic<8> [4]; var addresses: logic<30> [4];
var decoded: logic<8> [4]; var lower: logic<33> [4]; var upper: logic<33> [4];
var nonempty: logic<4>;
always_comb { for i in 0..4 { configs[i]=cfg[i*8+:8]; }
addresses[0]=a0; addresses[1]=a1; addresses[2]=a2; addresses[3]=a3;
config_out=decoded[entry]; lo=lower[entry]; hi=upper[entry]; nonempty_out=nonempty[entry]; }
inst p: PmpRegisteredRanges(clk,rst_n,pmp_config:configs,pmp_address:addresses,
decoded_config:decoded,lower,upper,nonempty);
}"#;
    for mapped in [false, true] {
        let mut sim = if mapped {
            let rtl = struo_frontend_veryl::analyze_and_lower(
                &source,
                "pmp_pipeline",
                "RegisteredPmpTest",
            )
            .unwrap();
            let net = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&net)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source, "RegisteredPmpTest")
                .build_native()
                .unwrap()
        };
        let reset = sim.signal("rst_n");
        let clock = sim.event("clk");
        sim.modify(|io| io.set(reset, 0u8)).unwrap();
        sim.tick(clock).unwrap();
        sim.modify(|io| io.set(reset, 1u8)).unwrap();
        let mut previous: Option<([u8; 4], [u32; 4])> = None;
        let mut seed = 0x918273645u64;
        for cycle in 0..1000 {
            let resetting = cycle % 97 == 0;
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let cfg: [u8; 4] = seed.to_le_bytes()[..4].try_into().unwrap();
            let addresses = if cycle % 16 == 0 {
                [0; 4]
            } else if cycle % 16 == 1 {
                [0x3fffffff; 4]
            } else if cycle % 16 == 2 {
                [0, 1, 2, 3]
            } else if cycle % 16 == 3 {
                [3, 2, 1, 0]
            } else if cycle % 7 == 0 {
                [0x3fffffff, 0, 1, 0x3ffffffe]
            } else {
                [
                    seed as u32 & 0x3fffffff,
                    (seed >> 30) as u32 & 0x3fffffff,
                    0x2000001f,
                    0x20000000 + cycle,
                ]
            };
            let inputs = ["cfg", "a0", "a1", "a2", "a3"].map(|n| sim.signal(n));
            sim.modify(|io| {
                io.set(reset, u8::from(!resetting));
                io.set(inputs[0], u32::from_le_bytes(cfg));
                for i in 0..4 {
                    io.set(inputs[i + 1], addresses[i]);
                }
            })
            .unwrap();
            sim.tick(clock).unwrap();
            if resetting {
                for entry in 0..4_u8 {
                    let selector = sim.signal("entry");
                    sim.modify(|io| io.set(selector, entry)).unwrap();
                    for name in ["config_out", "lo", "hi", "nonempty_out"] {
                        assert_eq!(
                            sim.get(sim.signal(name))
                                .to_u64_digits()
                                .first()
                                .copied()
                                .unwrap_or(0),
                            0,
                            "reset mapped={mapped} cycle={cycle} entry={entry} {name}"
                        );
                    }
                }
                previous = None;
                continue;
            }
            if let Some((old_cfg, old_addr)) = previous {
                for i in 0..4 {
                    let selector = sim.signal("entry");
                    sim.modify(|io| io.set(selector, i as u8)).unwrap();
                    let (lo, hi) = match (old_cfg[i] >> 3) & 3 {
                        1 => (
                            if i == 0 {
                                0
                            } else {
                                u64::from(old_addr[i - 1]) * 4
                            },
                            u64::from(old_addr[i]) * 4,
                        ),
                        3 => {
                            let len = 1u64 << (old_addr[i].trailing_ones().min(29) + 3);
                            let base = u64::from(old_addr[i]) * 4 & !(len - 1);
                            (base, base + len)
                        }
                        _ => (u64::from(old_addr[i]) * 4, u64::from(old_addr[i]) * 4 + 4),
                    };
                    for (name, expected) in [
                        ("config_out", u64::from(old_cfg[i])),
                        ("lo", lo),
                        ("hi", hi),
                        ("nonempty_out", u64::from(lo < hi)),
                    ] {
                        let actual = sim
                            .get(sim.signal(name))
                            .to_u64_digits()
                            .first()
                            .copied()
                            .unwrap_or(0);
                        assert_eq!(
                            actual, expected,
                            "mapped={mapped} cycle={cycle} entry={i} {name}"
                        );
                    }
                }
            }
            previous = Some((cfg, addresses));
        }
    }
}

fn page_reference(page: u64, user: bool, cfg: [u8; 4], addr: [u32; 4]) -> (bool, [bool; 3]) {
    let base = page << 12;
    let end = base + 4096;
    for i in 0..4 {
        let (lo, hi) = match (cfg[i] >> 3) & 3 {
            0 => continue,
            1 => (
                if i == 0 {
                    0
                } else {
                    u64::from(addr[i - 1]) * 4
                },
                u64::from(addr[i]) * 4,
            ),
            2 => (u64::from(addr[i]) * 4, u64::from(addr[i]) * 4 + 4),
            _ => {
                let length = 1u64 << (addr[i].trailing_ones().min(29) + 3);
                let lo = (u64::from(addr[i]) * 4) & !(length - 1);
                (lo, lo + length)
            }
        };
        if lo < hi && lo < end && base < hi {
            let permission = |kind: u8| (!user && cfg[i] & 128 == 0) || cfg[i] & (1 << kind) != 0;
            return (
                lo <= base && end <= hi,
                [permission(0), permission(1), permission(2)],
            );
        }
    }
    (true, [!user; 3])
}

#[test]
fn page_uniform_classification_matches_bytewise_oracle_inside_each_page() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let source = fs::read_to_string(root.join("PmpCheck.veryl")).unwrap()
        + r#"
module PageTest(page: input logic<32>, user_access: input logic, cfg: input logic<32>,
a0: input logic<30>, a1: input logic<30>, a2: input logic<30>, a3: input logic<30>,
uniform: output logic, read: output logic, write: output logic, execute: output logic) {
var configs: logic<8> [4]; var addresses: logic<30> [4];
var lower: logic<33> [4]; var upper: logic<33> [4]; var nonempty: logic<4>;
always_comb { for i in 0..4 { configs[i]=cfg[i*8+:8]; nonempty[i]=lower[i] <: upper[i]; }
addresses[0]=a0; addresses[1]=a1; addresses[2]=a2; addresses[3]=a3; }
inst ranges: PmpRanges(pmp_config:configs,pmp_address:addresses,lower,upper);
inst check: PmpPageUniform(page:page[19:0],user_access,pmp_config:configs,lower,upper,nonempty,
uniform,read,write,execute);
}"#;
    let mut checked_accesses = 0_u64;
    let mut uniform_pages = [0_u64; 2];
    for mapped in [false, true] {
        let mut sim = if mapped {
            let rtl = struo_frontend_veryl::analyze_and_lower(&source, "pmp_page_test", "PageTest")
                .unwrap();
            let netlist = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&netlist)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source, "PageTest")
                .build_native()
                .unwrap()
        };
        let inputs = ["page", "user_access", "cfg", "a0", "a1", "a2", "a3"].map(|n| sim.signal(n));
        let outputs = ["uniform", "read", "write", "execute"].map(|n| sim.signal(n));
        let mut seed = 0x5eed_7a6e_c0de_1234_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for case in 0..6000 {
            let random = next();
            let cfg: [u8; 4] = if case % 7 == 0 {
                [0; 4]
            } else {
                random.to_le_bytes()[..4].try_into().unwrap()
            };
            // Mix page-aligned bounds, sub-page bounds and NAPOT sizes around a
            // few neighboring pages so that both uniform and split pages occur.
            let mut addr = [0_u32; 4];
            for (i, value) in addr.iter_mut().enumerate() {
                let bits = next();
                let page_index = 0x80000 + (bits & 7) as u32;
                *value = match (bits >> 3) % 4 {
                    0 => page_index << 10,
                    1 => (page_index << 10) + ((bits >> 8) & 0x3ff) as u32,
                    2 => {
                        ((page_index << 10) & !((1 << ((bits >> 8) % 14)) - 1))
                            | ((1 << ((bits >> 8) % 14)) - 1) >> 1
                    }
                    _ => ((page_index + 1) << 10) - 1 - (i as u32),
                } & 0x3fff_ffff;
            }
            if case % 13 == 0 {
                addr = [0x3fff_ffff, 0x3fff_fffe, 0, 1];
            }
            let page = match case % 9 {
                0 => 0,
                1 => 0xf_ffff,
                _ => 0x80000 + (next() % 10) - 1,
            };
            let user = case % 2 == 0;
            sim.modify(|io| {
                io.set(inputs[0], page as u32);
                io.set(inputs[1], u8::from(user));
                io.set(inputs[2], u32::from_le_bytes(cfg));
                for i in 0..4 {
                    io.set(inputs[i + 3], addr[i]);
                }
            })
            .unwrap();
            let got = outputs.map(|signal| {
                sim.get(signal)
                    .to_u64_digits()
                    .first()
                    .copied()
                    .unwrap_or(0)
                    != 0
            });
            let (uniform, permissions) = page_reference(page, user, cfg, addr);
            let context = format!(
                "case={case} mapped={mapped} page={page:x} user={user} cfg={cfg:x?} addr={addr:x?}"
            );
            assert_eq!(got[0], uniform, "uniform {context}");
            if !uniform {
                continue;
            }
            uniform_pages[usize::from(mapped)] += 1;
            assert_eq!(&got[1..], &permissions, "permissions {context}");
            // The property the core relies on: every contained access in a
            // uniform page has exactly the cached permission of its kind.
            for _ in 0..8 {
                let bits = next();
                let size = (bits & 3) as u8;
                let offset = (bits >> 2) % (4096 - (1 << size) + 1);
                let address = (page << 12) + offset;
                for access in [1_u8, 2, 3, 4] {
                    let expected = (access & 1 == 0 || permissions[0])
                        && (access & 2 == 0 || permissions[1])
                        && (access & 4 == 0 || permissions[2]);
                    assert_eq!(
                        oracle(address, size, user, access, cfg, addr),
                        expected,
                        "contained access {address:x}/{size}/{access} {context}"
                    );
                    checked_accesses += 1;
                }
            }
        }
    }
    // Both classes must actually occur for the generated configurations.
    // About an eighth of the generated pages are split by a PMP boundary.
    assert!(
        uniform_pages[0] > 1000 && uniform_pages[0] < 5500,
        "{uniform_pages:?}"
    );
    assert_eq!(uniform_pages[0], uniform_pages[1]);
    assert!(checked_accesses > 50_000, "{checked_accesses}");
}

#[test]
fn cached_page_match_equals_effective_address_page_containment() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/PmpCheck.veryl"))
            .unwrap()
            + r#"
module PageMatchTest(base: input logic<64>, offset: input logic<16>, size: input logic<2>,
tag: input logic<32>, hit: output logic) {
inst check: PmpCachedPageMatch(base, offset: offset[11:0], size, tag: tag[19:0], hit);
}"#;
    for mapped in [false, true] {
        let mut sim = if mapped {
            let rtl = struo_frontend_veryl::analyze_and_lower(
                &source,
                "pmp_page_match_test",
                "PageMatchTest",
            )
            .unwrap();
            let netlist = struo_target_ecp5::map_to_ecp5_with_options(
                &struo_synth::synthesize(&rtl).unwrap().netlist,
                Default::default(),
            )
            .unwrap();
            struo_celox::ecp5_simulator(&netlist)
                .unwrap()
                .build_native()
                .unwrap()
        } else {
            SimulatorBuilder::new(&source, "PageMatchTest")
                .build_native()
                .unwrap()
        };
        let inputs = ["base", "offset", "size", "tag"].map(|n| sim.signal(n));
        let hit = sim.signal("hit");
        let mut seed = 0x0dd5_1a7e_9e11_b007_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut hits = 0;
        for case in 0..200_000 {
            let bits = next();
            let tag = match case % 17 {
                0 => 0,
                1 => 0xf_ffff,
                _ => (next() & 0xf_ffff) as u32,
            };
            // Bases cluster around the cached page, its neighbors and the
            // 2^32 boundary so carries, borrows and wraps all occur.
            let page = match bits % 6 {
                0 => u64::from(tag),
                1 => u64::from(tag).wrapping_sub(1),
                2 => u64::from(tag) + 1,
                3 => next() & 0xf_ffff,
                4 => 0xf_ffff,
                _ => 0,
            };
            let upper = match (bits >> 3) % 8 {
                0 => 1_u64,
                1 => 0xffff_ffff,
                2 => next() >> 32,
                _ => 0,
            };
            let low = match (bits >> 6) % 4 {
                0 => (next() & 7) as u64,
                1 => 0xff8 + (next() & 7),
                _ => next() & 0xfff,
            };
            let base = (upper << 32) | ((page & 0xf_ffff) << 12) | low;
            let offset = ((bits >> 9) & 0xfff) as u16;
            let size = ((bits >> 21) & 3) as u8;
            sim.modify(|io| {
                io.set(inputs[0], base);
                io.set(inputs[1], offset);
                io.set(inputs[2], size);
                io.set(inputs[3], tag);
            })
            .unwrap();
            let got = sim.get(hit).to_u64_digits().first().copied().unwrap_or(0) != 0;
            let effective = base.wrapping_add_signed(i64::from(((offset << 4) as i16) >> 4));
            let expected = base >> 32 == 0
                && base >> 12 == u64::from(tag)
                && effective >> 12 == u64::from(tag)
                && (effective & 0xfff) + (1 << size) - 1 < 4096;
            assert_eq!(
                got, expected,
                "mapped={mapped} base={base:x} offset={offset:x} size={size} tag={tag:x}"
            );
            hits += usize::from(got);
        }
        assert!(hits > 10_000, "{hits}");
    }
}
