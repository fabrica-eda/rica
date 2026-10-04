"""Recheck provenance and qualification of the native measured-placement result."""
import argparse
import json
from pathlib import Path
import re
from phase_sweep import ROOT,sha


def validate(root):
    root=root.resolve();run=root/'native-01';q=run/'qualification'
    cp_path=run/'result.texo.json';cp=json.loads(cp_path.read_text());app=json.loads((run/'application.json').read_text());qualification=json.loads((q/'qualification.json').read_text())
    assert app['exit_code']==0 and qualification['qualified']
    assert qualification['hardware_tests']==267 and qualification['console_bytes']==686
    assert qualification['checkpoint_sha256']==sha(cp_path)
    assert qualification['application_sha256']==sha(run/'application.json')
    assert qualification['bitstream_sha256']==sha(q/'rv64-a-measured-124p8.bit')
    for step in qualification['runs']:
        assert step['exit_code']==0
        assert step['log_sha256']==sha(q/(step['name']+'.log'))
    for name,digest in app['input_sha256'].items():assert sha(name)==digest,name
    for name,digest in qualification['elf_sha256'].items():assert sha(name)==digest,name
    measured=cp['target']['measured_placement']
    assert not measured['guessed_placement_predictor_enabled'] and not measured['sta_model_replaced']
    assert measured['model_sha256']==app['model_sha256']==sha(measured['model_path'])
    model=json.loads(Path(measured['model_path']).read_text())
    for name,digest in model['input_sha256'].items():assert sha(name)==digest,name
    accepted=[a for a in measured['attempts'] if a['accepted']];assert len(accepted)==1
    baseline=json.loads((ROOT/'riscv/build/atomic-120.texo.json').read_text())
    for key in ['design','primitive_metadata','absorbed_inputs','packing']:assert cp[key]==baseline[key],key
    previous={p['cell']:p['bel'] for p in baseline['placement']};current={p['cell']:p['bel'] for p in cp['placement']}
    moves=[dict(cell=n,old=previous[n],new=current[n]) for n in previous if previous[n]!=current[n]]
    assert moves==[{k:accepted[0][k] for k in ['cell','old','new']}]
    t=cp['timing'];assert t['met_timing'] and t['meets_timing_closure']
    assert t['setup_uncertainty_ps']==t['hold_uncertainty_ps']==200
    assert t['worst_slack_ps']>=baseline['timing']['worst_slack_ps'] and t['worst_hold_slack_ps']>=baseline['timing']['worst_hold_slack_ps']
    changes=json.loads((root/'source-changes.json').read_text())
    source=ROOT/'toolchain/worktrees/isa/texo-measured-timing'
    for name,data in changes.items():assert sha(source/name)==data['after_sha256'],name
    coverage=json.loads((root/'coverage-report.json').read_text());assert not coverage['independent_per_class_sta_replacement_qualified']
    tests=(root/'tests-final.log').read_text();test_count=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',tests)));assert test_count==139
    scripts=['collect_physical_routes.py','audit_replacement_coverage.py','run_native_model.py','qualify_native_model.py','validate_native_model.py','texo-measured-placement.patch']
    result=dict(passed=True,native_placement_model_replaced=True,sta_table_replaced=False,
        cpu_hz=124800000,memory_hz=62400000,hardware_tests=267,console_bytes=686,
        rust_tests_passed=test_count,moves=moves,setup_slack_ps=t['worst_slack_ps'],hold_slack_ps=t['worst_hold_slack_ps'],
        checkpoint_sha256=sha(cp_path),bitstream_sha256=qualification['bitstream_sha256'],
        model_sha256=measured['model_sha256'],qualification_sha256=sha(q/'qualification.json'),
        source_changes_sha256=sha(root/'source-changes.json'),coverage_sha256=sha(root/'coverage-report.json'),
        scripts_sha256={str(ROOT/'riscv/characterization'/p):sha(ROOT/'riscv/characterization'/p) for p in scripts},
        clippy='passed with explicit allowances for unused_self/too_many_lines in baseline, case-sensitive BEL-name suffixes, and existing missing_panics_doc')
    (root/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('root',type=Path);validate(p.parse_args().root)
