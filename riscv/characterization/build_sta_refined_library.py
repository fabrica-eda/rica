"""Serialize the complete jointly measured STA table and bind its evidence.

The old table supplies names only. Its numerical delays are never accessed.
Unsupported entries fail construction; there is no legacy-value fallback.
"""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
from fit_sta_dsp_distance_experiment import entry, features
from fit_sta_crossvalidated import calibration_family
from phase_sweep import ROOT, sha


def absent_multiplier_arc(cell, source, output):
    if cell!='MULT18X18D:REGS=NONE' or not output.startswith('P'):return False
    bit=int(output[1:])
    if source in ['SIGNEDA','SIGNEDB']:return bit<18
    return source[0] in 'AB' and bit<int(source[1:])


def surface(source):
    old=json.loads(source.read_text())['timing_surface_for_coverage_only']
    result=dict(name=old['name'],pip_classes=sorted(old['pip_classes']),cells=[]);removed=[]
    for cell in old['cells']:
        arcs=[]
        for arc in cell['arcs']:
            key=(cell['cell_type'],arc['from_pin'],arc['to_pin'])
            if absent_multiplier_arc(*key):removed.append(list(key))
            else:arcs.append([arc['from_pin'],arc['to_pin']])
        result['cells'].append(dict(cell_type=cell['cell_type'],arcs=arcs,
                                   setup_holds=[[h['signal_pin'],h['clock_pin']] for h in cell['setup_holds']]))
    return result,removed


def construct(feature_file,corner_file,identity_dir,surface_file,output,refinement_plan):
    data=json.loads(feature_file.read_text());candidate=json.loads(corner_file.read_text())
    assert candidate['heldout_passed'] and candidate['all_path_checks_passed']
    assert not candidate['legacy_delay_labels_used'] and not data['legacy_delay_values_used']
    assert candidate['input_sha256'][str(feature_file.resolve())]==sha(feature_file)
    plan=json.loads(refinement_plan.read_text())
    assert plan['before_first_hardware_sample']
    fresh=[c for c in candidate['checks'] if c['image'] in plan['datasets'] and c['holdout']]
    assert len(fresh)==plan['fresh_holdout_probes']==38
    assert all(c['min_passed'] and c['max_passed'] for c in fresh)
    for name,digest in plan['planned_analysis_sha256'].items():assert sha(name)==digest,name
    assert candidate['kind']=='experimental_interval_ring_dsp_distance_candidate'
    assert candidate['fresh_heldout_passed'] and candidate['fresh_heldout_samples']==38
    assert plan['guard_protocol_amendment']['before_first_fresh_model_validation']
    training={(r['image'],r['probe']) for r in data['rows'] if not r['holdout'] and r.get('kind')!='clock_reference' and not r.get('joint_controls') and r['measurement']!='loop_average'}
    assert {(f['image'],f['probe']) for f in candidate['training_folds']}==training
    row_families={(r['image'],r['probe']):calibration_family(r) for r in data['rows'] if (r['image'],r['probe']) in training}
    for fold in candidate['training_folds']:
        assert fold['family']==row_families[fold['image'],fold['probe']]
        residual=candidate['corners']['residual_bounds_ps'].get(fold['family'],dict(lower_ps=0,upper_ps=0))
        assert candidate['setup_hold_guard_ps']+residual['lower_ps']>=fold['lower_deficit_ps']
        assert candidate['setup_hold_guard_ps']+residual['upper_ps']>=fold['upper_deficit_ps']
    models=candidate['corners'];guard=candidate['setup_hold_guard_ps']
    shape,removed=surface(surface_file)
    kinds={r.get('kind') for r in data['rows']}
    required={'ff_ce_independent','ebr_oce_A','ebr_oce_B','ebr_ce_A','ebr_ce_B','ebr_registered_output'}
    required|={f'ebr_cs{j}_{side}' for j in range(3) for side in 'AB'}
    assert required<=kinds,('missing sensitized cell families',sorted(required-kinds))
    inputs={};visited=set()
    def bind(path,expected=None):
        path=Path(path).resolve();digest=sha(path)
        if expected is not None:assert digest==expected,('changed evidence',str(path))
        inputs[str(path)]=digest
        if path in visited:return
        visited.add(path)
        if path.suffix!='.json' or path.stat().st_size>20_000_000:return
        value=json.loads(path.read_text())
        if not isinstance(value,dict):return
        for name,h in value.get('input_sha256',{}).items():bind(name,h)
        if path.name=='build.json':
            for name,h in value.get('sha256',{}).items():
                target=Path(name)
                if not target.is_absolute():target=path.parent/target
                bind(target,h)
        if path.name=='tools-sha256.json':
            for name,h in value.items():bind(name,h)
    for p in [feature_file,corner_file,identity_dir/'requests.json',identity_dir/'identities.json',surface_file,refinement_plan]:bind(p)
    for name,digest in plan['planned_analysis_sha256'].items():bind(name,digest)
    roots={}
    for file in identity_dir.glob('*-physical.json'):
        image=json.loads(file.read_text());roots[Path(image['root']).name]=Path(image['root'])
    passed={(r['image'],r['probe']) for r in candidate['checks'] if r['min_passed'] and r['max_passed']}
    samples={};direct=defaultdict(set);basis=defaultdict(set);rows=[];sample_metadata={}
    for row in data['rows']:
        if (row['image'],row['probe']) not in passed:continue
        root=roots[row['image']];sample=f"{row['image']}:probe-{row['probe']}"
        if row['measurement']=='loop_average':
            evidence=root/'raw-counts.json'
            for name in ['raw-counts.json','probes.json','routed.json','measure.bit']:bind(root/name)
        elif row['measurement']=='clock_loop_period':evidence=root/'clock-loop-model.json'
        else:
            evidence=root/'aperture-model.json'
            probe=next(p for p in json.loads(evidence.read_text())['probes'] if p['id']==row['probe'])
            assert probe['usable_boundary']
            # Both sweep directions must support each adopted boundary. One
            # coarse point is not sufficient evidence for a final library.
            for polarity in probe.get('measured_polarities',['rising','falling']):
                edge=probe[polarity]
                for boundary in [edge['last_all_old_ps'],edge['first_all_correct_ps']]:
                    point=next(o for o in edge['observations'] if o['phase_ps']==boundary)
                    assert len(point['errors'])>=2,(sample,polarity,boundary,'needs repeated boundary')
        bind(evidence);samples[sample]=str(evidence.resolve());rows.append((sample,row))
        sample_metadata[sample]=dict(holdout=row['holdout'],measurement=row['measurement'])
        for path in row['paths']:
            for key,count in path.items():
                if count:direct[key].add(sample)
            if not row['holdout']:
                for corner in ['min','max']:
                    if row['measurement']=='clock_loop_period':continue
                    for name,count in features(path,corner).items():
                        if count:basis[name].add(sample)
    assert samples
    training=[name for name,m in sample_metadata.items() if not m['holdout']]
    evidence={};projections={};numbers={}
    def support(key):
        refs=set(direct[key]);projection={}
        if key!='arc:DCCA:CLKI:CLKO':
            for corner in ['min','max']:
                projected=features({key:1},corner);projection[corner]=projected
                for name in projected:
                    assert basis[name],('unsupported shared feature',key,name)
                    refs.update(basis[name])
        if not refs:
            assert key.startswith('pip:'),('no measured support',key)
            # Zero-valued fixed access/alias entries are an explicit reference
            # choice. Their delay is in the fitted input/output cell arcs.
            # They are not claimed as independent zero-delay measurements.
            refs.update(training)
            projection['reference']='fixed access absorbed into adjacent cell arcs; global backbone in common clock latency'
        evidence[key]=sorted(refs);projections[key]=projection
    def range_for(key):
        low,high=entry(key,models)
        support(key)
        if key.startswith('check:'):
            result=dict(setup=dict(min_ps=0,max_ps=math.ceil(high)),hold=dict(min_ps=0,max_ps=math.ceil(-low)))
            numbers[key]=(-result['hold']['max_ps'],result['setup']['max_ps'])
            return result
        low=max(0,math.floor(low));high=max(low,math.ceil(high));numbers[key]=(low,high)
        return dict(min_ps=low,max_ps=high)
    timing=dict(name=shape['name'],pip_classes={},cells=[])
    for category in shape['pip_classes']:
        key='pip:'+category;base=range_for(key)
        fanout_key='fanout:'+category
        low,high=entry(fanout_key,models)
        fanout=dict(min_ps=max(0,math.floor(low)),max_ps=max(0,math.ceil(high)))
        numbers[fanout_key]=(fanout['min_ps'],fanout['max_ps'])
        for corners in [base,fanout]:corners['typ_ps']=(corners['min_ps']+corners['max_ps'])//2
        timing['pip_classes'][category]=dict(base=base,fanout_adder=fanout)
    for cell in shape['cells']:
        record=dict(cell_type=cell['cell_type'],arcs=[],setup_holds=[])
        for a,b in cell['arcs']:
            key=f'arc:{cell["cell_type"]}:{a}:{b}'
            record['arcs'].append(dict(from_pin=a,to_pin=b,delay=range_for(key)))
        for signal,clock in cell['setup_holds']:
            key=f'check:{cell["cell_type"]}:{signal}:{clock}'
            record['setup_holds'].append(dict(signal_pin=signal,clock_pin=clock,**range_for(key)))
        timing['cells'].append(record)
    rounded_checks=[]
    for sample,row in rows:
        paths=[tuple(sum(numbers[key][corner]*count for key,count in p.items()) for corner in [0,1]) for p in row['paths']]
        low=min(p[0] for p in paths);high=max(p[1] for p in paths)
        if row['measurement']=='loop_average':observed=[row['observed_ps']]*2;uncertainty=0
        else:observed=row['observed_interval_ps'];uncertainty=guard*(2 if row['measurement']=='clock_loop_period' else 1)
        assert low-uncertainty<=observed[0] and high+uncertainty>=observed[1],('rounded table failed',sample)
        rounded_checks.append(dict(sample=sample,holdout=row['holdout'],min_ps=low,max_ps=high,guard_ps=uncertainty,observed_interval_ps=observed))
    for name in ['build_sta_refined_library.py','fit_sta_crossvalidated.py','fit_sta_dsp_distance_experiment.py','fit_sta_candidate.py','extract_sta_paths_observer.py','collect_sta_identities.py']:
        bind(Path(__file__).parent/name)
    result=dict(schema=1,kind='measured_joint_cell_route_sta_library',device='LFE5UM5G-85F',package='CABGA381',
                timing=timing,entry_evidence=evidence,entry_projection=projections,input_sha256=inputs,
                qualification=dict(heldout_passed=True,fresh_heldout_passed=True,fresh_heldout_samples=len(fresh),refinement_plan_sha256=sha(refinement_plan),no_legacy_delay_labels=True,independent_voltage_sweep=False,
                                   temperature_fraction=.2,required_setup_hold_guard_ps=guard,validated_samples=samples,
                                   sample_metadata=sample_metadata,serialized_table_checks=rounded_checks),
                removed_structural_non_arcs=removed,independent_pin_parameters_identified=False,
                parameter_gauge=candidate['gauge'],clock_policy=candidate['clock_policy'],
                family_residual_bounds_ps=models['residual_bounds_ps'],residual_calibration_policy=candidate['guard_method'],
                reference_hz_nominal=12000000,reference_externally_calibrated=False,
                qualification_scope='Measured path bounds on the connected board; CPU implementation qualification is separate.')
    output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(dict(entries=len(evidence),samples=len(samples),input_files=len(inputs),removed_non_arcs=len(removed),guard_ps=guard,sha256=sha(output)),indent=2))


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--features',type=Path,required=True);parser.add_argument('--corners',type=Path,required=True)
    parser.add_argument('--identities',type=Path,required=True);parser.add_argument('--surface',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--refinement-plan',type=Path,required=True)
    args=parser.parse_args();construct(args.features.resolve(),args.corners.resolve(),args.identities.resolve(),args.surface.resolve(),args.output.resolve(),args.refinement_plan.resolve())
