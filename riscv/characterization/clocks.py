# These are implementation budgets for the readout counter, not measurements.
# Characterization oscillators are intentionally combinational loops.
import json
from pathlib import Path
probes=json.loads(Path('toolchain/measurements/2026-09-22-ecp5-measured-delay/probes.json').read_text())
for p in probes:
    ctx.addClock('ro_%d[%d]' % (p['id'],p['stages']-1),100)
    ctx.addClock('div_%d[3]' % p['id'],6.25)
ctx.addClock('jtck',6)
