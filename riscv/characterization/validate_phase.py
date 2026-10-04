"""Reproduce the adopted model and verify its independent hardware application."""
import argparse
import contextlib
import io
import json
from pathlib import Path
from analyze_phase import analyze
from phase_budget import build, family
from phase_sweep import sha, SAMPLES, BASELINE
from audit_phase import audit


def validate(out):
    aperture=out/'aperture-model.json';budget=out/'budget-model.json'
    previous=[sha(aperture),sha(budget)]
    for path,h in json.loads((out/'tools-sha256.json').read_text()).items():assert sha(path)==h
    with contextlib.redirect_stdout(io.StringIO()):
        model=analyze(out);cost=build(aperture)
    assert [sha(aperture),sha(budget)]==previous,'Model is not reproducible from its sealed data'
    assert model['usable_boundaries']==31
    appdir=out/'confirmation';audit(appdir)
    app=json.loads((appdir/'application.json').read_text())
    assert app['completed'] and app['passed'] and app['model_sha256']==sha(budget)
    assert app['new_value_phase_step']==cost['suggested_capture_step']
    assert app['old_value_phase_step']==cost['suggested_old_value_step']
    assert app['baseline_bitstream_sha256']==sha(BASELINE)=='3128297a36c001211e2c02fca5a354afd297c22962af068ebb6aedc7cddbf1b8'
    probes=json.loads((out/'probes.json').read_text())
    records=list(appdir.glob('step-*/raw.json'));assert len(records)==6
    seen=set()
    for path in records:
        raw=json.loads(path.read_text());seen.add((raw['step'],raw['repeat']))
        assert raw['header_words']==6 and raw['words_per_probe']==4
        assert raw['words'][1:4]==[32,1,SAMPLES]
        assert all(abs(c-(1<<19)*1.3)<=2 for c in raw['words'][4:6])
        assert sha(path)==app['input_sha256'][str(path.resolve())]
        assert sha(path.parent/'measure.bit')==raw['bitstream_sha256']
        assert sha(path.parent/'measure.config')==raw['config_sha256']
        for frame,offset in zip(raw['raw_frames'],raw['offsets']):
            value=int(frame,16)>>offset
            assert [(value>>(32*i))&0xffffffff for i in range(len(raw['words']))]==raw['words']
        for p in probes:
            k=p['id'];words=raw['words'][6+4*k:10+4*k]
            if p.get('constant_product',False):expected=[0,0,SAMPLES//2,SAMPLES//2]
            elif raw['step']==app['old_value_phase_step']:expected=[SAMPLES//2]*4
            else:expected=[0]*4
            assert words==expected,(path,k,words,expected)
    assert seen=={(step,repeat) for step in [app['old_value_phase_step'],app['new_value_phase_step']] for repeat in range(3)}
    assert sha(appdir/'restore.log')==app['restore_log_sha256']
    assert 'RV64 hardware tests: 1/1 passed' in (appdir/'restore.log').read_text()
    record=dict(passed=True,usable_boundaries=model['usable_boundaries'],measurement_images=model['records'],
        measurement_capture_checks=model['total_capture_checks'],independent_confirmation_images=len(records),
        confirmation_capture_checks=len(records)*32*SAMPLES,available_families=sum(f['available'] for f in cost['families'].values()),
        model_sha256=sha(aperture),budget_sha256=sha(budget),application_sha256=sha(appdir/'application.json'),
        source_sha256={str(p.resolve()):sha(p) for p in [*Path(__file__).parent.glob('*phase*.py'),Path(__file__).parent/'readout.py']})
    (out/'validation.json').write_text(json.dumps(record,indent=2)+'\n')
    print(json.dumps(record,indent=2));return record

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);a=p.parse_args();validate(a.output.resolve())
