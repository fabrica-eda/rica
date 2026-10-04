use std::error::Error;
use std::path::Path;

use texo_cli::{Ecp5BitgenOptions, bitgen};

fn main() -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let build = project.join("build");
    let output = bitgen(&Ecp5BitgenOptions::new(
        build.join("jtag-i2c-target-250.txcp"),
        build.join("jtag-i2c-target-250.bit"),
    ))?;
    println!(
        "Texo bitgen: {} programmable PIPs, {} fixed edges; bitstream={}",
        output.programmable_pips,
        output.fixed_edges,
        output.bitstream.display()
    );
    Ok(())
}
