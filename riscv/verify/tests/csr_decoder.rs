use std::fs;
use std::path::{Path, PathBuf};

use celox::{NativeBackend, Simulator, SimulatorBuilder};

const TEST_TOP: &str = r#"
module CsrDecoderTestTop #(
    param ENABLE_M: bit = 0,
    param ENABLE_INTERRUPTS: bit = 0,
) (
    instr    : input  logic<32>,
    csr_index: output logic<6> ,
    csr_write: output logic    ,
    illegal  : output logic    ,
) {
    inst decode_bus: DecodeBus;
    inst decoder: Decoder #(ENABLE_M: ENABLE_M, ENABLE_INTERRUPTS: ENABLE_INTERRUPTS) (
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
    decoder_with_m(false)
}

fn decoder_with_m(enable_m: bool) -> Simulator<NativeBackend> {
    decoder_with_options(enable_m, false)
}

fn decoder_with_options(enable_m: bool, interrupts: bool) -> Simulator<NativeBackend> {
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
        .param("ENABLE_M", u64::from(enable_m))
        .param("ENABLE_INTERRUPTS", u64::from(interrupts))
        .build_native()
        .unwrap()
}

#[test]
fn m_encodings_and_reserved_word_operations_have_exact_legality() {
    for enabled in [false, true] {
        let mut simulator = decoder_with_m(enabled);
        for opcode in [0x33, 0x3b] {
            for funct3 in 0..8 {
                let instruction = 1 << 25 | 2 << 20 | 1 << 15 | funct3 << 12 | 3 << 7 | opcode;
                let (_, _, illegal) = decode(&mut simulator, instruction);
                let valid_m = enabled && (opcode == 0x33 || funct3 == 0 || funct3 >= 4);
                assert_eq!(
                    illegal, !valid_m,
                    "enabled={enabled}, instruction={instruction:08x}"
                );
            }
        }
    }
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
fn every_implemented_csr_has_exact_write_legality_for_all_six_forms() {
    let mut simulator = decoder();
    const IMPLEMENTED: [(u16, u8, bool); 17] = [
        (0x300, 1, true),   // mstatus
        (0x301, 2, true),   // misa: fixed WARL, writes are ignored
        (0x305, 3, true),   // mtvec
        (0x340, 4, true),   // mscratch
        (0x341, 5, true),   // mepc
        (0x342, 6, true),   // mcause
        (0x343, 7, true),   // mtval
        (0xf11, 8, false),  // mvendorid
        (0xf12, 9, false),  // marchid
        (0xf13, 10, false), // mimpid
        (0xf14, 11, false), // mhartid
        (0xb00, 12, true),  // mcycle
        (0xc00, 12, false), // cycle: read-only alias of mcycle
        (0xb02, 13, true),  // minstret
        (0xc02, 13, false), // instret: read-only alias of minstret
        (0x7c0, 14, true),  // mstallsel
        (0x7c1, 15, true),  // mstalldata
    ];
    const CSR_FORMS: [u8; 6] = [0b001, 0b010, 0b011, 0b101, 0b110, 0b111];

    for (address, expected_index, writable) in IMPLEMENTED {
        for funct3 in CSR_FORMS {
            for source in [0_u8, 0x1f] {
                let write_requested = matches!(funct3, 0b001 | 0b101) || source != 0;
                let (index, csr_write, illegal) =
                    decode(&mut simulator, csr_instruction(address, funct3, source, 1));
                let context =
                    format!("CSR {address:#05x}, funct3={funct3:03b}, source={source:#04x}");

                assert_eq!(index, expected_index, "wrong compact index for {context}");
                assert_eq!(
                    csr_write, write_requested,
                    "wrong csr_write indication for {context}"
                );
                assert_eq!(
                    illegal,
                    write_requested && !writable,
                    "wrong legality for {context}"
                );
            }
        }
    }
}

#[test]
fn unknown_csrs_are_illegal_for_reads_and_writes_without_aliasing_an_index() {
    let mut simulator = decoder();
    const CSR_FORMS: [u8; 6] = [0b001, 0b010, 0b011, 0b101, 0b110, 0b111];

    for address in [0x000_u16, 0x302, 0x344, 0x7ff, 0xb01, 0xc01, 0xfff] {
        for funct3 in CSR_FORMS {
            for source in [0_u8, 0x1f] {
                let write_requested = matches!(funct3, 0b001 | 0b101) || source != 0;
                let (index, csr_write, illegal) =
                    decode(&mut simulator, csr_instruction(address, funct3, source, 1));
                let context = format!(
                    "unknown CSR {address:#05x}, funct3={funct3:03b}, source={source:#04x}"
                );

                assert_eq!(
                    index, 0,
                    "unknown address aliased compact storage for {context}"
                );
                assert_eq!(
                    csr_write, write_requested,
                    "wrong csr_write indication for {context}"
                );
                assert!(illegal, "unsupported address was accepted for {context}");
            }
        }
    }
}

#[test]
fn wfi_is_a_legal_m_mode_nop_without_accepting_unimplemented_privileged_encodings() {
    let mut sim = decoder();
    let (_, write, illegal) = decode(&mut sim, 0x1050_0073);
    assert!(!write && !illegal);
    for instr in [
        0x1020_0073,
        0x1200_0073,
        0x1040_0073,
        0x1060_0073,
        0x1050_00f3,
        0x1050_8073,
    ] {
        assert!(decode(&mut sim, instr).2, "instruction={instr:08x}");
    }
}

#[test]
fn machine_interrupt_csrs_decode_all_forms_only_when_enabled() {
    for enabled in [false, true] {
        let mut simulator = decoder_with_options(false, enabled);
        for (address, expected_index) in [(0x304, 16), (0x344, 17)] {
            for funct3 in [1, 2, 3, 5, 6, 7] {
                for source in [0, 31] {
                    let (index, write, illegal) =
                        decode(&mut simulator, csr_instruction(address, funct3, source, 1));
                    assert_eq!(index, if enabled { expected_index } else { 0 });
                    assert_eq!(illegal, !enabled);
                    // MIP pending bits are read-only, but its CSR address is
                    // writable: all six instructions are legal when enabled.
                    assert_eq!(write, matches!(funct3, 1 | 5) || source != 0);
                }
            }
        }
    }
}

#[test]
fn csr_decode_recognizes_exact_address_set_and_write_permissions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let source = [
        fs::read_to_string(root.join("PipelineInterfaces.veryl")).unwrap(),
        fs::read_to_string(root.join("Decoder.veryl")).unwrap(),
        r#"module CsrDecodeTest #(param U: bit = 0, param IRQ: bit = 0) (
            instr: input logic<32>, illegal: output logic,
            prefetched_illegal: output logic,
            csr_index: output logic<6>, prefetched_csr_index: output logic<6>,
        ) {
            inst d: DecodeBus;
            inst decoder: Decoder #(ENABLE_U: U, ENABLE_INTERRUPTS: IRQ) (decode: d);
            inst pre: DecodeBus;
            var known: logic;
            var writable: logic;
            inst classify: CsrAddressClass #(ENABLE_U: U, ENABLE_INTERRUPTS: IRQ) (
                address: instr[31:20], known, writable,
            );
            inst prefetched: DecoderWithCsrClass #(USE_CSR_CLASS: 1, ENABLE_U: U, ENABLE_INTERRUPTS: IRQ) (
                decode: pre, csr_known: known, csr_writable: writable,
            );
            assign d.instr = instr;
            assign pre.instr = instr;
            assign illegal = d.illegal;
            assign prefetched_illegal = pre.illegal;
            assign csr_index = d.csr_index;
            assign prefetched_csr_index = pre.csr_index;
        }"#
        .to_owned(),
    ]
    .join("\n");
    for user in [false, true] {
        for irq in [false, true] {
            let mut sim = SimulatorBuilder::new(&source, "CsrDecodeTest")
                .param("U", u64::from(user))
                .param("IRQ", u64::from(irq))
                .build_native()
                .unwrap();
            let instr = sim.signal("instr");
            let illegal = sim.signal("illegal");
            let prefetched_illegal = sim.signal("prefetched_illegal");
            let csr_index = sim.signal("csr_index");
            let prefetched_csr_index = sim.signal("prefetched_csr_index");
            for address in 0..4096_u32 {
                let known = matches!(address,
                    0x300 | 0x301 | 0x305 | 0x340..=0x343 | 0xf11..=0xf14
                    | 0xb00 | 0xb02 | 0xc00 | 0xc02 | 0x7c0 | 0x7c1)
                    || (irq && matches!(address, 0x304 | 0x344))
                    || (user && matches!(address, 0x306 | 0x30a | 0x3a0 | 0x3a2 | 0x3b0..=0x3bf));
                for funct3 in [1_u32, 2, 3, 5, 6, 7] {
                    for source in [0_u32, 1] {
                        let word = address << 20 | source << 15 | funct3 << 12 | 0x73;
                        sim.modify(|io| io.set(instr, word)).unwrap();
                        let writes = funct3 & 3 == 1 || source != 0;
                        let expected = !known || (writes && address >> 10 == 3);
                        let actual = sim
                            .get(illegal)
                            .to_u64_digits()
                            .first()
                            .copied()
                            .unwrap_or(0)
                            != 0;
                        assert_eq!(
                            actual, expected,
                            "U={user} IRQ={irq} instruction={word:08x}"
                        );
                        assert_eq!(sim.get(prefetched_illegal), sim.get(illegal));
                        assert_eq!(sim.get(prefetched_csr_index), sim.get(csr_index));
                    }
                }
            }
        }
    }
}

const SUPERVISOR_TOP: &str = r#"
module SupervisorDecoderTestTop #(
    param ENABLE_S: bit = 0,
) (
    instr    : input  logic<32>,
    csr_index: output logic<6> ,
    is_sret  : output logic    ,
    is_mret  : output logic    ,
    illegal  : output logic    ,
) {
    inst decode_bus: DecodeBus;
    inst decoder: Decoder #(ENABLE_U: 1, ENABLE_S: ENABLE_S, ENABLE_INTERRUPTS: 1) (
        decode: decode_bus,
    );

    always_comb {
        decode_bus.instr = instr;
        csr_index        = decode_bus.csr_index;
        is_sret          = decode_bus.is_sret;
        is_mret          = decode_bus.is_mret;
        illegal          = decode_bus.illegal;
    }
}
"#;

#[test]
fn supervisor_csrs_sret_and_sfence_decode_only_with_s() {
    let riscv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let source = [
        fs::read_to_string(riscv.join("src/PipelineInterfaces.veryl")).unwrap(),
        fs::read_to_string(riscv.join("src/Decoder.veryl")).unwrap(),
        SUPERVISOR_TOP.to_owned(),
    ]
    .join("\n");
    const SUPERVISOR: [(u16, u8); 13] = [
        (0x100, 32), // sstatus
        (0x104, 33), // sie
        (0x105, 34), // stvec
        (0x106, 35), // scounteren
        (0x10a, 36), // senvcfg
        (0x140, 37), // sscratch
        (0x141, 38), // sepc
        (0x142, 39), // scause
        (0x143, 40), // stval
        (0x144, 41), // sip
        (0x180, 42), // satp
        (0x302, 43), // medeleg
        (0x303, 44), // mideleg
    ];
    for enabled in [false, true] {
        let mut simulator = SimulatorBuilder::new(&source, "SupervisorDecoderTestTop")
            .param("ENABLE_S", u64::from(enabled))
            .build_native()
            .unwrap();
        let mut run = |instruction: u32| {
            let instr = simulator.signal("instr");
            simulator.modify(|io| io.set(instr, instruction)).unwrap();
            (
                value(&mut simulator, "csr_index") as u8,
                value(&mut simulator, "is_sret") != 0,
                value(&mut simulator, "is_mret") != 0,
                value(&mut simulator, "illegal") != 0,
            )
        };
        for (address, index) in SUPERVISOR {
            for funct3 in [1, 2, 3, 5, 6, 7] {
                let (decoded, _, _, illegal) = run(csr_instruction(address, funct3, 1, 2));
                assert_eq!(illegal, !enabled, "{address:03x} funct3={funct3}");
                if enabled {
                    assert_eq!(decoded, index, "{address:03x}");
                } else {
                    assert!(decoded < 32, "{address:03x} decoded {decoded}");
                }
            }
        }
        // Neighboring unimplemented supervisor addresses remain illegal.
        for address in [0x101, 0x107, 0x145, 0x181, 0x304 + 0x100] {
            assert!(run(csr_instruction(address, 2, 0, 1)).3, "{address:03x}");
        }
        let (_, sret, mret, illegal) = run(0x1020_0073);
        assert_eq!((sret, mret, illegal), (enabled, false, !enabled));
        // CSR indices are meaningful only for CSR instructions.
        let (_, sret, mret, illegal) = run(0x3020_0073);
        assert_eq!((sret, mret, illegal), (false, true, false));
        // SFENCE.VMA rs1, rs2 with rd = 0; other rd or funct7 values are reserved.
        for (rs1, rs2) in [(0, 0), (5, 0), (0, 7), (31, 31)] {
            let sfence = 0x1200_0073 | rs2 << 20 | rs1 << 15;
            assert_eq!(run(sfence).3, !enabled, "sfence.vma x{rs1}, x{rs2}");
            assert!(run(sfence | 1 << 7).3, "sfence.vma with rd");
            assert!(run(sfence ^ 1 << 25).3, "funct7 0b0001000");
        }
    }
}
