"""Sequential board-only characterization; archives every programmed experiment."""
from pathlib import Path
import subprocess,shutil
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay'
for pin in 'ABCD':
 for axis in 'xy':
  dest=OUT/f'registered-{pin}-{axis}';assert not dest.exists(),dest
  print(f'BUILD {pin}-{axis}',flush=True)
  subprocess.run(['python3','riscv/characterization/generate.py','--axis',axis,'--pin',pin],cwd=ROOT,check=True)
  subprocess.run(['python3','riscv/characterization/build.py'],cwd=ROOT,check=True)
  print(f'PROGRAM {pin}-{axis}',flush=True)
  with (OUT/'program.log').open('w') as f:subprocess.run(['openFPGALoader','-c','xvc-client','--ip','127.0.0.1','--port','3721','--write-sram',str(OUT/'measure.bit')],cwd=ROOT,stdout=f,stderr=subprocess.STDOUT,check=True)
  with (OUT/'readout.log').open('w') as f:subprocess.run(['python3','riscv/characterization/readout.py','--seconds','22','--bitstream',str(OUT/'measure.bit'),'--output',str(OUT/'raw-counts.json')],cwd=ROOT,stdout=f,stderr=subprocess.STDOUT,check=True)
  dest.mkdir()
  for p in OUT.iterdir():
   if p.is_file():shutil.copy2(p,dest/p.name)
  for p in Path(__file__).parent.glob('*.py'):shutil.copy2(p,dest/p.name)
  print(f'ARCHIVED {pin}-{axis}',flush=True)
