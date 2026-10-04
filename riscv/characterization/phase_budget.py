"""Build empirical whole-path budgets with separate-location/pattern holdouts.

This is a measured circuit-family screening model, not a library of STA arcs.
A family without a successful holdout is explicitly unavailable to consumers.
"""
import argparse
import json
import math
from pathlib import Path
from phase_sweep import sha, STEP_PS


def family(p):
    if p['kind']=='ff_lut_ff':return f"ff_lut_ff/{p['lut_stages']}"
    if p['kind']=='loaded_ff_route':return f"loaded_ff_route/{p['fanout']}"
    return p['kind']


def build(path):
    model=json.loads(path.read_text())
    for source,h in model['input_sha256'].items():assert sha(source)==h
    usable=[p for p in model['probes'] if p['usable_boundary']]
    families={}
    for key in sorted({family(p) for p in usable}):
        train=[p for p in usable if family(p)==key and not p['holdout']]
        test=[p for p in usable if family(p)==key and p['holdout']]
        if not train or not test:
            families[key]=dict(available=False,reason='Missing independently placed/pattern holdout')
            continue
        # Fit uses training data only; test data cannot enlarge the selected budget.
        upper=max(p['observed_aperture_envelope_ps'][1] for p in train)
        lower=min(p['observed_aperture_envelope_ps'][0] for p in train)
        margin=model['temperature_margin']
        budget=max(0,upper)*(1+margin['fraction'])+margin['additional_guard_ps']
        early=lower-abs(lower)*margin['fraction']-margin['additional_guard_ps']
        checks=[dict(probe=p['id'],observed_upper_ps=p['observed_aperture_envelope_ps'][1],observed_lower_ps=p['observed_aperture_envelope_ps'][0],within_budget=p['observed_aperture_envelope_ps'][1]<=budget and p['observed_aperture_envelope_ps'][0]>=early) for p in test]
        families[key]=dict(available=all(c['within_budget'] for c in checks),training_probes=[p['id'] for p in train],holdout_checks=checks,
            measured_training_upper_ps=upper,measured_training_lower_ps=lower,guarded_capture_budget_ps=budget,guarded_old_value_latest_phase_ps=early)
    selected=[f['guarded_capture_budget_ps'] for f in families.values() if f['available']]
    result=dict(kind='measured_whole_path_family_budget',sta_qualified=False,
        applies_to='sampled measurement circuit families; includes endpoint and clock skew effects',
        forbidden_uses=['standalone FF clkQ/setup/hold','arbitrary CPU route signoff','untested data patterns or memory/DSP modes'],
        temperature_margin=model['temperature_margin'],source_sha256={str(path.resolve()):sha(path)},families=families,
        suggested_old_value_step=math.floor(min(f['guarded_old_value_latest_phase_ps'] for f in families.values() if f['available'])/STEP_PS) if selected else None,
        suggested_capture_step=math.ceil(max(selected)/STEP_PS) if selected else None)
    target=path.parent/'budget-model.json';target.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2));return result


def query(path,key):
    model=json.loads(path.read_text())
    for source,h in model['source_sha256'].items():assert sha(source)==h
    entry=model['families'][key]
    if not entry['available']:raise ValueError('Family failed independent holdout; do not substitute a guessed cost')
    return dict(family=key,capture_budget_ps=entry['guarded_capture_budget_ps'],sta_qualified=False,model_sha256=sha(path))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--apertures',type=Path);p.add_argument('--model',type=Path);p.add_argument('--family');a=p.parse_args()
    if a.apertures:build(a.apertures.resolve())
    else:print(json.dumps(query(a.model,a.family),indent=2))
