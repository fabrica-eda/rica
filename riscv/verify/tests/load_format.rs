use celox::SimulatorBuilder;
use std::{fs, path::Path};

#[test]
fn native_and_mapped_load_extension_match_integer_casts() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/LoadFormat.veryl"))
            .unwrap();
    let native = SimulatorBuilder::new(&source, "LoadFormat")
        .build_native()
        .unwrap();
    let rtl = struo_frontend_veryl::analyze_and_lower(&source, "load_test", "LoadFormat").unwrap();
    let mapped = struo_target_ecp5::map_to_ecp5_with_options(
        &struo_synth::synthesize(&rtl).unwrap().netlist,
        struo_target_ecp5::MappingOptions {
            timing_goal_mhz: 175,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(mapped.retiming().equivalence_signed_off);
    let mapped = struo_celox::ecp5_simulator(&mapped)
        .unwrap()
        .build_native()
        .unwrap();
    let mut values = vec![
        0,
        u64::MAX,
        0x80,
        0x8000,
        0x8000_0000,
        0x7f,
        0x7fff,
        0x7fff_ffff,
    ];
    for bit in 0..64 {
        values.push(1 << bit);
        values.push(!(1 << bit));
    }
    let mut random = 0x8293_cdea_7401_6b25_u64;
    for _ in 0..2048 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        values.push(random);
    }
    for mut sim in [native, mapped] {
        let data = sim.signal("value");
        let size = sim.signal("size");
        let unsigned = sim.signal("unsigned_load");
        let result = sim.signal("formatted");
        for &value in &values {
            let expected = [
                [value as u8 as i8 as i64 as u64, value as u8 as u64],
                [value as u16 as i16 as i64 as u64, value as u16 as u64],
                [value as u32 as i32 as i64 as u64, value as u32 as u64],
                [value, value],
            ];
            for (width, signedness) in expected.into_iter().enumerate() {
                for (zero_extend, expected) in signedness.into_iter().enumerate() {
                    sim.modify(|io| {
                        io.set(data, value);
                        io.set(size, width as u8);
                        io.set(unsigned, zero_extend as u8);
                    })
                    .unwrap();
                    let actual = sim
                        .get(result)
                        .to_u64_digits()
                        .first()
                        .copied()
                        .unwrap_or(0);
                    assert_eq!(
                        actual, expected,
                        "value={value:016x} size={width} unsigned={zero_extend}"
                    );
                }
            }
        }
    }
    eprintln!(
        "{} native/mapped load-extension cases",
        values.len() * 8 * 2
    );
}
