//! Check the physical-write/response ordering and forwarding against byte RAM.
use std::{fs, path::Path, sync::OnceLock};

use celox::{NativeBackend, NativeProgramImage, Simulator, SimulatorBuilder};

fn image(fast: u64) -> &'static NativeProgramImage {
    static IMAGES: [OnceLock<NativeProgramImage>; 6] = [const { OnceLock::new() }; 6];
    IMAGES[fast as usize].get_or_init(|| {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let source = fs::read_to_string(project.join("src/HardwareMemory.veryl")).unwrap();
        SimulatorBuilder::new(&source, "HardwareMemory")
            .param("FAST_LSU", fast)
            .compile_native()
            .unwrap()
            .into_program_image()
    })
}

struct Memory {
    sim: Simulator<NativeBackend>,
    sequence: u8,
}

impl Memory {
    fn new(fast: u64) -> Self {
        let sim = SimulatorBuilder::new("", "HardwareMemory")
            .build_native_from_image(image(fast).clone())
            .unwrap();
        let mut mem = Self { sim, sequence: 0 };
        for name in [
            "rst_n",
            "cpu_instr_address",
            "cpu_instr_request_sequence",
            "cpu_data_address",
            "cpu_data_write",
            "cpu_data_mask",
            "cpu_data_we",
            "cpu_data_request_sequence",
            "debug_write",
            "debug_address",
            "debug_data",
        ] {
            mem.set(name, 0);
        }
        mem.tick();
        mem.set("rst_n", 1);
        mem
    }

    fn set(&mut self, name: &str, value: u64) {
        let signal = self.sim.signal(name);
        self.sim
            .modify(|io| match name {
                "cpu_data_write" => io.set(signal, value),
                "cpu_instr_address" | "cpu_data_address" | "debug_address" => {
                    io.set(signal, value as u32)
                }
                _ => io.set(signal, value as u8),
            })
            .unwrap();
    }

    fn get(&mut self, name: &str) -> u64 {
        let signal = self.sim.signal(name);
        self.sim
            .get(signal)
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn tick(&mut self) {
        let clock = self.sim.event("clk");
        self.sim.tick(clock).unwrap();
    }

    fn request(&mut self, address: u32, write: Option<(u64, u8)>) -> (usize, u64) {
        self.sequence ^= 1;
        self.set("cpu_data_address", address.into());
        self.set("cpu_data_we", u64::from(write.is_some()));
        self.set("cpu_data_write", write.map_or(0, |w| w.0));
        self.set("cpu_data_mask", write.map_or(0, |w| u64::from(w.1)));
        self.set("cpu_data_request_sequence", self.sequence.into());
        for elapsed in 1..=5 {
            self.tick();
            if self.get("cpu_data_response_sequence") == u64::from(self.sequence) {
                return (elapsed, self.get("cpu_data_read"));
            }
        }
        panic!("memory request did not complete");
    }
}

#[test]
fn stores_and_matching_loads_complete_on_the_physical_write_or_forward_edge() {
    for fast in 0..=5 {
        let mut mem = Memory::new(fast);
        for offset in 0..8 {
            let address = 0x1fff8 + offset;
            let data = 0x8172635445362718_u64 ^ u64::from(offset);
            assert_eq!(
                mem.request(address, Some((data, 255))).0,
                if fast != 0 { 2 } else { 3 }
            );
            let (edges, result) = mem.request(address, None);
            assert_eq!(result, data);
            assert_eq!(edges, if fast == 1 || fast >= 3 { 2 } else { 3 });
            // A response is a held bundle, including after the ordinary RAM
            // output catches up with the forwarded value one edge later.
            for _ in 0..4 {
                mem.tick();
                assert_eq!(mem.get("cpu_data_read"), data);
                assert_eq!(
                    mem.get("cpu_data_response_sequence"),
                    u64::from(mem.sequence)
                );
            }
        }
    }
}

#[test]
fn partial_overlapping_and_wrapping_writes_match_byte_memory() {
    for fast in 0..=5 {
        let mut mem = Memory::new(fast);
        let mut expected = vec![0_u8; 1 << 18];
        // Initialize all bytes that the randomized unaligned loads can read.
        for address in (0..128).step_by(8).chain((0x3ff80..0x40000).step_by(8)) {
            mem.request(address, Some((0, 255)));
        }
        let mut random = 0x739e_ab21_7542_dcb9_u64;
        for index in 0..768 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let address = (0x3ffc0 + ((random >> 8) as u32 & 63)) & 0x3ffff;
            let mask = if index % 3 == 0 { 255 } else { random as u8 };
            mem.request(address, Some((random, mask)));
            for byte in 0..8 {
                if mask & (1 << byte) != 0 {
                    expected[(address as usize + byte) & 0x3ffff] = (random >> (8 * byte)) as u8;
                }
            }
            // Exact hits, overlapping misses and upper-bit physical aliases.
            for delta in [0, 1, 7] {
                let load = address + delta + if index % 2 == 0 { 0x8000_0000 } else { 0 };
                let want = u64::from_le_bytes(std::array::from_fn(|byte| {
                    expected[(load as usize + byte) & 0x3ffff]
                }));
                assert_eq!(
                    mem.request(load, None).1,
                    want,
                    "fast={fast}, index={index}, address={load:x}"
                );
            }
        }
    }
}

#[test]
fn consecutive_debug_bytes_keep_the_registered_address_and_data_together() {
    for fast in 0..=5 {
        let mut mem = Memory::new(fast);
        for base in [0_u32, 0x8000_0000, 0x8001_ff00] {
            for byte in 0..32 {
                mem.set("debug_address", u64::from(base + byte));
                mem.set("debug_data", u64::from(byte * 7 + 13));
                mem.set("debug_write", 1);
                mem.tick();
            }
            mem.set("debug_write", 0);
            mem.tick();
            mem.tick();
            for offset in (0..32_u32).step_by(8) {
                let want = u64::from_le_bytes(std::array::from_fn(|byte| {
                    ((offset + byte as u32) * 7 + 13) as u8
                }));
                let physical = if base == 0 { 0x3_f000 } else { base & 0x3_ffff };
                assert_eq!(
                    mem.request(physical + offset, None).1,
                    want,
                    "fast={fast}, base={base:x}, offset={offset}"
                );
                mem.set("cpu_instr_address", u64::from(base + offset));
                for _ in 0..4 {
                    mem.tick();
                }
                assert_eq!(
                    mem.get("cpu_instr_line"),
                    want,
                    "instruction fast={fast}, base={base:x}, offset={offset}"
                );
            }
        }
    }
}

#[test]
fn token_holds_do_not_repeat_writes_and_debug_loading_invalidates_forwarding() {
    for fast in 0..=5 {
        let mut mem = Memory::new(fast);
        let data = 0x0123456789abcdef;
        mem.request(0x300, Some((data, 255)));
        mem.set("cpu_data_write", 0);
        for _ in 0..5 {
            mem.tick();
        }
        assert_eq!(mem.request(0x300, None).1, data);
        mem.set("debug_address", 0x8000_0303);
        mem.set("debug_data", 0x55);
        mem.set("debug_write", 1);
        mem.tick();
        mem.tick();
        mem.set("debug_write", 0);
        mem.tick();
        mem.tick();
        let (edges, result) = mem.request(0x300, None);
        assert_eq!(edges, if fast >= 3 { 2 } else { 3 });
        assert_eq!(result, 0x0123456755abcdef);
        mem.set("rst_n", 0);
        mem.tick();
        mem.sequence = 0;
        mem.set("cpu_data_request_sequence", 0);
        mem.set("rst_n", 1);
        assert_eq!(
            mem.request(0x300, None),
            (if fast >= 3 { 2 } else { 3 }, 0x0123456755abcdef)
        );
    }
}
