use std::fs;
use std::path::PathBuf;

use celox::{NativeBackend, Simulator, SimulatorBuilder};

const TEST_TOP: &str = r#"
module CsrDecoderTestTop (
    instr    : input  logic<32>,
    csr_index: output logic<4> ,
    csr_write: output logic    ,
    illegal  : output logic    ,
) {
    inst decode_bus: DecodeBus;
    inst decoder: Decoder (
        decode: decode_bus,
    );

    always_comb {
        decode_bus.instr = instr;
        csr_index        = decode_bus.csr_index;
        csr_write        = decode_bus.csr_write;
        illegal          = decode_bus.illegal;
    }
}
"#;

fn csr_instruction(address: u16, funct3: u8, source: u8, destination: u8) -> u32 {
    (u32::from(address) << 20)
        | (u32::from(source) << 15)
        | (u32::from(funct3) << 12)
        | (u32::from(destination) << 7)
        | 0x73
}

fn value(simulator: &mut Simulator<NativeBackend>, name: &str) -> u64 {
    simulator
        .get(simulator.signal(name))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

fn decoder() -> Simulator<NativeBackend> {
    let riscv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let source = [
        fs::read_to_string(riscv.join("src/PipelineInterfaces.veryl")).unwrap(),
        fs::read_to_string(riscv.join("src/Decoder.veryl")).unwrap(),
        TEST_TOP.to_owned(),
    ]
    .join("\n");
    SimulatorBuilder::new(&source, "CsrDecoderTestTop")
        .build_native()
        .unwrap()
}

fn decode(simulator: &mut Simulator<NativeBackend>, instruction: u32) -> (u8, bool, bool) {
    let instr = simulator.signal("instr");
    simulator.modify(|io| io.set(instr, instruction)).unwrap();
    (
        value(simulator, "csr_index") as u8,
        value(simulator, "csr_write") != 0,
        value(simulator, "illegal") != 0,
    )
}

#[test]
fn counter_aliases_share_storage_but_only_machine_addresses_are_writable() {
    let mut simulator = decoder();
    for (address, index, writable) in [
        (0xb00, 12, true),  // mcycle
        (0xc00, 12, false), // cycle
        (0xb02, 13, true),  // minstret
        (0xc02, 13, false), // instret
        (0x7c0, 14, true),  // mstallsel
        (0x7c1, 15, true),  // mstalldata
        (0x301, 2, false),  // misa
        (0xf14, 11, false), // mhartid
    ] {
        // CSRRS with x0 is a pure read and is legal for every implemented CSR.
        let (read_index, read_write, read_illegal) =
            decode(&mut simulator, csr_instruction(address, 0b010, 0, 1));
        assert_eq!(
            read_index, index,
            "wrong compact index for CSR {address:#05x}"
        );
        assert!(
            !read_write,
            "pure read marked as a write for CSR {address:#05x}"
        );
        assert!(!read_illegal, "pure read rejected for CSR {address:#05x}");

        // CSRRW writes even with rd=x0, and legality follows the architectural
        // address rather than the shared four-bit storage index.
        let (write_index, write, write_illegal) =
            decode(&mut simulator, csr_instruction(address, 0b001, 1, 0));
        assert_eq!(
            write_index, index,
            "wrong write index for CSR {address:#05x}"
        );
        assert!(write, "CSRRW not marked as a write for CSR {address:#05x}");
        assert_eq!(
            write_illegal, !writable,
            "wrong write legality for CSR {address:#05x}"
        );
    }

    let (index, write, illegal) = decode(&mut simulator, csr_instruction(0x7ff, 0b010, 0, 1));
    assert_eq!(index, 0);
    assert!(!write);
    assert!(illegal, "unsupported custom CSR was accepted");
}
