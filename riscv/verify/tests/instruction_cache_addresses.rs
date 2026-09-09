use celox::{NativeBackend, Simulator, SimulatorBuilder};
use std::{fs, path::Path};

fn get(sim: &mut Simulator<NativeBackend>, name: &str) -> u32 {
    sim.get(sim.signal(name))
        .to_u32_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

#[test]
fn native_and_mapped_successor_lines_match_wrapping_address_arithmetic() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/InstructionCache.veryl"),
    )
    .unwrap();
    let mut native = SimulatorBuilder::new(&source, "InstructionCacheAddresses")
        .build_native()
        .unwrap();
    let rtl = struo_frontend_veryl::analyze_and_lower(
        &source,
        "cache_address_test",
        "InstructionCacheAddresses",
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
    let mut mapped = struo_celox::ecp5_simulator(&mapped)
        .unwrap()
        .build_native()
        .unwrap();
    // Include every low 16-bit PC, every upper carry boundary and wraparound.
    let mut addresses = (0_u32..=0xffff).collect::<Vec<_>>();
    for bit in 16..32 {
        for offset in 0..32 {
            addresses.push((1_u32 << bit).wrapping_sub(offset));
            addresses.push((1_u32 << bit).wrapping_add(offset));
        }
    }
    addresses.extend((0..32).map(|offset| u32::MAX - offset));
    let mut random = 0xac3d_28b7_u32;
    for _ in 0..4096 {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        addresses.push(random);
    }
    for pc in addresses {
        for sim in [&mut native, &mut mapped] {
            let signal = sim.signal("pc");
            sim.modify(|io| io.set(signal, pc)).unwrap();
            let line = pc & !7;
            assert_eq!(get(sim, "pc_line"), line, "pc={pc:08x}");
            assert_eq!(get(sim, "next_line"), line.wrapping_add(8), "pc={pc:08x}");
            assert_eq!(get(sim, "ahead_line"), line.wrapping_add(16), "pc={pc:08x}");
        }
    }
}
