"""Fit a structural joint model to board measurements, not vendor delay labels.

This is deliberately an unqualified candidate. Fixed input/output access delays
are inseparable from cell delay in ring measurements; the explicit gauge below
absorbs them into cell arcs. Holdout error and unsupported features are retained.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import numpy as np
from scipy.optimize import least_squares
from phase_sweep import sha


def project(path):
    result = Counter()
    for key, count in path.items():
        if key.startswith('pip:'):
            category = key[4:]
            if '_to_' not in category: continue
            source, sink = category.split('_to_', 1)
            target = re.match(r'span([0126])([hv])', sink)
            if target:
                result['wire:'+target[1]+target[2]] += count
                if source.startswith('span'): result['wire:switch'] += count
        elif key.startswith('fanout:'):
            category = key[7:]
            if '_to_span' in category:
                result['wire:load'] += count
        elif key.startswith('arc:'):
            cell, source, sink = key[4:].rsplit(':',2)
            if cell == 'TRELLIS_FF': result['ff:clock_q_effective'] += count
            elif cell == 'TRELLIS_COMB': result['lut:'+source] += count
            elif cell.startswith('TRELLIS_CARRY'):
                if source in 'ABCD': result['lut:'+source] += count
                result['carry:'+('cin' if source == 'FCI' else 'data')+':'+sink] += count
            elif cell == 'TRELLIS_PFUMX':
                if source in 'ABCD': result['lut:'+source] += count
                result['pfumx:'+('select' if source == 'M' else 'data')] += count
            elif cell == 'TRELLIS_L6MUX21': result['l6mux:'+('select' if source == 'M' else 'data')] += count
            elif cell == 'DP16KD': result['ebr:clock_q_effective'] += count
            elif cell == 'MULT18X18D:REGS=NONE': result['dsp:comb'] += count
            else: raise ValueError('unreviewed cell '+cell)
        elif key.startswith('check:'):
            _, cell, pin, _ = key.split(':')
            if cell=='TRELLIS_FF':result['ff:setup_'+('data' if pin in ['DI','M'] else pin)] += count
            elif cell=='DP16KD':
                group=next(prefix for prefix in ['OCE','AD','DI','CE','CS','RST','WE'] if pin.startswith(prefix))
                result['ebr:setup_'+group] += count
            else:raise ValueError(key)
        else: raise ValueError(key)
    return result


def fit(path, output):
    data = json.loads(path.read_text()); rows = data['rows']
    for row in rows:
        row['projected'] = [project(p) for p in row['paths']]
        # PLL-output-to-data routes have a distinct source reference. These rows
        # are retained for later clock calibration, not treated as FF clk-to-Q.
        row['fit_eligible'] = row.get('kind') != 'clock_reference'
    train = [r for r in rows if not r['holdout'] and r['fit_eligible']]
    names = sorted({n for r in train for p in r['projected'] for n in p})
    # A fixed gauge: setup aperture assigned 200 ps (one phase quantum rounded
    # down), remainder in effective clkQ. This is not a standalone setup claim.
    fixed = {'ff:setup_data':200.0}
    names = [n for n in names if n not in fixed]
    def matrix(row):
        return np.array([[p.get(n,0) for n in names] for p in row['projected']],dtype=float)
    for row in rows:
        row['matrix'] = matrix(row)
        row['fixed'] = np.array([sum(p.get(n,0)*v for n,v in fixed.items()) for p in row['projected']])
        row['scale'] = sum(row['paths'][0].get('arc:TRELLIS_COMB:'+pin+':F',0) for pin in 'ABCD') if row['measurement']=='loop_average' else 1
        row['target'] = row.get('observed_ps', sum(row.get('observed_interval_ps',[0,0]))/2)
    def predict(row, coef): return float(np.max(row['matrix']@coef+row['fixed']))
    def residual(coef): return [(predict(r,coef)-r['target'])/r['scale'] for r in train]
    result = least_squares(residual, np.full(len(names),50.0), bounds=(0,np.inf), max_nfev=10000,
                           ftol=1e-11, xtol=1e-11, gtol=1e-11)
    coefficients = dict(zip(names,map(float,result.x))); coefficients.update(fixed)
    records=[]
    for r in rows:
        pred=predict(r,result.x)
        records.append(dict(image=r['image'],probe=r['probe'],holdout=r['holdout'],fit_eligible=r['fit_eligible'],
                            observed_ps=r['target'],predicted_ps=pred,error_ps=pred-r['target'],
                            relative_error=pred/r['target']-1 if r['target'] else None,
                            measurement=r['measurement']))
    sv=np.linalg.svd(result.jac,compute_uv=False)
    report=dict(schema=1,kind='unqualified_structural_joint_sta_candidate',sta_qualified=False,
                input_sha256={str(path.resolve()):sha(path)},coefficients_ps=coefficients,
                gauge='Fixed access and driver delay absorbed into cell arcs; FF setup data assigned 200 ps, remainder effective clkQ.',
                legacy_delay_labels_used=False,optimizer_success=bool(result.success),
                jacobian_rank=int(np.sum(sv>1e-7)),free_parameters=len(names),singular_values=list(map(float,sv)),
                fit_rmse_ps=float(np.sqrt(np.mean(np.square(result.fun)))),rows=records)
    output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k!='rows'},indent=2))


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('features',type=Path);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();fit(args.features,args.output)
