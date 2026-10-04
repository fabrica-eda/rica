use celox::{NativeBackend, Simulator, SimulatorBuilder};
use std::{fs, path::Path};

fn value(sim: &mut Simulator<NativeBackend>, name: &str) -> u64 {
    sim.get(sim.signal(name))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

// Deliberately byte-wise: the RTL uses region decoders and lane comparisons,
// whereas this oracle checks membership of every requested byte.
fn expected(address: u64, size: u8, instruction: bool, timer: bool) -> (bool, bool, u64) {
    for region in [0..0x1000, 0x8000_0000..0x8003_f000] {
        if region.contains(&address) {
            for offset in 0..(1 << size) {
                let byte = address + offset;
                if !region.contains(&byte) {
                    return (false, true, byte);
                }
            }
            return (true, false, address);
        }
    }
    let mmio = timer
        && !instruction
        && (0x0200_0000..0x0201_0000).contains(&address)
        && address.is_multiple_of(1 << size);
    (mmio, false, address)
}

#[test]
fn native_and_mapped_access_checks_cover_boundaries_aliases_and_every_width() {
    let original = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/PhysicalAccessCheck.veryl"),
    )
    .unwrap();
    for timer in [false, true] {
        let source = original.replace(
            "param ENABLE_TIMER: bit = 0",
            &format!("param ENABLE_TIMER: bit = {}", u8::from(timer)),
        );
        let native = SimulatorBuilder::new(&source, "PhysicalAccessCheck")
            .build_native()
            .unwrap();
        let rtl =
            struo_frontend_veryl::analyze_and_lower(&source, "access_test", "PhysicalAccessCheck")
                .unwrap();
        let netlist = struo_target_ecp5::map_to_ecp5_with_options(
            &struo_synth::synthesize(&rtl).unwrap().netlist,
            struo_target_ecp5::MappingOptions {
                timing_goal_mhz: 175,
                ..Default::default()
            },
        )
        .unwrap();
        let mapped = struo_celox::ecp5_simulator(&netlist)
            .unwrap()
            .build_native()
            .unwrap();
        let mut addresses = Vec::new();
        for boundary in [
            0_u64,
            0x1000,
            0x0200_0000,
            0x0201_0000,
            0x8000_0000,
            0x8003_f000,
        ] {
            for delta in -16_i64..16 {
                let low = boundary.wrapping_add_signed(delta);
                addresses.push(low);
                for bit in 32..64 {
                    addresses.push(low ^ (1 << bit));
                }
            }
        }
        for bit in 0..64 {
            addresses.push(1 << bit);
            addresses.push(!(1 << bit));
        }
        for mut sim in [native, mapped] {
            let a = sim.signal("address");
            let s = sim.signal("size");
            let i = sim.signal("instruction");
            for &address in &addresses {
                for size in 0..4_u8 {
                    for instruction in [false, true] {
                        sim.modify(|io| {
                            io.set(a, address);
                            io.set(s, size);
                            io.set(i, u8::from(instruction));
                        })
                        .unwrap();
                        assert_eq!(
                            (
                                value(&mut sim, "allowed") != 0,
                                value(&mut sim, "crosses_bound") != 0,
                                value(&mut sim, "fault_address"),
                            ),
                            expected(address, size, instruction, timer),
                            "address {address:016x} size {size} instruction {instruction} timer {timer}",
                        );
                    }
                }
            }
        }
    }
}
