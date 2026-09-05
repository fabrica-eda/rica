use std::fs;
use std::path::PathBuf;

use celox::{NativeBackend, Simulator, SimulatorBuilder};

fn i_type(imm: i32, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    ((imm as u32 & 0x0fff) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | (u32::from(rd) << 7)
        | u32::from(opcode)
}

fn r_type(funct7: u8, rs2: u8, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    (u32::from(funct7) << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | (u32::from(rd) << 7)
        | u32::from(opcode)
}

fn s_type(imm: i32, rs2: u8, rs1: u8, funct3: u8) -> u32 {
    let imm = imm as u32 & 0x0fff;
    ((imm >> 5) << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | ((imm & 0x1f) << 7)
        | 0x23
}

fn b_type(imm: i32, rs2: u8, rs1: u8, funct3: u8) -> u32 {
    let imm = imm as u32 & 0x1fff;
    (((imm >> 12) & 1) << 31)
        | (((imm >> 5) & 0x3f) << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (u32::from(funct3) << 12)
        | (((imm >> 1) & 0x0f) << 8)
        | (((imm >> 11) & 1) << 7)
        | 0x63
}

fn j_type(imm: i32, rd: u8) -> u32 {
    let imm = imm as u32 & 0x001f_ffff;
    (((imm >> 20) & 1) << 31)
        | (((imm >> 1) & 0x03ff) << 21)
        | (((imm >> 11) & 1) << 20)
        | (((imm >> 12) & 0xff) << 12)
        | (u32::from(rd) << 7)
        | 0x6f
}

fn u_type(imm20: i32, rd: u8) -> u32 {
    ((imm20 as u32 & 0x000f_ffff) << 12) | (u32::from(rd) << 7) | 0x37
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
    let source = fs::read_to_string(riscv.join("src/CompressedDecoder.veryl")).unwrap();
    SimulatorBuilder::new(&source, "CompressedDecoder")
        .build_native()
        .unwrap()
}

fn decode(simulator: &mut Simulator<NativeBackend>, raw: u16) -> (u32, bool) {
    let compressed = simulator.signal("compressed");
    simulator.modify(|io| io.set(compressed, raw)).unwrap();
    (
        value(simulator, "instruction") as u32,
        value(simulator, "illegal") != 0,
    )
}

#[test]
fn representative_rv64c_encodings_expand_to_canonical_rv64i() {
    let mut simulator = decoder();
    let cases = [
        // Quadrant 0: all compact-base load/store families and the scrambled
        // ADDI4SPN immediate, using offsets that exercise every source field.
        (0x1fe4, i_type(1020, 2, 0, 9, 0x13)), // C.ADDI4SPN s1, sp, 1020
        (0x40e8, i_type(68, 9, 2, 10, 0x03)),  // C.LW a0, 68(s1)
        (0x656c, i_type(200, 10, 3, 11, 0x03)), // C.LD a1, 200(a0)
        (0xd5d0, s_type(44, 12, 11, 2)),       // C.SW a2, 44(a3)
        (0xfff8, s_type(248, 14, 15, 3)),      // C.SD a4, 248(a5)
        // Quadrant 1: CI/CB/CA arithmetic and PC-relative control flow.
        (0x0001, i_type(0, 0, 0, 0, 0x13)),          // C.NOP
        (0x1401, i_type(-32, 8, 0, 8, 0x13)),        // C.ADDI s0, -32
        (0x34fd, i_type(-1, 9, 0, 9, 0x1b)),         // C.ADDIW s1, -1
        (0x553d, i_type(-17, 0, 0, 10, 0x13)),       // C.LI a0, -17
        (0x7901, u_type(-32, 18)),                   // C.LUI s2, -32
        (0x7101, i_type(-512, 2, 0, 2, 0x13)),       // C.ADDI16SP sp, -512
        (0x907d, i_type(63, 8, 5, 8, 0x13)),         // C.SRLI s0, 63
        (0x9485, i_type(0x421, 9, 5, 9, 0x13)),      // C.SRAI s1, 33
        (0x993d, i_type(-17, 10, 7, 10, 0x13)),      // C.ANDI a0, -17
        (0x8c05, r_type(0x20, 9, 8, 0, 8, 0x33)),    // C.SUB s0, s1
        (0x8d2d, r_type(0, 11, 10, 4, 10, 0x33)),    // C.XOR a0, a1
        (0x8e55, r_type(0, 13, 12, 6, 12, 0x33)),    // C.OR a2, a3
        (0x8f7d, r_type(0, 15, 14, 7, 14, 0x33)),    // C.AND a4, a5
        (0x9d0d, r_type(0x20, 11, 10, 0, 10, 0x3b)), // C.SUBW a0, a1
        (0x9c25, r_type(0, 9, 8, 0, 8, 0x3b)),       // C.ADDW s0, s1
        (0xdffd, b_type(-2, 0, 15, 0)),              // C.BEQZ a5, -2
        (0xfffd, b_type(-2, 0, 15, 1)),              // C.BNEZ a5, -2
        (0xbffd, j_type(-2, 0)),                     // C.J -2
        // Quadrant 2: full register fields and stack-relative LSU.
        (0x197e, i_type(63, 18, 1, 18, 0x13)), // C.SLLI s2, 63
        (0x557e, i_type(252, 2, 2, 10, 0x03)), // C.LWSP a0, 252(sp)
        (0x75fe, i_type(504, 2, 3, 11, 0x03)), // C.LDSP a1, 504(sp)
        (0x894e, r_type(0, 19, 0, 0, 18, 0x33)), // C.MV s2, s3
        (0x9a56, r_type(0, 21, 20, 0, 20, 0x33)), // C.ADD s4, s5
        (0x8482, i_type(0, 9, 0, 0, 0x67)),    // C.JR s1
        (0x9482, i_type(0, 9, 0, 1, 0x67)),    // C.JALR s1
        (0x9002, 0x0010_0073),                 // C.EBREAK
        (0xdfda, s_type(252, 22, 2, 2)),       // C.SWSP s6, 252(sp)
        (0xffde, s_type(504, 23, 2, 3)),       // C.SDSP s7, 504(sp)
    ];
    for (raw, expected) in cases {
        let (instruction, illegal) = decode(&mut simulator, raw);
        assert!(!illegal, "0x{raw:04x} was rejected");
        assert_eq!(instruction, expected, "wrong expansion for 0x{raw:04x}");
    }
}

#[test]
fn rv64c_hint_code_points_remain_legal_no_ops() {
    let mut simulator = decoder();
    let cases = [
        (0x0005, i_type(1, 0, 0, 0, 0x13)),     // C.NOP imm!=0
        (0x0401, i_type(0, 8, 0, 8, 0x13)),     // C.ADDI rd!=x0, imm=0
        (0x507d, i_type(-1, 0, 0, 0, 0x13)),    // C.LI x0, -1
        (0x6005, u_type(1, 0)),                 // C.LUI x0, 1
        (0x801a, r_type(0, 6, 0, 0, 0, 0x33)),  // C.MV x0, x6
        (0x901a, r_type(0, 6, 0, 0, 0, 0x33)),  // C.ADD x0, x6
        (0x0002, i_type(0, 0, 1, 0, 0x13)),     // C.SLLI x0, 0
        (0x8001, i_type(0, 8, 5, 8, 0x13)),     // C.SRLI s0, 0
        (0x8401, i_type(0x400, 8, 5, 8, 0x13)), // C.SRAI s0, 0
    ];
    for (raw, expected) in cases {
        let (instruction, illegal) = decode(&mut simulator, raw);
        assert!(!illegal, "HINT 0x{raw:04x} was rejected");
        assert_eq!(
            instruction, expected,
            "wrong HINT expansion for 0x{raw:04x}"
        );
    }
}

#[test]
fn all_compressed_code_points_are_deterministic_and_canonical() {
    let mut simulator = decoder();
    let mut legal = 0;
    let mut digest = 0xcbf29ce484222325_u64;
    for raw in 0_u16..=u16::MAX {
        let (instruction, illegal) = decode(&mut simulator, raw);
        for byte in instruction
            .to_le_bytes()
            .into_iter()
            .chain([u8::from(illegal)])
        {
            digest = (digest ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        if illegal {
            assert_eq!(
                instruction, 0x0000_0013,
                "illegal 0x{raw:04x} did not produce the deterministic NOP value"
            );
        } else {
            legal += 1;
            assert_ne!(raw & 3, 3, "non-compressed 0x{raw:04x} was accepted");
            assert_eq!(
                instruction & 3,
                3,
                "0x{raw:04x} expanded to a noncanonical instruction"
            );
        }
    }
    // This count fixes the intended RV64C/Zca integer subset, including HINT
    // encodings and excluding floating-point and reserved code points.
    assert_eq!(legal, 38_551);
    // FNV-1a of every little-endian expansion and illegal byte, measured on
    // the pre-timing-repair decoder. This also checks every immediate/register
    // bit, which the legality count and canonical opcode checks cannot cover.
    assert_eq!(digest, 0x1c2f7a0300eecc23);
}

#[test]
fn reserved_and_noninteger_encodings_are_rejected() {
    let mut simulator = decoder();
    for raw in [
        0x0000, // C.ADDI4SPN nzuimm=0 / permanent all-zero illegal
        0x2000, // C.FLD (floating point is not in the integer-only subset)
        0xa000, // C.FSD
        0x2001, // C.ADDIW x0
        0x6101, // C.ADDI16SP immediate=0
        0x6181, // C.LUI immediate=0
        0x9c41, // Reserved RV64 compact-ALU funct2
        0x2002, // C.FLDSP
        0x4002, // C.LWSP x0
        0x6002, // C.LDSP x0
        0x8002, // C.JR x0
        0xa002, // C.FSDSP
        0x0003, // Quadrant 3 is not a compressed instruction
        0xffff, // Permanently illegal all-ones variable-length prefix
    ] {
        let (_, illegal) = decode(&mut simulator, raw);
        assert!(illegal, "reserved encoding 0x{raw:04x} was accepted");
    }
}
