"""Experimental interval-aware ring fit; not an adopted timing library.

Fit separate earliest/latest path bounds from real board observations.

Ring averages constrain the shared structural route/LUT basis. Capture data
constrain the earliest and latest transitions separately. Fixed cell-access
delays are absorbed into cell arcs; the resulting parameters are a joint STA
model, not claims of independently identified transistor/pin delay.
"""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import numpy as np
import scipy
from scipy.optimize import least_squares
from fit_sta_candidate import project as base_project

def project(path):
    result=base_project(path)
    result.pop('dsp:comb',None)
    for key,count in path.items():
        if key.startswith('arc:MULT18X18D:REGS=NONE:'):
            source=key.rsplit(':',2)[1]
            side=source[-1] if source.startswith('SIGNED') else source[0]
            assert side in 'AB',source
            result['dsp:comb:'+side]+=count
    return result

from phase_sweep import sha


def features(path, corner):
    original=project(path)
    if corner=='max':return dict(original)
    return {name.replace(':setup_',':hold_') if ':setup_' in name else name:
            -count if ':setup_' in name else count for name,count in original.items()}


def fit_corner(rows,corner):
    prepared=[]
    fixed={'ff:setup_data':200.0} if corner=='max' else {'ff:hold_data':0.0}
    registered_output_present=any(r.get('kind')=='ebr_registered_output' for r in rows)
    for row in rows:
        if row.get('kind')=='clock_reference' or row.get('joint_controls') or row['measurement']=='clock_loop_period':continue
        # A generic DP16KD table must cover both output modes. Fit its lower
        # corner from the registered-output path and upper corner from the
        # memory-array path. Both modes remain in all validation checks below.
        if registered_output_present and corner=='min' and row.get('kind') in ['bram_output','ebr_output']:continue
        if corner=='max' and row.get('kind')=='ebr_registered_output':continue
        projected=[features(p,corner) for p in row['paths']]
        ring=row['measurement']=='loop_average'
        target=row['observed_ps'] if ring else row['observed_interval_ps'][1 if corner=='max' else 0]
        scale=sum(row['paths'][0].get(f'arc:TRELLIS_COMB:{pin}:F',0) for pin in 'ABCD') if ring else 1
        prepared.append(dict(row=row,projected=projected,target=target,scale=scale))
    train=[r for r in prepared if not r['row']['holdout']]
    names=sorted({n for r in train for p in r['projected'] for n in p}-fixed.keys())
    for r in prepared:
        r['matrix']=np.array([[p.get(n,0) for n in names] for p in r['projected']],dtype=float)
        r['fixed']=np.array([sum(p.get(n,0)*v for n,v in fixed.items()) for p in r['projected']])
    def prediction(r,coef):
        values=r['matrix']@coef+r['fixed']
        return float(np.max(values) if corner=='max' else np.min(values))
    def residual(coef):
        result=[]
        for r in train:
            error=(prediction(r,coef)-r['target'])/r['scale']
            if r['row']['measurement']=='loop_average':
                # An average constrains the envelope; it does not identify
                # either earliest or latest propagation as that average.
                error=max(0.,error) if corner=='min' else min(0.,error)
            result.append(error)
        return result
    trials=[least_squares(residual,np.full(len(names),initial),bounds=(0,np.inf),max_nfev=3000,
                          ftol=1e-10,xtol=1e-10,gtol=1e-10) for initial in [50.,200.]]
    result=min(trials,key=lambda r:float(np.sum(r.fun**2)))
    coefficients=dict(zip(names,map(float,result.x)));coefficients.update(fixed)
    singular=np.linalg.svd(result.jac,compute_uv=False)
    return dict(coefficients_ps=coefficients,optimizer_success=bool(result.success),
                training_normalized_rmse_ps=float(np.sqrt(np.mean(result.fun**2))),
                free_parameters=len(names),jacobian_rank=int(np.sum(singular>1e-6)),
                independent_pin_delays_identified=False)


def value(key,corner,models):
    if key=='arc:DCCA:CLKI:CLKO':return models.get('clock_common_latency_ps',0.)
    path={key:1};f=features(path,corner);coeff=models[corner]['coefficients_ps']
    missing=set(f)-coeff.keys()
    if missing:raise ValueError('Unmeasured model features: '+str(sorted(missing)))
    scale=1.2 if corner=='max' or key.startswith('check:') else .8
    return sum(count*coeff[name] for name,count in f.items())*scale


def entry(key,models):
    if key.startswith('check:'):
        # Minimum path uses minus hold_max, maximum path uses plus setup_max.
        return value(key,'min',models),value(key,'max',models)
    maximum=value(key,'max',models)
    minimum=min(value(key,'min',models),maximum)
    return minimum,maximum


def predict(row,models):
    paths=[]
    for path in row['paths']:
        low=high=0.
        for key,count in path.items():
            minimum,maximum=entry(key,models);low+=minimum*count;high+=maximum*count
        paths.append((low,high))
    return min(p[0] for p in paths),max(p[1] for p in paths)


def fit(path,output):
    data=json.loads(path.read_text());rows=data['rows']
    models={corner:fit_corner(rows,corner) for corner in ['min','max']}
    clock_estimates=[]
    for row in rows:
        if row['measurement']=='clock_loop_period' and not row['holdout']:
            low,high=predict(row,models)
            clock_estimates.append((sum(row['observed_interval_ps'])/2-(low+high)/2)/2)
    assert clock_estimates and min(clock_estimates)>0,clock_estimates
    models['clock_common_latency_ps']=float(np.median(clock_estimates))
    required_guard=1.
    train_predictions=[]
    for row in rows:
        if row['holdout'] or row.get('kind')=='clock_reference' or row.get('joint_controls'):continue
        low,high=predict(row,models)
        if row['measurement']=='capture_aperture':
            observed_low,observed_high=row['observed_interval_ps']
            required_guard=max(required_guard,low-observed_low,observed_high-high)
        elif row['measurement']=='clock_loop_period':
            observed_low,observed_high=row['observed_interval_ps']
            required_guard=max(required_guard,(low-observed_low)/2,(observed_high-high)/2)
        train_predictions.append((row,low,high))
    guard=math.ceil(required_guard)
    checks=[]
    for row in rows:
        if row.get('kind')=='clock_reference' or row.get('joint_controls'):continue
        low,high=predict(row,models)
        if row['measurement']=='loop_average':
            observed_low=observed_high=row['observed_ps'];used_guard=0
        else:
            observed_low,observed_high=row['observed_interval_ps']
            used_guard=guard*(2 if row['measurement']=='clock_loop_period' else 1)
        checks.append(dict(image=row['image'],probe=row['probe'],holdout=row['holdout'],
                           predicted_min_ps=low,predicted_max_ps=high,guard_ps=used_guard,
                           observed_interval_ps=[observed_low,observed_high],
                           min_passed=low-used_guard<=observed_low,max_passed=high+used_guard>=observed_high))
    heldout=[c for c in checks if c['holdout']]
    result=dict(schema=1,kind='experimental_interval_ring_dsp_port_candidate',sta_qualified=False,
                input_sha256={str(path.resolve()):sha(path),str(Path(__file__).resolve()):sha(Path(__file__)),str(Path(__file__).with_name('fit_sta_candidate.py').resolve()):sha(Path(__file__).with_name('fit_sta_candidate.py'))},
                numerical_environment=dict(numpy=np.__version__,scipy=scipy.__version__),corners=models,temperature_fraction=.2,
                setup_hold_guard_ps=guard,guard_fitted_from_training_only=True,
                ring_policy='One-sided envelope constraint; loop average is not fitted as an extremum.',
                dsp_policy='Separate A/B source-port coefficients, fitted only to the preplanned training patterns; no per-output parameter claims.',
                requires_fresh_independent_validation=True,
                heldout_passed=bool(heldout) and all(c['min_passed'] and c['max_passed'] for c in heldout),
                all_path_checks_passed=all(c['min_passed'] and c['max_passed'] for c in checks),
                checks=checks,legacy_delay_labels_used=False,
                gauge='Shared fixed access in cell arcs; max FF data setup reference 200 ps; min FF data hold reference 0 ps; independently fitted effective clock-to-Q corners.',
                clock_policy='One measured common clock-distribution latency; residual relative-clock uncertainty is checked with the global guard. Clock-loop periods contain two traversals.',
                ebr_output_corner_policy='OUTREG defines lower corner, NOREG defines upper corner; both modes are validated against both bounds.',
                training_clock_latency_estimates_ps=clock_estimates,
                qualifications_still_required=['complete timing-surface coverage','serialized-table rounding validation','124.8 MHz full CPU STA and hardware regression'])
    output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k not in ['checks']},indent=2))


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('features',type=Path);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();fit(args.features,args.output)
