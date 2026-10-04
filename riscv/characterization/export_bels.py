# Run with nextpnr-ecp5 --um5g-85k --package CABGA381 --run this-file.
import json
from pathlib import Path
output=Path('toolchain/measurements/2026-09-22-ecp5-measured-delay/bels.json')
output.parent.mkdir(parents=True,exist_ok=True)
output.write_text(json.dumps([str(bel) for bel in ctx.getBels()])+'\n')
