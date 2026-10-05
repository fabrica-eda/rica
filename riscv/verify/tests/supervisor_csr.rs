//! Supervisor CSRs, delegation, SRET and interrupt routing in `CsrFile`,
//! checked natively and after ECP5 mapping.
use celox::{NativeBackend, Simulator, SimulatorBuilder};
use std::{fs, path::Path};

const WIDE: [&str; 4] = ["write_source", "trap_pc", "trap_cause", "trap_value"];

struct Csr(Simulator<NativeBackend>);

impl Csr {
    fn set(&mut self, name: &str, value: u64) {
        let signal = self.0.signal(name);
        self.0
            .modify(|io| {
                if WIDE.contains(&name) {
                    io.set(signal, value);
                } else {
                    io.set(signal, value as u8);
                }
            })
            .unwrap();
    }
    fn get(&mut self, name: &str) -> u64 {
        self.0
            .get(self.0.signal(name))
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }
    fn tick(&mut self) {
        let clock = self.0.event("clk");
        self.0.tick(clock).unwrap();
    }
    fn write(&mut self, index: u64, value: u64) {
        self.set("write_index", index);
        self.set("write_source", value);
        self.set("write_enable", 1);
        self.tick();
        self.set("write_enable", 0);
    }
    fn read(&mut self, index: u64) -> u64 {
        self.set("index", index);
        self.get("read_data")
    }
    fn pulse(&mut self, name: &str) {
        self.set(name, 1);
        self.tick();
        self.set(name, 0);
    }
    /// Presents a trap and returns the vector chosen before the edge.
    fn trap(&mut self, cause: u64, pc: u64, value: u64) -> u64 {
        self.set("trap_cause", cause);
        self.set("trap_pc", pc);
        self.set("trap_value", value);
        let vector = self.get("trap_vector");
        self.pulse("trap_enter");
        vector
    }
    /// Privilege as 0 = U, 1 = S, 3 = M.
    fn privilege(&mut self) -> u64 {
        match (self.get("below_machine"), self.get("supervisor_mode")) {
            (0, 0) => 3,
            (1, 1) => 1,
            (1, 0) => 0,
            other => panic!("inconsistent privilege outputs {other:?}"),
        }
    }
    /// MRET into the mode encoded as MPP.
    fn enter(&mut self, mpp: u64) {
        let status = self.read(MSTATUS) & !(3 << 11);
        self.write(MSTATUS, status | mpp << 11);
        self.pulse("mret");
        assert_eq!(self.privilege(), mpp);
    }
}

const MSTATUS: u64 = 1;
const MISA: u64 = 2;
const MTVEC: u64 = 3;
const MEPC: u64 = 5;
const MCAUSE: u64 = 6;
const MIE: u64 = 16;
const MIP: u64 = 17;
const MCOUNTEREN: u64 = 25;
const SSTATUS: u64 = 32;
const SIE: u64 = 33;
const STVEC: u64 = 34;
const SCOUNTEREN: u64 = 35;
const SENVCFG: u64 = 36;
const SSCRATCH: u64 = 37;
const SEPC: u64 = 38;
const SCAUSE: u64 = 39;
const STVAL: u64 = 40;
const SIP: u64 = 41;
const SATP: u64 = 42;
const MEDELEG: u64 = 43;
const MIDELEG: u64 = 44;
const INTERRUPT: u64 = 1 << 63;

fn simulators() -> Vec<Csr> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let source = [
        "PipelineInterfaces.veryl",
        "PerformanceCounter64.veryl",
        "PerformanceEventCounters4.veryl",
        "CsrFile.veryl",
    ]
    .map(|name| fs::read_to_string(root.join(name)).unwrap())
    .join("\n")
        + include_str!("fixtures/supervisor_csr.veryl");
    let native = SimulatorBuilder::new(&source, "SupervisorCsrTest")
        .build_native()
        .unwrap();
    let rtl =
        struo_frontend_veryl::analyze_and_lower(&source, "supervisorcsr", "SupervisorCsrTest")
            .unwrap();
    let netlist = struo_target_ecp5::map_to_ecp5_with_options(
        &struo_synth::synthesize(&rtl).unwrap().netlist,
        Default::default(),
    )
    .unwrap();
    let mapped = struo_celox::ecp5_simulator(&netlist)
        .unwrap()
        .build_native()
        .unwrap();
    [native, mapped]
        .into_iter()
        .map(|simulator| {
            let mut csr = Csr(simulator);
            for name in [
                "rst_n",
                "index",
                "write_index",
                "write_enable",
                "write_source",
                "trap_enter",
                "trap_pc",
                "trap_cause",
                "trap_value",
                "mret",
                "sret",
                "machine_interrupts",
            ] {
                csr.set(name, 0);
            }
            csr.tick();
            csr.set("rst_n", 1);
            csr
        })
        .collect()
}

#[test]
fn delegated_traps_sret_and_status_views() {
    for mut csr in simulators() {
        assert_eq!(csr.read(MISA) & (1 << 18 | 1 << 20), 1 << 18 | 1 << 20);
        assert_eq!(csr.privilege(), 3);
        // SXL and UXL are 64-bit; SSTATUS shows UXL but not SXL.
        assert_eq!(csr.read(MSTATUS) >> 32 & 0xf, 0xa);
        assert_eq!(csr.read(SSTATUS) >> 32 & 0xf, 0x2);

        // MPP is WARL over M, S and U.
        csr.write(MSTATUS, 2 << 11);
        assert_eq!(csr.read(MSTATUS) >> 11 & 3, 0);
        csr.write(MSTATUS, 1 << 11);
        assert_eq!(csr.read(MSTATUS) >> 11 & 3, 1);

        csr.write(MEDELEG, u64::MAX);
        assert_eq!(csr.read(MEDELEG), 0xb3ff);
        csr.write(MEDELEG, 1 << 2 | 1 << 8);
        csr.write(MTVEC, 0x8000_0100);
        csr.write(STVEC, 0x8000_0203);
        assert_eq!(csr.read(STVEC), 0x8000_0200);
        csr.write(SSCRATCH, 0x1234_5678_9abc_def0);
        assert_eq!(csr.read(SSCRATCH), 0x1234_5678_9abc_def0);
        csr.write(SATP, 8 << 60 | 0x1234);
        assert_eq!(csr.read(SATP), 0, "satp is Bare only");
        csr.write(SENVCFG, u64::MAX);
        assert_eq!(csr.read(SENVCFG), 0);

        // A delegated cause in M stays in M.
        assert_eq!(csr.trap(2, 0x8000_0010, 0x11), 0x8000_0100);
        assert_eq!(csr.privilege(), 3);
        assert_eq!((csr.read(MEPC), csr.read(MCAUSE)), (0x8000_0010, 2));

        // MRET to S with SIE set, then a delegated illegal instruction.
        csr.write(SSTATUS, 1 << 1);
        csr.enter(1);
        assert_eq!(csr.trap(2, 0x8000_0020, 0x22), 0x8000_0200);
        assert_eq!(csr.privilege(), 1);
        assert_eq!(
            (csr.read(SEPC), csr.read(SCAUSE), csr.read(STVAL)),
            (0x8000_0020, 2, 0x22)
        );
        assert_eq!((csr.read(MEPC), csr.read(MCAUSE)), (0x8000_0010, 2));
        // SPP = S, SPIE = old SIE, SIE = 0.
        assert_eq!(csr.read(SSTATUS) & 0x122, 0x120);

        // SRET returns to S through SEPC and restores SIE.
        assert_eq!(csr.get("supervisor_return_pc"), 0x8000_0020);
        csr.write(SEPC, 0x8000_0031);
        assert_eq!(csr.get("supervisor_return_pc"), 0x8000_0030);
        csr.pulse("sret");
        assert_eq!(csr.privilege(), 1);
        assert_eq!(csr.read(SSTATUS) & 0x122, 0x22);

        // A non-delegated breakpoint from S goes to M with MPP = S.
        assert_eq!(csr.trap(3, 0x8000_0040, 0), 0x8000_0100);
        assert_eq!(csr.privilege(), 3);
        assert_eq!(csr.read(MSTATUS) >> 11 & 3, 1);
        csr.pulse("mret");
        assert_eq!(csr.privilege(), 1);

        // SRET with SPP = U enters U and clears MPRV.
        let status = csr.read(MSTATUS);
        csr.write(MSTATUS, status | 1 << 17);
        let status = csr.read(SSTATUS);
        csr.write(SSTATUS, status & !(1 << 8));
        csr.pulse("sret");
        assert_eq!(csr.privilege(), 0);
        assert_eq!(csr.read(MSTATUS) & 1 << 17, 0);

        // ECALL from U is delegated; ECALL from M (11) never is.
        assert_eq!(csr.trap(8, 0x8000_0050, 0), 0x8000_0200);
        assert_eq!(csr.privilege(), 1);
        assert_eq!(csr.read(SSTATUS) & 1 << 8, 0, "SPP = U");
        assert_eq!(csr.read(SCAUSE), 8);
        csr.write(MEDELEG, 1 << 11);
        assert_eq!(csr.read(MEDELEG), 0);
    }
}

#[test]
fn supervisor_interrupts_follow_delegation_and_privilege() {
    for mut csr in simulators() {
        csr.write(MTVEC, 0x8000_0100);
        csr.write(STVEC, 0x8000_0200);
        csr.write(MIDELEG, u64::MAX);
        assert_eq!(csr.read(MIDELEG), 0x222);
        csr.write(MIE, 0x222);
        assert_eq!(csr.read(SIE), 0x222);
        // M-mode software owns all three supervisor pending bits.
        csr.write(MIP, 1 << 5);
        assert_eq!(csr.read(SIP), 1 << 5);
        // Delegated interrupts are never taken in M.
        csr.write(MSTATUS, 1 << 3);
        assert_eq!(csr.get("interrupt_pending"), 0);

        csr.enter(1);
        assert_eq!(csr.get("interrupt_pending"), 0, "SIE is clear");
        csr.write(SSTATUS, 1 << 1);
        assert_eq!(csr.get("interrupt_pending"), 1);
        assert_eq!(csr.get("interrupt_cause"), 5);
        assert_eq!(csr.trap(INTERRUPT | 5, 0x8000_0060, 0), 0x8000_0200);
        assert_eq!(csr.read(SCAUSE), INTERRUPT | 5);
        assert_eq!(csr.privilege(), 1);
        assert_eq!(csr.get("interrupt_pending"), 0, "trap entry clears SIE");

        // S may clear a delegated SSIP but cannot touch STIP through SIP.
        csr.write(SIP, 1 << 1);
        assert_eq!(csr.read(SIP), 1 << 1 | 1 << 5);
        csr.write(SIP, 0);
        assert_eq!(csr.read(SIP), 1 << 5);

        // In U every delegated pending interrupt is taken, regardless of SIE.
        csr.write(SSTATUS, 0);
        csr.pulse("sret");
        assert_eq!(csr.privilege(), 0);
        assert_eq!(csr.get("interrupt_pending"), 1);

        // Undelegate STI: it goes to M from U even with MIE clear; SEI
        // pending in M takes priority as a machine-destined interrupt.
        csr.write(MIDELEG, 1 << 1 | 1 << 9);
        assert_eq!(csr.read(SIE), 1 << 1 | 1 << 9);
        csr.write(MSTATUS, 0);
        assert_eq!(csr.get("interrupt_pending"), 1);
        assert_eq!(csr.get("interrupt_cause"), 5);
        assert_eq!(csr.trap(INTERRUPT | 5, 0x8000_0070, 0), 0x8000_0100);
        assert_eq!(csr.read(MCAUSE), INTERRUPT | 5);
        assert_eq!(csr.privilege(), 3);
        // Machine-level interrupts still precede the supervisor ones.
        csr.write(MIE, 0x222 | 1 << 11);
        csr.set("machine_interrupts", 4);
        csr.write(MSTATUS, 1 << 3);
        assert_eq!(csr.get("interrupt_cause"), 11);
    }
}

#[test]
fn counter_access_and_trap_controls() {
    for mut csr in simulators() {
        csr.write(MCOUNTEREN, 0b101);
        csr.write(SCOUNTEREN, 0b001);
        assert_eq!(csr.read(SCOUNTEREN), 0b001);
        csr.enter(1);
        assert_eq!(csr.get("counter_enable"), 0b101);
        csr.write(SSTATUS, 0);
        csr.pulse("sret");
        assert_eq!(csr.privilege(), 0);
        assert_eq!(csr.get("counter_enable"), 0b001);
        csr.trap(2, 0, 0);
        assert_eq!(csr.privilege(), 3);

        for (bit, name) in [
            (20, "trap_virtual_memory"),
            (21, "timeout_wait"),
            (22, "trap_sret"),
        ] {
            csr.write(MSTATUS, 1 << bit);
            assert_eq!(csr.get(name), 1);
            assert_eq!(csr.read(SSTATUS) & 1 << bit, 0, "{name} is M-only");
            csr.write(MSTATUS, 0);
            assert_eq!(csr.get(name), 0);
        }
        // MXR is shared by both views.
        csr.write(SSTATUS, 1 << 19);
        assert_eq!(csr.read(MSTATUS) & 1 << 19, 1 << 19);
        // MPRV with MPP = S applies PMP to data in M.
        csr.write(MSTATUS, 1 << 17 | 1 << 11);
        assert_eq!(csr.get("data_below_machine"), 1);
        csr.write(MSTATUS, 1 << 17 | 3 << 11);
        assert_eq!(csr.get("data_below_machine"), 0);
    }
}
