"""Separate the measured model's lower/upper residuals from its global guard.

Read-only audit: neither refits parameters nor chooses a guard from holdout data.
A smaller training residual is not permission to discard a holdout violation.
"""
import argparse
from collections import defaultdict
import json
from pathlib import Path
from phase_sweep import sha


def audit(features, corners, output):
    data=json.loads(features.read_text());model=json.loads(corners.read_text())
    assert model['input_sha256'][str(features.resolve())]==sha(features)
    rows={(r['image'],r['probe']):r for r in data['rows']}
    groups=defaultdict(list)
    for check in model['checks']:
        row=rows[check['image'],check['probe']]
        if row['measurement']=='loop_average':continue
        traversals=2 if row['measurement']=='clock_loop_period' else 1
        low,high=check['observed_interval_ps']
        lower=max(0.,(check['predicted_min_ps']-low)/traversals)
        upper=max(0.,(high-check['predicted_max_ps'])/traversals)
        groups['holdout' if row['holdout'] else 'training'].append(dict(
            image=row['image'],probe=row['probe'],kind=row.get('kind',row['measurement']),
            lower_bound_deficit_ps=lower,upper_bound_deficit_ps=upper))
    families={}
    for partition,checks in groups.items():
        by_kind=defaultdict(list)
        for check in checks:by_kind[check['kind']].append(check)
        families[partition]={kind:dict(samples=len(items),
            lower_bound_deficit_ps=max(c['lower_bound_deficit_ps'] for c in items),
            upper_bound_deficit_ps=max(c['upper_bound_deficit_ps'] for c in items))
            for kind,items in sorted(by_kind.items())}
    partitions={}
    for name,checks in groups.items():
        partitions[name]=dict(samples=len(checks),
            lower_bound_deficit_ps=max(c['lower_bound_deficit_ps'] for c in checks),
            upper_bound_deficit_ps=max(c['upper_bound_deficit_ps'] for c in checks),
            worst_lower=sorted(checks,key=lambda c:c['lower_bound_deficit_ps'],reverse=True)[:10],
            worst_upper=sorted(checks,key=lambda c:c['upper_bound_deficit_ps'],reverse=True)[:10])
    result=dict(input_sha256={str(p.resolve()):sha(p) for p in [features,corners,Path(__file__)]},
        declared_guard_ps=model['setup_hold_guard_ps'],temperature_fraction=model['temperature_fraction'],
        partitions=partitions,families=families,model_or_guard_changed=False,
        interpretation='Observed interval endpoints already retain phase quantization. The current global guard also covers model residuals; a smaller training residual does not establish holdout coverage. Family-specific and asymmetric residual models require separate validation.')
    output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:{s:v for s,v in p.items() if not s.startswith('worst_')} for k,p in partitions.items()},indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--features',type=Path,required=True);p.add_argument('--corners',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();audit(a.features,a.corners,a.output)
