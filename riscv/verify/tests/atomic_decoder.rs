use celox::SimulatorBuilder;
use std::{fs, path::Path};

#[test]
fn a_decoder_checks_every_funct5_width_rs2_and_ordering_encoding() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let source = [
        fs::read_to_string(root.join("PipelineInterfaces.veryl")).unwrap(),
        fs::read_to_string(root.join("Decoder.veryl")).unwrap(),
        r#"module AtomicDecodeTest #(param ENABLE_A: bit = 0) (
            instr: input logic<32>, flags: output logic<8>, immediate: output logic<64>,
        ) {
            inst d: DecodeBus;
            inst decoder: Decoder #(ENABLE_A: ENABLE_A) (decode: d);
            always_comb {
                d.instr = instr;
                flags = {d.illegal, d.is_atomic, d.use_rs1, d.use_rs2, d.write_enable,
                         d.use_immediate, d.is_load, d.is_store};
                immediate = d.immediate;
            }
        }"#
        .to_owned(),
    ]
    .join("\n");
    for enabled in [false, true] {
        let mut sim = SimulatorBuilder::new(&source, "AtomicDecodeTest")
            .param("ENABLE_A", u64::from(enabled))
            .build_native()
            .unwrap();
        let instr = sim.signal("instr");
        let flags = sim.signal("flags");
        let immediate = sim.signal("immediate");
        for op in 0..32_u32 {
            for width in 0..8_u32 {
                for rs2 in 0..32_u32 {
                    for order in 0..4_u32 {
                        let word = op << 27
                            | order << 25
                            | rs2 << 20
                            | 1 << 15
                            | width << 12
                            | 3 << 7
                            | 0x2f;
                        sim.modify(|io| io.set(instr, word)).unwrap();
                        let valid = enabled
                            && matches!(width, 2 | 3)
                            && (matches!(op, 0 | 1 | 3 | 4 | 8 | 12 | 16 | 20 | 24 | 28)
                                || (op == 2 && rs2 == 0));
                        let f = sim.get(flags).to_u64_digits().first().copied().unwrap_or(0);
                        assert_eq!(
                            f & 0xc0,
                            if valid { 0x40 } else { 0x80 },
                            "encoding={word:08x} A={enabled}"
                        );
                        if valid {
                            assert_eq!(f, if op == 2 { 0x6e } else { 0x7f });
                            assert_eq!(
                                sim.get(immediate)
                                    .to_u64_digits()
                                    .first()
                                    .copied()
                                    .unwrap_or(0),
                                0
                            );
                        }
                    }
                }
            }
        }
    }
}
