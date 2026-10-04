"""Audit selected raw hardware data, topology, model and native PnR evidence."""
from pathlib import Path
import json,hashlib,statistics,re,importlib.util
from readout import decode
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay'
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
m=json.loads((OUT/'measured-model.json').read_text());assert not m['sta_qualified'];assert m['holdout_max_abs_relative_error']<=.05
for p,h in m['input_sha256'].items():assert sha(ROOT/p)==h
assert len(m['rows'])==96 and sum(r['holdout'] for r in m['rows'])==24
checks=[]
for d in sorted(OUT.glob('registered-*')):
 raw=json.loads((d/'raw-counts.json').read_text());probes=json.loads((d/'probes.json').read_text());cells=json.loads((d/'routed.json').read_text())['modules']['top']['cells'];build=json.loads((d/'build.json').read_text())
 # Confirm archived build output matches the image named in the raw readout.
 assert raw['bitstream_sha256']==sha(d/'measure.bit')==build['sha256'][str((OUT/'measure.bit').relative_to(ROOT))]
 for p in probes:
  k,n=p['id'],p['stages'];pin='ABCD'.index(p['input_pin']);gatepin=(pin+1)%4
  for i,bel in enumerate(p['sites']):
   cell=cells[f'ring_{k}_{i}'];prev=cells[f'ring_{k}_{(i-1)%n}'];assert cell['attributes']['NEXTPNR_BEL']==bel
   assert cell['connections'][p['input_pin']]==prev['connections']['F']
   init=sum((not ((v>>pin&1) and (v>>gatepin&1)) if i==0 else not (v>>pin&1))<<v for v in range(16));assert cell['parameters']['INITVAL']==format(init,'016b')
  samples=[s for s in raw['samples'] if s['probe']==k and s['sequence']>12];assert len(samples)>=4
  for sample in samples:
   frames=[decode(int(f,16)) for f in sample['raw_frames']];expected={key:sample[key] for key in ['sequence','probe','count','bit_offset']};assert frames==[expected,expected]
  counts=[s['count'] for s in samples];assert (max(counts)-min(counts))/statistics.median(counts)<.01
 first,last=raw['samples'][0],raw['samples'][-1];expected_s=(last['sequence']-first['sequence'])*(1<<22)/12000000;observed_s=last['elapsed_s']-first['elapsed_s'];assert abs(observed_s/expected_s-1)<.02
 checks.append(dict(experiment=d.name,samples=len(raw['samples']),bitstream_sha256=sha(d/'measure.bit'),schedule_relative_error=observed_s/expected_s-1))
a=json.loads((OUT/'placement-01/application.json').read_text());assert a['exit_code']==0 and a['model_sha256']==sha(OUT/'measured-model.json');assert not a['uncalibrated_placement_predictor_enabled'];assert '--no-timing-optimization' in a['commands'][0]
for p,h in a['input_sha256'].items():assert sha(ROOT/p)==h
j=json.loads((OUT/'placement-01/jtck.json').read_text());assert j['native_setup_ps']>=0 and j['native_hold_ps']>=0;assert j['setup_uncertainty_ps']==j['hold_uncertainty_ps']==200;assert j['jtck_nominal_slacks_and_edges_match'];assert j['jtck_checks']==dict(setup_checks=515,hold_checks=515)
proof_path=ROOT/'toolchain/measurements/2026-09-21-rv64-a/end-carry-cycle-comparison.json';proof=json.loads(proof_path.read_text());frozen=Path(a['commands'][0][2]);assert frozen.resolve()==Path(proof['frozen_source']).resolve()
for source,h in proof['source_sha256'].items():assert sha(ROOT/source)==h and sha(frozen/'src'/Path(source).name)==h
log=(OUT/'restored-atomic-os.log').read_text();assert 'RV64 hardware tests: 1/1 passed' in log;assert 'minstret=41092' in log
r=dict(passed=True,experiments=checks,calibration_paths=96,training_paths=72,holdout_paths=24,stable_samples=sum(r['samples'] for r in m['rows']),max_holdout_relative_error=m['holdout_max_abs_relative_error'],max_within_probe_relative_range=max(r['relative_range'] for r in m['rows']),model_sha256=sha(OUT/'measured-model.json'),model_source_sha256={p.name:sha(p) for p in Path(__file__).parent.glob('*.py')},source_proof_sha256=sha(proof_path),pnr_application_sha256=sha(OUT/'placement-01/application.json'),pnr_checkpoint_sha256=sha(OUT/'placement-01/result.texo.json'),jtck_sha256=sha(OUT/'placement-01/jtck.json'),restored_os_log_sha256=sha(OUT/'restored-atomic-os.log'),new_cpu_image_programmed=False,restored_bitstream_sha256=sha(ROOT/'riscv/build/riscv-hardware-compact-atomic-124p8-20260922.bit'))
(OUT/'validation.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items() if k not in ['experiments','model_source_sha256']},indent=2))
