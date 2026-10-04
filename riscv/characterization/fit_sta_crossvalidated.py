"""Fit the predeclared structural model and calibrate guard on training folds.

Fresh holdouts are evaluated only after coefficients and the guard are fixed.
No numerical vendor timing labels or CPU slack enter this calculation.
"""
import argparse
from concurrent.futures import ProcessPoolExecutor
import copy
import json
import math
from pathlib import Path
import numpy as np
import scipy
from fit_sta_dsp_distance_experiment import fit_corner, predict, residual_family
from phase_sweep import sha

_ROWS = None


def corner_models(rows):
    models={corner:fit_corner(rows,corner) for corner in ['min','max']}
    assert all(models[c]['optimizer_success'] for c in ['min','max'])
    estimates=[]
    for row in rows:
        if row['measurement']=='clock_loop_period' and not row['holdout']:
            low,high=predict(row,models)
            estimates.append((sum(row['observed_interval_ps'])/2-(low+high)/2)/2)
    assert estimates and min(estimates)>0,estimates
    models['clock_common_latency_ps']=float(np.median(estimates))
    return models,estimates


def init_fold_worker(rows):
    global _ROWS
    _ROWS=rows


def training_fold(index):
    rows=copy.deepcopy(_ROWS)
    omitted=rows[index]
    assert not omitted['holdout']
    omitted['holdout']=True
    models,_=corner_models(rows)
    low,high=predict(omitted,models)
    observed_low,observed_high=omitted['observed_interval_ps']
    traversals=2 if omitted['measurement']=='clock_loop_period' else 1
    return dict(image=omitted['image'],probe=omitted['probe'],family=calibration_family(omitted),
        lower_deficit_ps=max(0.,(low-observed_low)/traversals),
        upper_deficit_ps=max(0.,(observed_high-high)/traversals),
        min_fit_rank=models['min']['jacobian_rank'],max_fit_rank=models['max']['jacobian_rank'])


def calibration_family(row):
    families={residual_family(key) for path in row['paths'] for key in path}
    families.discard(None)
    checks={f for f in families if f.startswith(('EBR_CHECK_','FF_CONTROL_'))}
    if checks:
        assert len(checks)==1,checks
        return next(iter(checks))
    for family in ['DSP','EBR_OUTPUT','L6MUX','PFUMX','CARRY']:
        if family in families:return family
    assert not families,families
    return 'SHARED'


def eligible(row):
    return row.get('kind')!='clock_reference' and not row.get('joint_controls')


def fit(feature_file,plan_file,output,workers):
    data=json.loads(feature_file.read_text());rows=data['rows']
    plan=json.loads(plan_file.read_text())
    for name,digest in plan['planned_analysis_sha256'].items():assert sha(name)==digest,name
    assert plan['guard_protocol_amendment']['before_first_fresh_model_validation']
    models,clock_estimates=corner_models(rows)
    indices=[i for i,r in enumerate(rows) if eligible(r) and not r['holdout'] and r['measurement']!='loop_average']
    folds=[]
    with ProcessPoolExecutor(max_workers=workers,initializer=init_fold_worker,initargs=(rows,)) as pool:
        for result in pool.map(training_fold,indices):
            folds.append(result)
            print(json.dumps(dict(training_fold=len(folds),total=len(indices),**result)),flush=True)
    # Keep cell/check-family residuals in that family's min/max envelope.
    # The shared guard comes only from FF data/route and clock references;
    # unrelated primitive errors are not charged to every CPU path.
    calibration=list(folds)
    for row in rows:
        if row['holdout'] or not eligible(row) or row['measurement']=='loop_average':continue
        low,high=predict(row,models);observed_low,observed_high=row['observed_interval_ps']
        traversals=2 if row['measurement']=='clock_loop_period' else 1
        calibration.append(dict(family=calibration_family(row),
            lower_deficit_ps=max(0.,(low-observed_low)/traversals),
            upper_deficit_ps=max(0.,(observed_high-high)/traversals)))
    shared=[c for c in calibration if c['family']=='SHARED']
    assert shared
    guard=math.ceil(max([1.]+[max(c['lower_deficit_ps'],c['upper_deficit_ps']) for c in shared]))
    models['residual_bounds_ps']={}
    for family in sorted({c['family'] for c in calibration}-{'SHARED'}):
        group=[c for c in calibration if c['family']==family]
        models['residual_bounds_ps'][family]=dict(
            lower_ps=math.ceil(max(0.,max(c['lower_deficit_ps'] for c in group)-guard)),
            upper_ps=math.ceil(max(0.,max(c['upper_deficit_ps'] for c in group)-guard)))
    checks=[]
    for row in rows:
        if not eligible(row):continue
        low,high=predict(row,models)
        if row['measurement']=='loop_average':observed_low=observed_high=row['observed_ps'];used_guard=0
        else:
            observed_low,observed_high=row['observed_interval_ps']
            used_guard=guard*(2 if row['measurement']=='clock_loop_period' else 1)
        checks.append(dict(image=row['image'],probe=row['probe'],holdout=row['holdout'],
            predicted_min_ps=low,predicted_max_ps=high,guard_ps=used_guard,
            observed_interval_ps=[observed_low,observed_high],
            min_passed=low-used_guard<=observed_low,max_passed=high+used_guard>=observed_high))
    heldout=[c for c in checks if c['holdout']]
    fresh=[c for c in heldout if c['image'] in plan['datasets']]
    assert len(fresh)==plan['fresh_holdout_probes']==38
    sources=[feature_file,plan_file,Path(__file__),Path(__file__).with_name('fit_sta_dsp_distance_experiment.py'),Path(__file__).with_name('fit_sta_candidate.py')]
    result=dict(schema=1,kind='experimental_interval_ring_dsp_distance_candidate',sta_qualified=False,
        input_sha256={str(p.resolve()):sha(p) for p in sources},
        numerical_environment=dict(numpy=np.__version__,scipy=scipy.__version__),corners=models,
        temperature_fraction=.2,setup_hold_guard_ps=guard,guard_fitted_from_training_only=True,
        guard_method='Shared FF-data/route/clock leave-one-training-capture-out and in-sample residual, rounded up with minimum 1 ps. Primitive/check-family excess is calibrated separately into that family min/max bounds from training folds only.',
        training_folds=folds,heldout_passed=bool(heldout) and all(c['min_passed'] and c['max_passed'] for c in heldout),
        fresh_heldout_passed=all(c['min_passed'] and c['max_passed'] for c in fresh),fresh_heldout_samples=len(fresh),
        all_path_checks_passed=all(c['min_passed'] and c['max_passed'] for c in checks),checks=checks,
        legacy_delay_labels_used=False,
        ring_policy='One-sided envelope constraint; loop average is not fitted as an extremum.',
        dsp_policy='A/B source-port coefficients and shared output bit-distance basis; full-product observation includes all structurally reachable outputs.',
        gauge='Shared fixed access in cell arcs; max FF data setup reference 200 ps; min FF data hold reference 0 ps; independently fitted effective clock-to-Q corners.',
        clock_policy='One measured common clock-distribution latency; residual relative-clock uncertainty is checked with the global guard. Clock-loop periods contain two traversals.',
        ebr_output_corner_policy='OUTREG defines lower corner, NOREG defines upper corner; both modes are validated against both bounds.',
        training_clock_latency_estimates_ps=clock_estimates,
        qualifications_still_required=['complete timing-surface coverage','serialized-table rounding validation','124.8 MHz full CPU STA and hardware regression'])
    output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:result[k] for k in ['setup_hold_guard_ps','heldout_passed','fresh_heldout_passed','all_path_checks_passed']},indent=2),flush=True)


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('features',type=Path);p.add_argument('--plan',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--workers',type=int,default=4)
    a=p.parse_args();fit(a.features.resolve(),a.plan.resolve(),a.output.resolve(),a.workers)
