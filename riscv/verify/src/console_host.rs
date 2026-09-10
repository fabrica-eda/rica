//! Host side of the console kernel's byte stream contract, shared by native,
//! simultaneous-clock and physical JTAG verification.
use std::error::Error;

#[derive(Default)]
pub struct KernelConsole {
    pub output: Vec<u8>,
    sent: usize,
}

impl KernelConsole {
    pub fn input(&self) -> Option<u8> {
        b"ping\n".get(self.sent).copied()
    }

    /// Process exactly one completed exchange. Failed pushes remain pending;
    /// a held response must never be passed here twice.
    pub fn accept(&mut self, response: u16) -> Result<(), Box<dyn Error>> {
        if response & (1 << 9) != 0 {
            if self.input().is_none() {
                return Err("console accepted an unrequested input byte".into());
            }
            self.sent += 1;
        }
        if response & (1 << 8) != 0 {
            self.output.push(response as u8);
        }
        if response & 0xf000 != 0 {
            return Err("console response has nonzero reserved bits".into());
        }
        Ok(())
    }

    pub fn transcript(&self) -> Result<&str, Box<dyn Error>> {
        Ok(std::str::from_utf8(&self.output)?)
    }

    pub fn validate(&self) -> Result<(), Box<dyn Error>> {
        let text = self.transcript()?;
        let lines = text.lines().collect::<Vec<_>>();
        if self.sent != 5
            || lines.len() != 12
            || !text.ends_with('\n')
            || lines[0] != "rica: boot M-mode"
            || lines[9..] != ["console: pong irq=MEI rx=5", "tasks: PASS", "kernel: PASS"]
        {
            return Err(format!(
                "invalid kernel console transcript (sent={}): {text:?}",
                self.sent
            )
            .into());
        }
        let mut faults = Vec::new();
        for line in &lines[1..9] {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 4 || fields[0] != "fault" {
                return Err(format!("invalid fault log: {line:?}").into());
            }
            let mut values = [0_u64; 3];
            for (index, key) in ["cause=0x", "pc=0x", "tval=0x"].iter().enumerate() {
                let value = fields[index + 1]
                    .strip_prefix(key)
                    .ok_or("invalid fault field")?;
                if value.len() != 16 {
                    return Err("fault field must preserve all 64 bits".into());
                }
                values[index] = u64::from_str_radix(value, 16)?;
            }
            faults.push(values);
        }
        if faults.iter().map(|f| f[0]).collect::<Vec<_>>() != [5, 7, 7, 5, 1, 1, 1, 1]
            || faults[0][2] != faults[1][2]
            || !(0x1_8000_0000..0x1_8003_f000).contains(&faults[0][2])
            || faults[2][2] != 0x8003_f000
            || faults[3][2] != 0x8003_f000
            || faults[4] != [1, 0x1_8000_0000, 0x1_8000_0000]
            || faults[5] != [1, 0xffff_ffff_8000_0000, 0xffff_ffff_8000_0000]
            || faults[6] != [1, 0x8003_effe, 0x8003_f000]
            || faults[7] != [1, 0x8003_f000, 0x8003_f000]
            || faults[..4]
                .iter()
                .any(|f| !(0x8000_0000..0x8003_f000).contains(&f[1]))
        {
            return Err(format!("incorrect fault values in console: {faults:x?}").into());
        }
        Ok(())
    }
}
