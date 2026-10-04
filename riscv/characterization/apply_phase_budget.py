"""Consume a measured budget to choose phases, then independently retest hardware.

No CPU timing evidence is transferred. This exercises the measured model's
actual consumer and refuses unsupported families instead of inventing arcs.
"""
import argparse
import json
from pathlib import Path
import shutil
from phase_sweep import sha, sweep, SAMPLES, STEP_PS, BASELINE
from phase_budget import family, query


def apply(model_path,out,execute=False):
    model=json.loads(model_path.read_text())
    aperture_path=Path(next(iter(model['source_sha256'])))
    assert sha(aperture_path)==model['source_sha256'][str(aperture_path)]
    aperture=json.loads(aperture_path.read_text())
    for path,h in aperture['input_sha256'].items():assert sha(path)==h
    available={key:query(model_path,key) for key,f in model['families'].items() if f['available']}
    assert available,'No family passed its holdout'
    early,late=model['suggested_old_value_step'],model['suggested_capture_step']
    # The checker operates on the opposite launch edge. Do not consume its
    # settling budget when converting a measured data budget into a phase.
    assert -150<early<late<110,(early,late)
    out.mkdir(parents=True,exist_ok=False)
    base=aperture_path.parent
    for name in [*json.loads((base/'build.json').read_text())['sha256'],'build.json']:
        shutil.copy2(base/name,out/name)
    record=dict(model_sha256=sha(model_path),model=str(model_path.resolve()),
        applied_families=available,unavailable_families=[k for k,f in model['families'].items() if not f['available']],
        old_value_phase_step=early,new_value_phase_step=late,nominal_phase_ps=[early*STEP_PS,late*STEP_PS],
        repeats=3,cpu_clock_hz_unchanged=124800000,cpu_sta_modified=False,
        baseline_bitstream_sha256=sha(BASELINE),completed=False)
    (out/'application.json').write_text(json.dumps(record,indent=2)+'\n')
    if execute:
        sweep(out,[early,late],3)
        probes=json.loads((out/'probes.json').read_text())
        results=[]
        for path in sorted(out.glob('step-*/raw.json')):
            raw=json.loads(path.read_text());assert raw['words_per_probe']==4
            for p in probes:
                words=raw['words'][6+4*p['id']:10+4*p['id']]
                if p.get('constant_product',False):expected=[0,0,SAMPLES//2,SAMPLES//2]
                elif raw['step']==early:expected=[SAMPLES//2]*4
                else:expected=[0]*4
                results.append(dict(raw=str(path),probe=p['id'],family=family(p),family_supported=family(p) in available,passed=words==expected))
        record.update(completed=True,checks=len(results),passed=all(r['passed'] for r in results),results=results,
            input_sha256={str(p):sha(p) for p in out.glob('step-*/raw.json')},restore_log_sha256=sha(out/'restore.log'))
        assert 'RV64 hardware tests: 1/1 passed' in (out/'restore.log').read_text()
        (out/'application.json').write_text(json.dumps(record,indent=2)+'\n')
        assert record['passed'],'Independent guarded phase confirmation failed'
    print(json.dumps({k:v for k,v in record.items() if k not in ['results','input_sha256','applied_families']},indent=2))
    return record

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--model',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--run',action='store_true');a=p.parse_args();apply(a.model.resolve(),a.output.resolve(),a.run)
