"""Extract observed early/late capture boundaries, retaining censored results.

The independent variable is the nominal PLL phase code, NOT a simulator delay.
Clock skew, endpoint delays and route delays remain inseparable in these data.
"""
import argparse
import json
import math
from pathlib import Path
from phase_sweep import sha, STEP_PS, SAMPLES, MAGIC


def analyze(out, temperature_fraction=.20, guard_ps=200):
    from audit_phase import audit
    audit(out)
    probes=json.loads((out/'probes.json').read_text())
    raw_paths=sorted(out.glob('step-*/raw.json'))
    records=[json.loads(p.read_text()) for p in raw_paths]
    assert records
    config=(out/'measure.config').read_text()
    # Every measurement uses identical routes and logic; only these PLL words vary.
    import re
    normalize=lambda s:re.sub(r'(?m)^word: CLKOS2_(?:CPHASE|FPHASE) [01]+$', '', s)
    for path,r in zip(raw_paths,records):
        assert r['bitstream_sha256']==sha(path.parent/'measure.bit')
        assert r['config_sha256']==sha(path.parent/'measure.config')
        phase_config=(path.parent/'measure.config').read_text()
        assert normalize(config)==normalize(phase_config)
        coarse,fine=divmod(19*8+r['step'],8)
        assert f'word: CLKOS2_CPHASE {coarse%40:07b}' in phase_config
        assert f'word: CLKOS2_FPHASE {fine:03b}' in phase_config
        assert r['nominal_phase_ps']==r['step']*STEP_PS
        assert r['words'][:4]==[MAGIC,len(probes),1,SAMPLES]
        if r.get('header_words')==6:
            assert all(abs(c-(1<<19)*1.3)<=2 for c in r['words'][4:6])
        assert all(0<=c<=SAMPLES//2 for c in r['words'][r.get('header_words',4):])
        assert len(r['words'])==r.get('header_words',4)+r.get('words_per_probe',2)*len(probes)
        for raw,offset in zip(r['raw_frames'],r['offsets']):
            v=int(raw,16)>>offset
            assert [(v>>(i*32))&0xffffffff for i in range(len(r['words']))]==r['words']
    steps=sorted({r['step'] for r in records})
    rows=[]
    for p in probes:
        row=dict(p)
        for polarity,i in [('rising',0),('falling',1)]:
            observations=[]
            for step in steps:
                rr=[r for r in records if r['step']==step]
                counts=[r['words'][r.get('header_words',4)+r.get('words_per_probe',2)*p['id']+i] for r in rr]
                old_counts=[r['words'][r.get('header_words',4)+r['words_per_probe']*p['id']+2+i] for r in rr if r.get('words_per_probe',2)>=4]
                if old_counts and not p.get('constant_product',False):
                    assert all(0<=old<=wrong<=SAMPLES//2 for old,wrong in zip(old_counts,counts))
                observations.append(dict(step=step,phase_ps=step*STEP_PS,errors=counts,old_value_correct=old_counts,invalid_values=[wrong-old for wrong,old in zip(counts,old_counts)] if not p.get('constant_product',False) else None,trials_each=SAMPLES//2))
            first_pass=next((j for j,x in enumerate(observations) if all(n==0 for n in x['errors'])),None)
            last_old=max((j for j,x in enumerate(observations) if x['old_value_correct'] and all(n==SAMPLES//2 for n in x['old_value_correct'])),default=None)
            # All later points must pass; glitches/non-monotone behavior are reported.
            monotone=first_pass is not None and all(all(n==0 for n in x['errors']) for x in observations[first_pass:])
            old_prefix=last_old is not None and all(x['old_value_correct'] and all(n==SAMPLES//2 for n in x['old_value_correct']) for x in observations[:last_old+1])
            bracketed=monotone and old_prefix and last_old<first_pass
            row[polarity]=dict(observations=observations,monotone_pass_tail=monotone,monotone_old_value_prefix=old_prefix,bracketed=bracketed,
                last_all_old_ps=observations[last_old]['phase_ps'] if last_old is not None else None,
                first_all_correct_ps=observations[first_pass]['phase_ps'] if first_pass is not None else None)
        row['usable_boundary']=all(row[p]['bracketed'] for p in ['rising','falling']) and not row.get('constant_product',False)
        if row['usable_boundary']:
            lo=min(row[p]['last_all_old_ps'] for p in ['rising','falling'])
            hi=max(row[p]['first_all_correct_ps'] for p in ['rising','falling'])
            row['observed_aperture_envelope_ps']=[lo,hi]
            row['assumed_temperature_guarded_capture_ps']=max(0,hi)*(1+temperature_fraction)+guard_ps
        rows.append(row)
    model=dict(kind='measured_fixed_route_capture_apertures',sta_qualified=False,
        independent_arc_parameters_identified=False,reference_hz_nominal=12000000,
        reference_externally_calibrated=False,vco_hz_nominal=624000000,phase_quantum_ps=STEP_PS,
        launch_hz_nominal=15600000,voltage_condition='unchanged board supply; not independently measured',
        temperature_margin=dict(fraction=temperature_fraction,additional_guard_ps=guard_ps,measured=False,scope='experimental capture-budget selection, not PVT qualification'),
        sources={'pll':'https://www.latticesemi.com/view_document?document_id=50465','board':'https://www.latticesemi.com/view_document?document_id=52479'},
        exclusions=['No decomposition into FF clock-to-Q/setup/hold or clock skew','No voltage sweep','Two-pattern DSP and alternating-address BRAM are not exhaustive data sensitization','No CPU STA table replacement'],
        records=len(records),total_capture_checks=len(records)*len(probes)*SAMPLES,probes=rows,
        input_sha256={str(p):sha(p) for p in [out/'measure.v',out/'probes.json',out/'routed.json',out/'measure.config',out/'build.json',out/'physical-audit.json',out/'design.json',out/'generator-at-build.py',out/'tools-sha256.json',*raw_paths,*[p.parent/n for p in raw_paths for n in ['measure.config','measure.bit']]]})
    usable=[r for r in rows if r['usable_boundary']]
    model['usable_boundaries']=len(usable)
    if usable:
        model['suggested_guarded_step']=math.ceil(max(r['assumed_temperature_guarded_capture_ps'] for r in usable)/STEP_PS)
    (out/'aperture-model.json').write_text(json.dumps(model,indent=2)+'\n')
    print(json.dumps({k:v for k,v in model.items() if k not in ['probes','input_sha256']},indent=2))
    return model

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--temperature-fraction',type=float,default=.20);a=p.parse_args()
    assert 0<=a.temperature_fraction<=1
    analyze(a.output.resolve(),a.temperature_fraction)
