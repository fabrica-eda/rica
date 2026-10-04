"""Recheck the entire measured STA adoption, including board qualification."""
import argparse
import json
from pathlib import Path
import re
from phase_sweep import ROOT,sha


def validate(root, run_name):
    root=root.resolve();run=root/run_name;q=run/'qualification'
    cp_path=run/'result.texo.json';cp=json.loads(cp_path.read_text())
    app=json.loads((run/'application.json').read_text());qualification=json.loads((q/'qualification.json').read_text())
    assert app['exit_code']==0 and app['sta_table_replaced'] and qualification['qualified']
    assert qualification['hardware_tests']==267 and qualification['console_bytes']==686
    assert len(qualification['counter_comparisons'])==266
    assert all(abs(c['mcycle_delta'])<=1 and c['minstret_delta']==0 for c in qualification['counter_comparisons'])
    assert qualification['checkpoint_sha256']==sha(cp_path)
    assert qualification['application_sha256']==sha(run/'application.json')
    assert qualification['bitstream_sha256']==sha(q/'rv64-a-measured-sta-124p8.bit')
    for step in qualification['runs']:
        assert step['exit_code']==0
        assert step['log_sha256']==sha(q/(step['name']+'.log'))
    if 'orchestrator_sha256' in qualification:
        from qualify_sta_model import is_transport_failure
        assert qualification['orchestrator_sha256']==sha(Path(__file__).with_name('qualify_sta_model.py'))
        for attempt in qualification['failed_transport_attempts']:
            logfile=q/attempt['log_file']
            assert attempt['exit_code']!=0 and sha(logfile)==attempt['log_sha256']
            assert is_transport_failure(logfile.read_text())
        for name,assembly in qualification['assembled_logs'].items():
            assert sha(q/name)==assembly['sha256']
            assert (q/name).read_bytes()==b''.join((q/part).read_bytes() for part in assembly['parts'])
    for name,digest in app['input_sha256'].items():assert sha(name)==digest,name
    for name,digest in qualification['elf_sha256'].items():assert sha(name)==digest,name
    measured=cp['target']['measured_placement'];timing_model=cp['target']['measured_timing']
    assert not measured['guessed_placement_predictor_enabled'] and measured['sta_model_replaced']
    assert measured['model_sha256']==app['model_sha256']==sha(measured['model_path'])
    assert not timing_model['legacy_table_fallback']
    assert timing_model.get('lut_input_timing')=='routed_physical_pin'
    assert timing_model['sha256']==app['timing_library_sha256']==qualification['timing_library_sha256']==sha(timing_model['path'])
    library=json.loads(Path(timing_model['path']).read_text())
    for model in [library,json.loads(Path(measured['model_path']).read_text())]:
        for name,digest in model['input_sha256'].items():assert sha(name)==digest,name
    assert len(library['entry_evidence'])==timing_model['entries']==1552
    assert library['qualification']['heldout_passed'] and library['qualification']['no_legacy_delay_labels']
    assert len(library['removed_structural_non_arcs'])==342
    baseline=json.loads((ROOT/'riscv/build/atomic-120.texo.json').read_text())
    for key in ['design','primitive_metadata','absorbed_inputs']:
        assert cp[key]==baseline[key],key
    connectivity=lambda c: sorted((n['net_id'],n['driver_cell_id'],n['driver_pin_id'],n['sink_cell_id'],n['sink_pin_id']) for n in c['timing']['net_delays'])
    assert connectivity(cp)==connectivity(baseline),'logical net connectivity changed'
    # Hold repair can change physical DI/M bindings, but not the logical graph.
    assert sorted(c['period_ps'] for c in cp['packing']['generated_clock_periods_ps'])==[8013,16026,166666]
    assert cp['packing']['generated_clock_periods_ps']==baseline['packing']['generated_clock_periods_ps']
    t=cp['timing'];assert t['met_timing'] and t['meets_timing_closure']
    guard=library['qualification']['required_setup_hold_guard_ps']
    assert t['setup_uncertainty_ps']==t['hold_uncertainty_ps']==guard==app['setup_hold_guard_ps']
    assert t['worst_slack_ps']>=0 and t['worst_hold_slack_ps']>=0
    jtck=json.loads((q/'jtck.json').read_text());assert jtck['jtck_nominal_slacks_and_edges_match']
    assert jtck['checkpoint_sha256']==sha(cp_path)
    assert sha(jtck['supplemental_command'][0])==jtck['supplemental_binary_sha256']
    assert sha(q/'jtck.supplemental.json')==jtck['supplemental_report_sha256']
    sources=json.loads((root/'source-sha256.json').read_text())
    for name,digest in sources.items():assert sha(name)==digest,name
    tests=(root/'tests-full.log').read_text();assert 'FAILED' not in tests
    test_count=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',tests)));assert test_count>=139
    scripts={ROOT/'riscv/characterization'/name for name in ['run_sta_model.py','qualify_sta_model.py','validate_sta_model.py','adopt_sta_model.py']}
    scripts.update(Path(name) for name in library['input_sha256'] if name.endswith('.py'))
    result=dict(passed=True,native_placement_model_replaced=True,sta_table_replaced=True,legacy_table_fallback=False,
        cpu_hz=124800000,memory_hz=62400000,hardware_tests=267,console_bytes=686,rust_tests_passed=test_count,
        sta_entries=1552,measurement_samples=len(library['qualification']['validated_samples']),
        heldout_samples=sum(m['holdout'] for m in library['qualification']['sample_metadata'].values()),
        setup_hold_guard_ps=guard,setup_slack_ps=t['worst_slack_ps'],hold_slack_ps=t['worst_hold_slack_ps'],
        checkpoint=str(cp_path),checkpoint_sha256=sha(cp_path),bitstream_sha256=qualification['bitstream_sha256'],
        placement_model_sha256=measured['model_sha256'],timing_library_sha256=timing_model['sha256'],
        qualification_sha256=sha(q/'qualification.json'),source_manifest_sha256=sha(root/'source-sha256.json'),
        scripts_sha256={str(p):sha(p) for p in sorted(scripts)},
        scope=library['qualification_scope'],independent_pin_parameters_identified=False,independent_voltage_sweep=False)
    (root/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('root',type=Path);p.add_argument('--run',default='native-sta-01');a=p.parse_args();validate(a.root,a.run)
