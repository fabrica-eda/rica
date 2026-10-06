//! Sv39 page-table walker against an independent reference walk, natively
//! and after ECP5 mapping.
use celox::{NativeBackend, Simulator, SimulatorBuilder};
use std::collections::{BTreeMap, BTreeSet};
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Translated { page: u32, permissions: u8 },
    PageFault,
    AccessFault,
}

#[derive(Default)]
struct Memory {
    ptes: BTreeMap<u32, u64>,
    denied: BTreeSet<u32>,
}

/// Reference Sv39 walk (privileged ISA 4.3.2 with Svade, 32-bit physical).
fn reference(memory: &Memory, root: u32, vpn: u32) -> Outcome {
    let mut table = u64::from(root);
    for level in (0..3).rev() {
        let index = u64::from(vpn >> (9 * level)) & 0x1ff;
        let address = table * 4096 + index * 8;
        if address >= 1 << 32 || memory.denied.contains(&(address as u32)) {
            return Outcome::AccessFault;
        }
        let pte = memory.ptes.get(&(address as u32)).copied().unwrap_or(0);
        let (v, r, w, x) = (pte & 1, pte >> 1 & 1, pte >> 2 & 1, pte >> 3 & 1);
        if v == 0 || (r == 0 && w == 1) || pte >> 54 != 0 {
            return Outcome::PageFault;
        }
        let ppn = pte >> 10 & ((1 << 44) - 1);
        if r == 1 || x == 1 {
            let low_mask = (1u64 << (9 * level)) - 1;
            if ppn & low_mask != 0 || pte >> 6 & 1 == 0 {
                return Outcome::PageFault;
            }
            let page = (ppn & !low_mask) | (u64::from(vpn) & low_mask);
            if page >= 1 << 20 {
                return Outcome::AccessFault;
            }
            let permissions = (pte >> 7 & 1) << 4 | (pte >> 4 & 1) << 3 | x << 2 | w << 1 | r;
            return Outcome::Translated {
                page: page as u32,
                permissions: permissions as u8,
            };
        }
        if level == 0 {
            return Outcome::PageFault;
        }
        table = ppn;
    }
    unreachable!()
}

struct Walker(Simulator<NativeBackend>);

const WIDE: [&str; 3] = ["virtual_page", "root", "response_data"];

/// 64-bit wide ports, since the simulator rejects values wider than a port.
const TEST_TOP: &str = r#"
module WalkerTest (
    clk: input clock, rst_n: input reset,
    start: input logic, virtual_page: input logic<64>, root: input logic<64>,
    request_denied: input logic, response_valid: input logic, response_data: input logic<64>,
    request_address: output logic<64>, request_issue: output logic, busy: output logic,
    done: output logic, page_fault: output logic, access_fault: output logic,
    physical_page: output logic<64>, permissions: output logic<64>,
) {
    var address: logic<32>;
    var page: logic<20>;
    var flags: logic<5>;
    inst walker: PageTableWalker (
        clk, rst_n, start, virtual_page: virtual_page[26:0], root: root[19:0],
        request_address: address, request_denied, request_issue, response_valid,
        response_data, busy, done, page_fault, access_fault, physical_page: page,
        permissions: flags,
    );
    assign request_address = {32'd0, address};
    assign physical_page = {44'd0, page};
    assign permissions = {59'd0, flags};
}
"#;

impl Walker {
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
    fn walk(&mut self, memory: &Memory, root: u32, vpn: u32, latency: u32) -> Outcome {
        self.set("virtual_page", u64::from(vpn));
        self.set("root", u64::from(root));
        self.set("start", 1);
        self.tick();
        self.set("start", 0);
        let mut countdown = None::<u32>;
        for _ in 0..200 {
            let address = self.get("request_address") as u32;
            self.set("request_denied", u64::from(memory.denied.contains(&address)));
            if self.get("done") == 1 {
                return if self.get("page_fault") == 1 {
                    Outcome::PageFault
                } else if self.get("access_fault") == 1 {
                    Outcome::AccessFault
                } else {
                    Outcome::Translated {
                        page: self.get("physical_page") as u32,
                        permissions: self.get("permissions") as u8,
                    }
                };
            }
            let issue = self.get("request_issue") == 1;
            let ready = countdown == Some(0);
            self.set("response_valid", u64::from(ready));
            if ready {
                let data = memory.ptes.get(&address).copied().unwrap_or(0);
                self.set("response_data", data);
                countdown = None;
            }
            self.tick();
            self.set("response_valid", 0);
            countdown = match (issue, countdown) {
                (true, _) => Some(latency),
                (false, Some(n)) => Some(n.saturating_sub(1)),
                (false, None) => None,
            };
        }
        panic!("walk did not finish");
    }
}

fn walkers() -> Vec<(&'static str, Walker)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let source = fs::read_to_string(root.join("PageTableWalker.veryl")).unwrap() + TEST_TOP;
    let native = SimulatorBuilder::new(&source, "WalkerTest")
        .build_native()
        .unwrap();
    let rtl = struo_frontend_veryl::analyze_and_lower(&source, "walker", "WalkerTest").unwrap();
    let netlist = struo_target_ecp5::map_to_ecp5_with_options(
        &struo_synth::synthesize(&rtl).unwrap().netlist,
        Default::default(),
    )
    .unwrap();
    let mapped = struo_celox::ecp5_simulator(&netlist)
        .unwrap()
        .build_native()
        .unwrap();
    [("native", native), ("mapped", mapped)]
        .into_iter()
        .map(|(name, simulator)| {
            let mut walker = Walker(simulator);
            for signal in [
                "rst_n",
                "start",
                "virtual_page",
                "root",
                "request_denied",
                "response_valid",
                "response_data",
            ] {
                walker.set(signal, 0);
            }
            walker.tick();
            walker.set("rst_n", 1);
            (name, walker)
        })
        .collect()
}

fn pte(ppn: u64, flags: u64) -> u64 {
    ppn << 10 | flags
}

const V: u64 = 1;
const R: u64 = 2;
const W: u64 = 4;
const X: u64 = 8;
const U: u64 = 16;
const A: u64 = 64;
const D: u64 = 128;

#[test]
fn directed_leaves_and_faults() {
    let root = 0x80010;
    let l1 = 0x80011;
    let l0 = 0x80012;
    // VA 0x0000_0040_2030_1000: VPN[2]=1, VPN[1]=0x101, VPN[0]=0x101.
    let vpn = 1 << 18 | 0x101 << 9 | 0x101;
    let slot = |table: u32, index: u32| table * 4096 + index * 8;
    let mut cases: Vec<(&str, Memory, Outcome)> = Vec::new();
    let base = |leaf_level: u32, leaf: u64| {
        let mut memory = Memory::default();
        memory.ptes.insert(slot(root, 1), pte(u64::from(l1), V));
        memory.ptes.insert(slot(l1, 0x101), pte(u64::from(l0), V));
        let table = [l0, l1, root][leaf_level as usize];
        let index = [0x101, 0x101, 1][leaf_level as usize];
        memory.ptes.insert(slot(table, index), leaf);
        memory
    };
    cases.push((
        "4 KiB",
        base(0, pte(0x8_1234, V | R | W | U | A | D)),
        Outcome::Translated { page: 0x8_1234, permissions: 0b11011 },
    ));
    cases.push((
        "2 MiB keeps VPN[0]",
        base(1, pte(0x8_0200, V | X | A)),
        Outcome::Translated { page: 0x8_0301, permissions: 0b00100 },
    ));
    cases.push((
        "1 GiB keeps VPN[1:0]",
        base(2, pte(0x4_0000, V | R | A)),
        Outcome::Translated { page: 0x4_0000 | 0x101 << 9 | 0x101, permissions: 0b00001 },
    ));
    cases.push(("invalid", base(0, pte(0x8_1234, R | A)), Outcome::PageFault));
    cases.push(("W without R", base(0, pte(0x8_1234, V | W | A)), Outcome::PageFault));
    cases.push(("A clear", base(0, pte(0x8_1234, V | R | D)), Outcome::PageFault));
    cases.push(("N bit", base(0, pte(0x8_1234, V | R | A) | 1 << 63), Outcome::PageFault));
    cases.push(("PBMT", base(0, pte(0x8_1234, V | R | A) | 1 << 61), Outcome::PageFault));
    cases.push(("misaligned 2 MiB", base(1, pte(0x8_0201, V | R | A)), Outcome::PageFault));
    cases.push(("misaligned 1 GiB", base(2, pte(0x4_0200, V | R | A)), Outcome::PageFault));
    cases.push(("pointer at level 0", base(0, pte(0x8_1234, V)), Outcome::PageFault));
    cases.push(("leaf beyond 32 bits", base(0, pte(1 << 20, V | R | A)), Outcome::AccessFault));
    let mut wide_pointer = base(0, pte(0x8_1234, V | R | A));
    wide_pointer.ptes.insert(slot(root, 1), pte(1 << 21, V));
    cases.push(("pointer beyond 32 bits", wide_pointer, Outcome::AccessFault));
    let mut denied = base(0, pte(0x8_1234, V | R | A));
    denied.denied.insert(slot(l1, 0x101));
    cases.push(("denied PTE read", denied, Outcome::AccessFault));

    for (name, mut walker) in walkers() {
        for (case, memory, expected) in &cases {
            assert_eq!(reference(memory, root, vpn), *expected, "reference {case}");
            for latency in [0, 3] {
                assert_eq!(
                    walker.walk(memory, root, vpn, latency),
                    *expected,
                    "{name} {case} latency {latency}"
                );
            }
        }
    }
}

#[test]
fn random_tables_match_the_reference() {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for (name, mut walker) in walkers() {
        let walks = if name == "native" { 3000 } else { 300 };
        let mut seen = [0usize; 3];
        for _ in 0..walks {
            // Few tables and indices so walks share and revisit entries.
            let tables = [0x8_0010u32, 0x8_0011, 0x8_0012, 0x8_0013];
            let mut memory = Memory::default();
            for &table in &tables {
                for index in 0..4u32 {
                    let r = next();
                    // Mostly pointers and valid leaves; some raw flag bytes.
                    let flags = match r & 7 {
                        0..=2 => V,
                        3..=6 => {
                            let rwx = [R, R | W, X, R | X, R | W | X][(r >> 3 & 7) as usize % 5];
                            let accessed = if r >> 33 & 15 == 0 { 0 } else { A };
                            V | rwx | accessed | (r >> 6 & 1) * D | (r >> 7 & 1) * U
                        }
                        _ => r >> 8 & 0xff,
                    };
                    let ppn = match r >> 16 & 7 {
                        0..=2 => u64::from(tables[(r >> 19 & 3) as usize]),
                        3 => r >> 21 & 0xf_ffff,
                        4 => (r >> 21 & 0x3) << 18,
                        5 => (r >> 21 & 0x7ff) << 9,
                        _ => 1 << (20 + (r >> 21 & 7)),
                    };
                    let high = if r >> 40 & 15 == 0 { (r >> 44 & 0x3ff) << 54 } else { 0 };
                    memory.ptes.insert(table * 4096 + index * 8, ppn << 10 | flags | high);
                    if r >> 50 & 31 == 0 {
                        memory.denied.insert(table * 4096 + index * 8);
                    }
                }
            }
            let r = next();
            let vpn = ((r & 3) << 18 | (r >> 2 & 3) << 9 | (r >> 4 & 3)) as u32;
            let root = tables[(r >> 20 & 3) as usize];
            let expected = reference(&memory, root, vpn);
            seen[match expected {
                Outcome::Translated { .. } => 0,
                Outcome::PageFault => 1,
                Outcome::AccessFault => 2,
            }] += 1;
            let latency = (r >> 24 & 3) as u32;
            assert_eq!(
                walker.walk(&memory, root, vpn, latency),
                expected,
                "{name} vpn {vpn:#x} root {root:#x}"
            );
        }
        // Every outcome must be well represented, or the walk is untested.
        assert!(seen.iter().all(|&count| count * 20 >= walks), "{name} outcomes {seen:?}");
        eprintln!("{name} outcomes (translated, page fault, access fault): {seen:?}");
    }
}
