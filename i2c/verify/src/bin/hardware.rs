use std::error::Error;
use std::fmt::Write;
use std::fs;
use std::path::Path;

struct SvfI2c {
    text: String,
}

impl SvfI2c {
    fn new() -> Self {
        Self {
            text: String::from(
                "! ECP5 JTAGG ER1 I2C hardware regression\n\
                 TRST OFF;\n\
                 ENDIR IDLE;\n\
                 ENDDR IDLE;\n\
                 STATE RESET;\n\
                 STATE IDLE;\n\
                 SIR 8 TDI (32);\n",
            ),
        }
    }

    fn command(&mut self, scl: bool, sda_low: bool) {
        let command = (u8::from(scl) << 1) | u8::from(sda_low);
        writeln!(self.text, "SDR 3 TDI ({command:X});").unwrap();
        self.text.push_str("RUNTEST 16 TCK;\n");
    }

    fn sample(&mut self, scl: bool, sda_low: bool, expected_high: bool) {
        let command = (u8::from(scl) << 1) | u8::from(sda_low);
        let expected = if expected_high { 7 } else { 0 };
        writeln!(
            self.text,
            "SDR 3 TDI ({command:X}) TDO ({expected:X}) MASK (7);"
        )
        .unwrap();
        self.text.push_str("RUNTEST 16 TCK;\n");
    }

    fn start(&mut self) {
        self.command(true, false);
        self.command(true, true);
        self.command(false, true);
    }

    fn repeated_start(&mut self) {
        self.command(false, false);
        self.command(true, false);
        self.command(true, true);
        self.command(false, true);
    }

    fn stop(&mut self) {
        self.command(false, true);
        self.command(true, true);
        self.command(true, false);
    }

    fn write_bit(&mut self, bit: bool) {
        let drive_low = !bit;
        self.command(false, drive_low);
        self.command(true, drive_low);
        self.command(false, drive_low);
    }

    fn write_byte(&mut self, byte: u8) {
        for bit in (0..8).rev() {
            self.write_bit(byte & (1 << bit) != 0);
        }
        self.command(false, false);
        self.command(true, false);
        self.sample(true, false, false);
        self.command(false, false);
    }

    fn read_bit(&mut self, expected: bool) {
        self.command(false, false);
        self.command(true, false);
        self.sample(true, false, expected);
        self.command(false, false);
    }

    fn read_byte(&mut self, expected: u8, acknowledge: bool) {
        self.read_byte_masked(expected, 0xff, acknowledge);
    }

    fn read_byte_masked(&mut self, expected: u8, mask: u8, acknowledge: bool) {
        for bit in (0..8).rev() {
            if mask & (1 << bit) != 0 {
                self.read_bit(expected & (1 << bit) != 0);
            } else {
                self.command(false, false);
                self.command(true, false);
                self.command(true, false);
                self.command(false, false);
            }
        }
        self.command(false, acknowledge);
        self.command(true, acknowledge);
        self.command(false, acknowledge);
        self.command(false, false);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = project.join("build/jtag-i2c-hardware.svf");
    let lock_probe_output = project.join("build/jtag-pll-lock-probe.svf");
    let mut bus = SvfI2c::new();

    // Write 0xa5 to scratch register 0x10. This must appear on led[7:0].
    bus.start();
    bus.write_byte(0x42 << 1);
    bus.write_byte(0x10);
    bus.write_byte(0xa5);
    bus.stop();

    // Read device ID and protocol version, checking every returned bit on TDO.
    bus.start();
    bus.write_byte(0x42 << 1);
    bus.write_byte(0x00);
    bus.repeated_start();
    bus.write_byte((0x42 << 1) | 1);
    bus.read_byte(0x52, true);
    bus.read_byte(0x02, false);
    bus.stop();

    // The low five status bits prove PLL lock, synchronized reset release,
    // 250 MHz counter activity, completion of the ratio window, and a measured
    // 250/12 MHz ratio within tolerance. Bit 5 is a free-running heartbeat.
    bus.start();
    bus.write_byte(0x42 << 1);
    bus.write_byte(0x03);
    bus.repeated_start();
    bus.write_byte((0x42 << 1) | 1);
    bus.read_byte_masked(0x1f, 0x1f, false);
    bus.stop();

    bus.text.push_str("STATE IDLE;\n");
    fs::create_dir_all(output.parent().unwrap())?;
    fs::write(&output, bus.text)?;
    println!("hardware SVF: {}", output.display());

    let mut lock_probe = SvfI2c::new();
    lock_probe.text.push_str(
        "SIR 8 TDI (38);\n\
         SDR 8 TDI (00) TDO (FF) MASK (FF);\n\
         STATE IDLE;\n",
    );
    fs::write(&lock_probe_output, lock_probe.text)?;
    println!("PLL lock probe SVF: {}", lock_probe_output.display());
    Ok(())
}
