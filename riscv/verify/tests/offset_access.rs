use celox::SimulatorBuilder;
use std::{fs, path::Path};

fn expected(address: u64, size: u8, timer: bool) -> (bool, Option<u64>) {
    let bytes = 1_u64 << size;
    for range in [0..0x1000, 0x8000_0000..0x8003_f000] {
        if range.contains(&address) {
            for i in 1..bytes {
                let byte = address.checked_add(i).unwrap();
                if !range.contains(&byte) {
                    return (false, Some(byte));
                }
            }
            return (true, None);
        }
    }
    (
        timer && (0x0200_0000..0x0201_0000).contains(&address) && address.is_multiple_of(bytes),
        None,
    )
}

#[test]
fn native_and_mapped_offset_permissions_match_bytewise_effective_addresses() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/PhysicalOffsetAccessCheck.veryl"),
    )
    .unwrap();
    let mut bases = Vec::new();
    for boundary in [
        0_u64,
        0x1000,
        0x0200_0000,
        0x0201_0000,
        0x8000_0000,
        0x8003_f000,
    ] {
        for delta in [-4096_i64, -2048, -17, -8, -1, 0, 1, 7, 16, 2047, 4095] {
            let base = boundary.wrapping_add_signed(delta);
            bases.push(base);
            for bit in 32..64 {
                bases.push(base ^ (1 << bit));
            }
        }
    }
    for timer in [false, true] {
        let source = source.replace(
            "param ENABLE_TIMER: bit = 0",
            &format!("param ENABLE_TIMER: bit = {}", u8::from(timer)),
        );
        let native = SimulatorBuilder::new(&source, "PhysicalOffsetAccessCheck")
            .build_native()
            .unwrap();
        let rtl = struo_frontend_veryl::analyze_and_lower(
            &source,
            "offset_test",
            "PhysicalOffsetAccessCheck",
        )
        .unwrap();
        let mapped = struo_target_ecp5::map_to_ecp5_with_options(
            &struo_synth::synthesize(&rtl).unwrap().netlist,
            struo_target_ecp5::MappingOptions {
                timing_goal_mhz: 175,
                ..Default::default()
            },
        )
        .unwrap();
        let mapped = struo_celox::ecp5_simulator(&mapped)
            .unwrap()
            .build_native()
            .unwrap();
        for mut sim in [native, mapped] {
            let a = sim.signal("base");
            let o = sim.signal("offset");
            let s = sim.signal("size");
            let out = sim.signal("allowed");
            let boot = sim.signal("boot_boundary");
            let ram = sim.signal("ram_boundary");
            let boot_region = sim.signal("boot_region");
            let timer_region = sim.signal("timer_region");
            let sparse = bases.iter().flat_map(|&base| {
                [-2048_i64, -17, -8, -1, 0, 1, 7, 16, 2047]
                    .into_iter()
                    .map(move |offset| (base, offset))
            });
            let dense = [
                0_u64,
                0xfff,
                0x7fff_ffff,
                0x8003_efff,
                0x0200_ffff,
                u64::MAX,
            ]
            .into_iter()
            .flat_map(|base| (-2048_i64..2048).map(move |offset| (base, offset)));
            for (base, offset) in sparse.chain(dense) {
                for size in 0_u8..4 {
                    sim.modify(|io| {
                        io.set(a, base);
                        io.set(o, (offset as u16) & 0xfff);
                        io.set(s, size);
                    })
                    .unwrap();
                    let actual = sim.get(out).to_u64_digits().first().copied().unwrap_or(0) != 0;
                    let address = base.wrapping_add_signed(offset);
                    let expected = expected(address, size, timer);
                    let boot_boundary =
                        sim.get(boot).to_u64_digits().first().copied().unwrap_or(0) != 0;
                    let ram_boundary =
                        sim.get(ram).to_u64_digits().first().copied().unwrap_or(0) != 0;
                    assert_eq!(
                        (actual, boot_boundary, ram_boundary),
                        (
                            expected.0,
                            expected.1 == Some(0x1000),
                            expected.1 == Some(0x8003_f000)
                        ),
                        "base={base:016x} offset={offset} size={size} timer={timer}"
                    );
                    assert_eq!(
                        (
                            sim.get(boot_region)
                                .to_u64_digits()
                                .first()
                                .copied()
                                .unwrap_or(0)
                                != 0,
                            sim.get(timer_region)
                                .to_u64_digits()
                                .first()
                                .copied()
                                .unwrap_or(0)
                                != 0,
                        ),
                        (
                            address < 0x1000,
                            timer && (0x0200_0000..0x0201_0000).contains(&address),
                        ),
                        "region: base={base:016x} offset={offset} size={size} timer={timer}"
                    );
                }
            }
        }
    }
}

#[test]
fn expanded_ram_offsets_match_effective_address_oracle() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/PhysicalOffsetAccessCheck.veryl"),
    )
    .unwrap();
    let end = 0x8800_0000_u64;
    let mut sim = SimulatorBuilder::new(&source, "PhysicalOffsetAccessCheck")
        .param("RAM_END", end)
        .build_native()
        .unwrap();
    let a = sim.signal("base");
    let o = sim.signal("offset");
    let s = sim.signal("size");
    let allowed = sim.signal("allowed");
    let boundary = sim.signal("ram_boundary");
    for base in [
        0x7fff_ffff_u64,
        0x8000_0000,
        0x8003_ffff,
        0x8020_0000,
        end - 4096,
        end - 1,
        end,
        end + 4095,
    ] {
        for upper in [0, 1_u64 << 32, 0xffff_ffff_0000_0000] {
            let base = base | upper;
            for offset in -2048_i64..2048 {
                let address = base.wrapping_add_signed(offset);
                for size in 0..4_u8 {
                    sim.modify(|io| {
                        io.set(a, base);
                        io.set(o, (offset as u16) & 4095);
                        io.set(s, size);
                    })
                    .unwrap();
                    let fits =
                        (0..1_u64 << size).all(|n| (0x8000_0000..end).contains(&(address + n)));
                    assert_eq!(
                        sim.get_as::<u8>(allowed) != 0,
                        fits,
                        "base={base:#x} offset={offset} size={size}"
                    );
                    assert_eq!(
                        sim.get_as::<u8>(boundary) != 0,
                        (0x8000_0000..end).contains(&address) && !fits
                    );
                }
            }
        }
    }
}
