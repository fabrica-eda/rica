use std::error::Error;
use std::fs;
use std::path::Path;
use std::time::Instant;

use celox::{Simulator, SimulatorBuilder};
use struo_frontend_veryl::analyze_and_lower;
use struo_synth::synthesize;
use struo_target_ecp5::{
    Ecp5Cell, JtaggBinding, MappedPortDirection, MappingOptions, OpenDrainIo, PllBinding,
    map_to_ecp5_with_options,
};
use texo_struo::{PrimitiveMetadata, import_ecp5};

struct Bus {
    simulator: Simulator,
    scl_high: bool,
    master_drive_low: bool,
    model_debug_bus: bool,
    writes: Vec<(u8, u8)>,
}

impl Bus {
    fn new(simulator: Simulator, model_debug_bus: bool) -> Self {
        Self {
            simulator,
            scl_high: true,
            master_drive_low: false,
            model_debug_bus,
            writes: Vec::new(),
        }
    }

    fn value(&mut self, name: &str) -> u32 {
        let signal = self.simulator.signal(name);
        self.simulator
            .get(signal)
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn tick_system_clock(&mut self) -> Result<(), Box<dyn Error>> {
        let target_drive_low = self.value("sda_drive_low") != 0;
        let bus_high = !(self.master_drive_low || target_drive_low);
        let scl = self.simulator.signal("scl_i");
        let sda = self.simulator.signal("sda_i");
        if self.model_debug_bus {
            let address = self.value("debug_address") as u8;
            let read_data = match address {
                0x10 => 0xa5_u8,
                0x11 => 0x5a_u8,
                _ => 0xff_u8,
            };
            let debug_read_data = self.simulator.signal("debug_read_data");
            self.simulator.modify(|io| {
                io.set(scl, u8::from(self.scl_high));
                io.set(sda, u8::from(bus_high));
                io.set(debug_read_data, read_data);
            })?;
        } else {
            self.simulator.modify(|io| {
                io.set(scl, u8::from(self.scl_high));
                io.set(sda, u8::from(bus_high));
            })?;
        }
        let clock = self.simulator.event("clk");
        self.simulator.tick(clock)?;

        if self.model_debug_bus && self.value("debug_write_strobe") != 0 {
            let address = self.value("debug_address") as u8;
            let data = self.value("debug_write_data") as u8;
            self.writes.push((address, data));
        }
        Ok(())
    }

    fn settle(&mut self) -> Result<(), Box<dyn Error>> {
        // Five 250 MHz cycles cover both input synchronizer stages and edge
        // detection while remaining much shorter than a standard I2C phase.
        for _ in 0..5 {
            self.tick_system_clock()?;
        }
        Ok(())
    }

    fn lines(&mut self, scl_high: bool, master_drive_low: bool) -> Result<(), Box<dyn Error>> {
        self.scl_high = scl_high;
        self.master_drive_low = master_drive_low;
        self.settle()
    }

    fn start(&mut self) -> Result<(), Box<dyn Error>> {
        self.lines(true, false)?;
        self.lines(true, true)?;
        self.lines(false, true)
    }

    fn repeated_start(&mut self) -> Result<(), Box<dyn Error>> {
        self.lines(false, false)?;
        self.lines(true, false)?;
        self.lines(true, true)?;
        self.lines(false, true)
    }

    fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        self.lines(false, true)?;
        self.lines(true, true)?;
        self.lines(true, false)
    }

    fn write_bit(&mut self, bit: bool) -> Result<(), Box<dyn Error>> {
        let drive_low = !bit;
        self.lines(false, drive_low)?;
        self.lines(true, drive_low)?;
        self.lines(false, drive_low)
    }

    fn write_byte(&mut self, byte: u8) -> Result<bool, Box<dyn Error>> {
        for bit in (0..8).rev() {
            self.write_bit(byte & (1 << bit) != 0)?;
        }
        self.lines(false, false)?;
        self.lines(true, false)?;
        let acknowledged = self.value("sda_drive_low") != 0;
        self.lines(false, false)?;
        Ok(acknowledged)
    }

    fn read_bit(&mut self) -> Result<bool, Box<dyn Error>> {
        self.lines(false, false)?;
        self.lines(true, false)?;
        let bit = self.value("sda_drive_low") == 0;
        self.lines(false, false)?;
        Ok(bit)
    }

    fn read_byte(&mut self, acknowledge: bool) -> Result<u8, Box<dyn Error>> {
        let mut byte = 0_u8;
        for _ in 0..8 {
            byte = (byte << 1) | u8::from(self.read_bit()?);
        }
        self.lines(false, acknowledge)?;
        self.lines(true, acknowledge)?;
        self.lines(false, acknowledge)?;
        self.lines(false, false)?;
        Ok(byte)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let core_source = fs::read_to_string(project.join("src/I2cTarget250.veryl"))?;
    let top_source = fs::read_to_string(project.join("src/I2cDebugTop.veryl"))?;
    let source = format!("{core_source}\n{top_source}");
    let clock_probe_source = fs::read_to_string(project.join("src/ClockDomainProbe.veryl"))?;
    let jtag_source = format!(
        "{source}\n{clock_probe_source}\n{}",
        fs::read_to_string(project.join("src/JtagI2cDebugTop.veryl"))?
    );

    let mut clock_probe =
        SimulatorBuilder::new(&clock_probe_source, "ClockDomainProbe").build_native()?;
    let clk_250 = clock_probe.event("clk_250");
    let clk_ref = clock_probe.signal("clk_ref");
    let pll_locked = clock_probe.signal("pll_locked");
    let debug_rst_n = clock_probe.signal("debug_rst_n");
    clock_probe.modify(|io| {
        io.set(pll_locked, 0_u8);
        io.set(debug_rst_n, 0_u8);
        io.set(clk_ref, 0_u8);
    })?;
    clock_probe.tick(clk_250)?;
    clock_probe.modify(|io| {
        io.set(pll_locked, 1_u8);
        io.set(debug_rst_n, 1_u8);
    })?;
    let mut phase = 0_u32;
    let mut ref_level = false;
    for _ in 0..65_550 {
        phase += 24;
        if phase >= 250 {
            phase -= 250;
            ref_level = !ref_level;
            clock_probe.modify(|io| io.set(clk_ref, u8::from(ref_level)))?;
        }
        clock_probe.tick(clk_250)?;
    }
    let clock_status = clock_probe
        .get(clock_probe.signal("clock_status"))
        .to_u32_digits()
        .first()
        .copied()
        .unwrap_or(0);
    if clock_status & 0x1f != 0x1f {
        return Err(format!(
            "clock-domain probe did not validate the 250/12 MHz ratio: status=0x{clock_status:02x}"
        )
        .into());
    }
    println!("Celox clock probe: lock, reset release, activity, and 250/12 ratio passed");

    let mut bus = Bus::new(
        SimulatorBuilder::new(&core_source, "I2cTarget250").build_native()?,
        true,
    );
    let reset = bus.simulator.signal("rst_n");
    bus.simulator.modify(|io| io.set(reset, 0_u8))?;
    bus.settle()?;
    bus.simulator.modify(|io| io.set(reset, 1_u8))?;
    bus.settle()?;

    // A non-matching address must be ignored and NACKed.
    bus.start()?;
    if bus.write_byte(0x40 << 1)? {
        return Err("I2C target ACKed a foreign address".into());
    }
    bus.stop()?;

    // Write two bytes beginning at debug register 0x10.
    bus.start()?;
    for byte in [0x42 << 1, 0x10, 0xab, 0xcd] {
        if !bus.write_byte(byte)? {
            return Err(format!("I2C target NACKed write byte 0x{byte:02x}").into());
        }
    }
    bus.stop()?;
    if bus.writes != [(0x10, 0xab), (0x11, 0xcd)] {
        return Err(format!("debug writes differ: {:?}", bus.writes).into());
    }

    // Select register 0x10, issue a repeated START, then read two bytes.
    bus.start()?;
    for byte in [0x42 << 1, 0x10] {
        if !bus.write_byte(byte)? {
            return Err(format!("I2C target NACKed pointer byte 0x{byte:02x}").into());
        }
    }
    bus.repeated_start()?;
    if !bus.write_byte((0x42 << 1) | 1)? {
        return Err("I2C target NACKed its read address".into());
    }
    let first = bus.read_byte(true)?;
    let second = bus.read_byte(false)?;
    bus.stop()?;
    if (first, second) != (0xa5, 0x5a) {
        return Err(format!("debug read differs: got 0x{first:02x}, 0x{second:02x}").into());
    }
    println!("Celox RTL: address NACK, sequential write, repeated START read passed");

    let mut top = Bus::new(
        SimulatorBuilder::new(&source, "I2cDebugTop").build_native()?,
        false,
    );
    let reset = top.simulator.signal("rst_n");
    let clock_status_input = top.simulator.signal("clock_status");
    top.simulator.modify(|io| {
        io.set(reset, 0_u8);
        io.set(clock_status_input, 0x1f_u8);
    })?;
    top.settle()?;
    top.simulator.modify(|io| io.set(reset, 1_u8))?;
    top.settle()?;

    top.start()?;
    for byte in [0x42 << 1, 0x10, 0x3c] {
        if !top.write_byte(byte)? {
            return Err(format!("I2C debug top NACKed write byte 0x{byte:02x}").into());
        }
    }
    top.stop()?;
    if top.value("led") != 0x3c {
        return Err("I2C debug top did not expose scratch register 0x10 on LEDs".into());
    }

    top.start()?;
    for byte in [0x42 << 1, 0x00] {
        if !top.write_byte(byte)? {
            return Err(format!("I2C debug top NACKed pointer byte 0x{byte:02x}").into());
        }
    }
    top.repeated_start()?;
    if !top.write_byte((0x42 << 1) | 1)? {
        return Err("I2C debug top NACKed its read address".into());
    }
    let device_id = top.read_byte(true)?;
    let version = top.read_byte(false)?;
    top.stop()?;
    if (device_id, version) != (0x52, 0x02) {
        return Err(format!(
            "I2C debug top identity differs: got 0x{device_id:02x}, 0x{version:02x}"
        )
        .into());
    }

    println!("Celox debug top: identity read and scratch-to-LED write passed");

    let analyze_started = Instant::now();
    let design = analyze_and_lower(&source, "rica_i2c", "I2cDebugTop")?;
    let analyze_elapsed = analyze_started.elapsed();
    let synth_started = Instant::now();
    let synthesized = synthesize(&design)?;
    let synth_elapsed = synth_started.elapsed();
    println!(
        "Struo synth: {} nodes, {} registers; analyze={analyze_elapsed:?}, synth={synth_elapsed:?}",
        synthesized.netlist.nodes().len(),
        synthesized.netlist.registers().len()
    );

    let map_started = Instant::now();
    let mut mapped = map_to_ecp5_with_options(
        &synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 250,
            ..MappingOptions::default()
        },
    )?;
    let map_elapsed = map_started.elapsed();
    println!(
        "Struo ECP5 map @ 250 MHz: {} cells, data={} -> {} ps, overall={} -> {} ps, LUT depth={} -> {}, critical registers={} -> {}, retimed={}, map={map_elapsed:?}",
        mapped.cells().len(),
        mapped.retiming().original_period_ps,
        mapped.retiming().selected_period_ps,
        mapped.retiming().original_overall_period_ps,
        mapped.retiming().selected_overall_period_ps,
        mapped.retiming().original_lut_depth,
        mapped.retiming().selected_lut_depth,
        mapped.retiming().original_critical_registers,
        mapped.retiming().selected_critical_registers,
        mapped.retiming().applied,
    );

    mapped.bind_open_drain_ios(&[OpenDrainIo::new("sda", "sda_i", "sda_drive_low")])?;
    let sda = mapped
        .ports()
        .iter()
        .find(|port| port.name == "sda")
        .ok_or("mapped netlist has no physical SDA port")?;
    if sda.direction != MappedPortDirection::Inout {
        return Err("physical SDA port is not inout".into());
    }
    if mapped
        .ports()
        .iter()
        .any(|port| port.name == "sda_i" || port.name == "sda_drive_low")
    {
        return Err("split SDA ports remain after open-drain binding".into());
    }
    if !mapped
        .cells()
        .iter()
        .any(|cell| matches!(cell, Ecp5Cell::TrellisIo { .. }))
    {
        return Err("open-drain binding emitted no TRELLIS_IO".into());
    }

    println!("Struo open-drain: sda inout -> TRELLIS_IO");

    let jtag_design = analyze_and_lower(&jtag_source, "rica_i2c", "JtagI2cDebugTop")?;
    let jtag_synthesized = synthesize(&jtag_design)?;
    let mut jtag_mapped = map_to_ecp5_with_options(
        &jtag_synthesized.netlist,
        MappingOptions {
            timing_goal_mhz: 250,
            ..MappingOptions::default()
        },
    )?;
    jtag_mapped.bind_jtagg(&JtaggBinding::with_prefix("jtag"))?;
    let pll: PllBinding =
        serde_json::from_str(&fs::read_to_string(project.join("pll-12-to-250.json"))?)?;
    jtag_mapped.bind_pll(&pll)?;

    if jtag_mapped
        .ports()
        .iter()
        .any(|port| port.name.starts_with("jtag_"))
    {
        return Err("logical JTAG ports remain after dedicated JTAGG binding".into());
    }
    if jtag_mapped
        .ports()
        .iter()
        .any(|port| port.name == "clk_250" || port.name == "pll_locked")
    {
        return Err("logical PLL output ports remain after EHXPLLL binding".into());
    }
    if jtag_mapped
        .cells()
        .iter()
        .filter(|cell| matches!(cell, Ecp5Cell::Pll { .. }))
        .count()
        != 1
    {
        return Err("PLL binding did not emit exactly one EHXPLLL cell".into());
    }
    if jtag_mapped
        .cells()
        .iter()
        .filter(|cell| matches!(cell, Ecp5Cell::Jtagg { .. }))
        .count()
        != 1
    {
        return Err("dedicated JTAGG binding did not emit exactly one JTAGG cell".into());
    }

    let imported = import_ecp5(&jtag_mapped)?;
    if !imported.metadata().values().any(|metadata| {
        matches!(
            metadata,
            PrimitiveMetadata::Jtagg {
                extension_register_1: true,
                extension_register_2: true,
            }
        )
    }) {
        return Err("Texo import lost JTAGG extension-register configuration".into());
    }

    if !imported
        .metadata()
        .values()
        .any(|metadata| matches!(metadata, PrimitiveMetadata::Pll { .. }))
    {
        return Err("Texo import lost EHXPLLL configuration".into());
    }

    println!("Struo -> Texo: JTAGG ER1/ER2 and 12-to-250 MHz EHXPLLL preserved");
    Ok(())
}
