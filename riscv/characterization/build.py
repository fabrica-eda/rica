"""Build a measurement-only image, auditing every preserved LUT and BEL."""
from pathlib import Path
import subprocess,json,hashlib
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay'
yosys='/nix/store/slvgs8kvy4vxvf9li522vj6fp2iv4fsd-yosys-0.68/bin/yosys'
nextpnr='/nix/store/zhq1h4dygy4kjqm07ip7zdimcqil4fl0-nextpnr-0.11.1/bin/nextpnr-ecp5'
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
record={'measurement_only':True,'cpu_timing_qualified':False,'intentional_combinational_loops':True,'commands':[]}
def run(name,args):
 record['commands'].append(args)
 with (OUT/(name+'.log')).open('w') as f:subprocess.run(args,cwd=ROOT,stdout=f,stderr=subprocess.STDOUT,check=True)
run('yosys',[yosys,'-Q','-T','-p',f'read_verilog {OUT}/measure.v; synth_ecp5 -top measure -json {OUT}/measure.json'])
run('nextpnr-011',[nextpnr,'--um5g-85k','--package','CABGA381','--speed','8','--json',str(OUT/'measure.json'),'--lpf',str(OUT/'measure.lpf'),'--textcfg',str(OUT/'measure.config'),'--write',str(OUT/'routed.json'),'--pre-pack',str(ROOT/'riscv/characterization/clocks.py'),'--ignore-loops','--no-tmdriv','--freq','12','--disable-router-lutperm'])
m=json.loads((OUT/'routed.json').read_text())['modules']['top'];probes=json.loads((OUT/'probes.json').read_text())
for p in probes:
 for i,bel in enumerate(p['sites']):
  c=m['cells'][f"ring_{p['id']}_{i}"];assert c['attributes']['NEXTPNR_BEL']==bel;assert c['type']=='TRELLIS_COMB';pin="ABCD".index(p.get('input_pin','A'));gatepin=(pin+1)%4
  value=sum((not ((v>>pin&1) and (v>>gatepin&1)) if i==0 else not (v>>pin&1))<<v for v in range(16))
  assert c['parameters']['INITVAL']==format(value,'016b')
assert 'FAIL at' not in (OUT/'nextpnr-011.log').read_text()
run('ecppack',['/usr/bin/ecppack',str(OUT/'measure.config'),str(OUT/'measure.bit')])
record['sha256']={str(p.relative_to(ROOT)):sha(p) for p in [OUT/'measure.v',OUT/'probes.json',OUT/'measure.lpf',OUT/'measure.json',OUT/'routed.json',OUT/'measure.config',OUT/'measure.bit',*Path(__file__).parent.glob('*.py')]}
record['tools_sha256']={p:sha(p) for p in [yosys,nextpnr,'/usr/bin/ecppack']};(OUT/'build.json').write_text(json.dumps(record,indent=2)+'\n');print('Measurement image build/audit passed')
