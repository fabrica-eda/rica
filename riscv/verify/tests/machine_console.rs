use celox::{NativeBackend, Simulator, SimulatorBuilder};
use std::{collections::VecDeque, fs, path::Path};

#[derive(Clone, Copy, Default)]
struct Bus {
    address: u8,
    request: bool,
    write: bool,
    data: u64,
    mask: u8,
}
#[derive(Clone, Copy, Default)]
struct Host {
    sequence: bool,
    pop: bool,
    push: bool,
    data: u8,
}
#[derive(Default)]
struct Oracle {
    tx: VecDeque<u8>,
    rx: VecDeque<u8>,
    enabled: bool,
    errors: u8,
    pipe: Host,
    ack: bool,
    response: u16,
}

impl Oracle {
    fn read(&self, address: u8) -> u64 {
        let word = match address / 8 {
            1 => self.rx.front().map_or(0, |v| 0x100 | u64::from(*v)),
            2 => {
                u64::from(self.tx.len() < 16)
                    | (u64::from(!self.rx.is_empty()) << 1)
                    | (u64::from(!self.tx.is_empty()) << 2)
                    | (u64::from(self.rx.len() < 16) << 3)
            }
            3 => u64::from(self.enabled),
            4 => 0x434f4e01,
            5 => self.tx.len() as u64 | ((self.rx.len() as u64) << 8),
            6 => u64::from(self.errors),
            _ => 0,
        };
        word >> ((address % 8) * 8)
    }

    fn edge(&mut self, bus: Bus, host: Host) {
        let active = self.pipe.sequence != self.ack;
        let read = bus.request && !bus.write && bus.address == 8 && !self.rx.is_empty();
        let write = bus.request && bus.write && bus.address % 8 == 0 && bus.mask & 1 != 0;
        let take = active && self.pipe.pop && !self.tx.is_empty();
        let room = self.rx.len() < 16 || read;
        let put = active && self.pipe.push && room;
        if active {
            self.response = u16::from(take) << 8
                | u16::from(put) << 9
                | u16::from(!self.tx.is_empty()) << 10
                | u16::from(room) << 11;
            self.ack = self.pipe.sequence;
        }
        if take {
            self.response |= u16::from(self.tx.pop_front().unwrap());
        }
        if read {
            self.rx.pop_front();
        }
        if write && bus.address == 0x30 {
            self.errors &= !(bus.data as u8) & 3;
        }
        if write && bus.address == 0x18 {
            self.enabled = bus.data & 1 != 0;
        }
        if write && bus.address == 0 {
            if self.tx.len() < 16 {
                self.tx.push_back(bus.data as u8);
            } else {
                self.errors |= 1;
            }
        }
        if put {
            self.rx.push_back(self.pipe.data);
        } else if active && self.pipe.push {
            self.errors |= 2;
        }
        self.pipe = host;
    }
}

fn get(sim: &mut Simulator<NativeBackend>, name: &str) -> u64 {
    sim.get(sim.signal(name))
        .to_u64_digits()
        .first()
        .copied()
        .unwrap_or(0)
}

fn step(sim: &mut Simulator<NativeBackend>, oracle: &mut Oracle, bus: Bus, host: Host) {
    for (name, value) in [
        ("address", u64::from(bus.address)),
        ("request", u64::from(bus.request)),
        ("write_enable", u64::from(bus.write)),
        ("write_data", bus.data),
        ("write_mask", u64::from(bus.mask)),
        ("host_request_sequence", u64::from(host.sequence)),
        ("host_pop", u64::from(host.pop)),
        ("host_push", u64::from(host.push)),
        ("host_data", u64::from(host.data)),
    ] {
        let signal = sim.signal(name);
        sim.modify(|io| {
            if name == "write_data" {
                io.set(signal, value);
            } else {
                io.set(signal, value as u8);
            }
        })
        .unwrap();
    }
    assert_eq!(
        get(sim, "read_data"),
        oracle.read(bus.address),
        "pre-edge address {:x}",
        bus.address
    );
    oracle.edge(bus, host);
    sim.tick(sim.event("clk")).unwrap();
    assert_eq!(
        get(sim, "read_data"),
        oracle.read(bus.address),
        "post-edge address {:x}",
        bus.address
    );
    assert_eq!(
        get(sim, "interrupt"),
        u64::from(oracle.enabled && !oracle.rx.is_empty())
    );
    assert_eq!(get(sim, "host_response_sequence"), u64::from(oracle.ack));
    assert_eq!(get(sim, "host_response"), u64::from(oracle.response));
}

fn write(address: u8, data: u64) -> Bus {
    Bus {
        address,
        request: true,
        write: true,
        data,
        mask: 255,
    }
}
fn read(address: u8) -> Bus {
    Bus {
        address,
        request: true,
        ..Bus::default()
    }
}

#[test]
fn console_native_and_mapped_match_independent_queues() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/HardwareMemory.veryl"),
    )
    .unwrap();
    let native = SimulatorBuilder::new(&source, "MachineConsole")
        .build_native()
        .unwrap();
    let rtl =
        struo_frontend_veryl::analyze_and_lower(&source, "console_test", "MachineConsole").unwrap();
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
        let reset = sim.signal("rst_n");
        sim.modify(|io| io.set(reset, 0_u8)).unwrap();
        sim.tick(sim.event("clk")).unwrap();
        sim.modify(|io| io.set(reset, 1_u8)).unwrap();
        let mut oracle = Oracle::default();
        let mut host = Host::default();
        step(&mut sim, &mut oracle, write(0x18, 1), host);
        for value in 0..16 {
            host = Host {
                sequence: !host.sequence,
                push: true,
                data: 0xa0 + value,
                ..Host::default()
            };
            step(&mut sim, &mut oracle, write(0, u64::from(value)), host);
            step(&mut sim, &mut oracle, Bus::default(), host);
        }
        assert_eq!((oracle.tx.len(), oracle.rx.len()), (16, 16));
        // The same physical full slot is read and rewritten on one edge.
        host = Host {
            sequence: !host.sequence,
            pop: true,
            ..Host::default()
        };
        step(&mut sim, &mut oracle, Bus::default(), host);
        step(&mut sim, &mut oracle, write(0, 0xe1), host);
        assert_eq!(oracle.tx.len(), 16);
        assert_eq!(oracle.response & 255, 0);
        host = Host {
            sequence: !host.sequence,
            push: true,
            data: 0xf1,
            ..Host::default()
        };
        step(&mut sim, &mut oracle, Bus::default(), host);
        step(&mut sim, &mut oracle, read(8), host);
        assert_eq!(oracle.rx.len(), 16);
        // Held host request cannot repeat; simultaneous W1C loses to a new error.
        host.sequence = !host.sequence;
        step(&mut sim, &mut oracle, write(0, 0xe2), host);
        step(&mut sim, &mut oracle, write(0x30, 3), host);
        assert_eq!(oracle.errors, 2);
        for _ in 0..8 {
            step(&mut sim, &mut oracle, read(0x28), host);
        }
        for offset in 1..8 {
            step(&mut sim, &mut oracle, read(8 + offset), host);
            step(&mut sim, &mut oracle, write(0x18 + offset, 0), host);
            step(
                &mut sim,
                &mut oracle,
                Bus {
                    mask: 254,
                    ..write(0, 0)
                },
                host,
            );
        }
        assert_eq!(oracle.rx.len(), 16);
        assert!(oracle.enabled);
        let mut random = 0x728adc901356aef1_u64;
        for cycle in 0..6000 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            if cycle % 4 == 0 {
                host = Host {
                    sequence: !host.sequence,
                    pop: random & 1 != 0,
                    push: random & 2 != 0,
                    data: (random >> 8) as u8,
                };
            }
            let address = [0, 8, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38][(random >> 2) as usize & 7];
            let bus = Bus {
                address,
                request: true,
                write: random & 32 != 0,
                data: random >> 16,
                mask: (random >> 12) as u8,
            };
            step(&mut sim, &mut oracle, bus, host);
        }
        let sequence = sim.signal("host_request_sequence");
        sim.modify(|io| {
            io.set(reset, 0_u8);
            io.set(sequence, 0_u8);
        })
        .unwrap();
        sim.tick(sim.event("clk")).unwrap();
        assert_eq!(get(&mut sim, "interrupt"), 0);
        assert_eq!(get(&mut sim, "host_response"), 0);
        sim.modify(|io| io.set(reset, 1_u8)).unwrap();
        step(
            &mut sim,
            &mut Oracle::default(),
            read(0x28),
            Host::default(),
        );
    }
}
