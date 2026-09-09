use celox::{NativeBackend, Simulator, SimulatorBuilder};
use std::{fs, path::Path};

fn get(sim: &mut Simulator<NativeBackend>, name: &str) -> u64 {
    sim.get(sim.signal(name))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

#[derive(Default)]
struct Oracle {
    time: u64,
    compare: u64,
    software: bool,
    pending: bool,
}

impl Oracle {
    fn reset() -> Self {
        Self {
            compare: u64::MAX,
            ..Self::default()
        }
    }

    fn read(&self, address: u16) -> u64 {
        let register = match address & !7 {
            0 => u64::from(self.software),
            0x4000 => self.compare,
            0xbff0 => 62_400_000,
            0xbff8 => self.time,
            _ => 0,
        };
        register >> (8 * (address & 7))
    }

    fn edge(&mut self, address: u16, write: bool, data: u64, mask: u8, freeze: bool) {
        self.pending = self.time >= self.compare;
        let mut written_time = false;
        // Byte-addressed oracle; no reproduction of the RTL's shifter/mux.
        for byte in 0..8 {
            if !write || mask & (1 << byte) == 0 || (address & 7) + byte >= 8 {
                continue;
            }
            let lane = (address & 7) + byte;
            let value = ((data >> (8 * byte)) & 0xff) << (8 * lane);
            let bits = 0xff_u64 << (8 * lane);
            match address & !7 {
                0 if lane == 0 => self.software = value & 1 != 0,
                0x4000 => self.compare = (self.compare & !bits) | value,
                0xbff8 => {
                    self.time = (self.time & !bits) | value;
                    written_time = true;
                }
                _ => {}
            }
        }
        if !freeze && !written_time {
            self.time = self.time.wrapping_add(1);
        }
    }
}

#[test]
fn native_and_ecp5_timer_match_byte_writes_wrap_and_pending_oracle() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/HardwareMemory.veryl"),
    )
    .unwrap();
    let native = SimulatorBuilder::new(&source, "MachineTimer")
        .build_native()
        .unwrap();
    let rtl =
        struo_frontend_veryl::analyze_and_lower(&source, "timer_test", "MachineTimer").unwrap();
    let mapped = struo_target_ecp5::map_to_ecp5_with_options(
        &struo_synth::synthesize(&rtl).unwrap().netlist,
        struo_target_ecp5::MappingOptions {
            timing_goal_mhz: 175,
            ..Default::default()
        },
    )
    .unwrap();
    let mapped = struo_celox::ecp5_simulator(&mapped)
        .unwrap()
        .build_native()
        .unwrap();
    for mut sim in [native, mapped] {
        let clk = sim.event("clk");
        let rst = sim.signal("rst_n");
        let addr = sim.signal("address");
        let we = sim.signal("write_enable");
        let data = sim.signal("write_data");
        let mask = sim.signal("write_mask");
        let freeze = sim.signal("freeze");
        sim.modify(|io| {
            io.set(rst, 0_u8);
            io.set(addr, 0_u16);
            io.set(we, 0_u8);
            io.set(data, 0_u64);
            io.set(mask, 0_u8);
            io.set(freeze, 0_u8);
        })
        .unwrap();
        sim.tick(clk).unwrap();
        sim.modify(|io| io.set(rst, 1_u8)).unwrap();
        let mut oracle = Oracle::reset();
        let mut stimuli = vec![
            (0xbff8, true, 0xffff_ffff_ffff_fffe, 255, false),
            (0xbff8, false, 0, 0, false),
            (0xbff8, false, 0, 0, false),
            (0xbff8, false, 0, 0, false),
            (0x4000, true, 0, 255, false),
            (0xbff8, false, 0, 0, true),
            (0x4000, true, u64::MAX, 255, true),
            (0xbff8, false, 0, 0, true),
        ];
        let mut random = 0xd8a4_097c_b155_23ef_u64;
        for address in [0, 0x4000, 0xbff0, 0xbff8, 0x1230] {
            for offset in 0..8 {
                for byte_mask in 0..=255_u8 {
                    random ^= random << 13;
                    random ^= random >> 7;
                    random ^= random << 17;
                    stimuli.push((
                        address + offset,
                        true,
                        random,
                        byte_mask,
                        byte_mask & 1 != 0,
                    ));
                }
            }
        }
        for (address, write, input, byte_mask, frozen) in stimuli {
            sim.modify(|io| {
                io.set(addr, address);
                io.set(we, u8::from(write));
                io.set(data, input);
                io.set(mask, byte_mask);
                io.set(freeze, u8::from(frozen));
            })
            .unwrap();
            assert_eq!(
                get(&mut sim, "read_data"),
                oracle.read(address),
                "pre-edge {address:04x}"
            );
            oracle.edge(address, write, input, byte_mask, frozen);
            sim.tick(clk).unwrap();
            assert_eq!(
                get(&mut sim, "read_data"),
                oracle.read(address),
                "post-edge {address:04x}"
            );
            assert_eq!(get(&mut sim, "software_interrupt") != 0, oracle.software);
            assert_eq!(get(&mut sim, "timer_interrupt") != 0, oracle.pending);
        }
        // Reset has the same meaning after arbitrary software writes.
        sim.modify(|io| {
            io.set(rst, 0_u8);
            io.set(addr, 0x4000_u16);
        })
        .unwrap();
        sim.tick(clk).unwrap();
        assert_eq!(get(&mut sim, "read_data"), u64::MAX);
        assert_eq!(get(&mut sim, "software_interrupt"), 0);
        assert_eq!(get(&mut sim, "timer_interrupt"), 0);
    }
}
