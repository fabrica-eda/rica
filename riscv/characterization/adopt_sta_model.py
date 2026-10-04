"""Adopt only an already validated complete STA implementation."""
import argparse
import json
from pathlib import Path
import shutil
from phase_sweep import ROOT,sha
from validate_sta_model import validate

WRAPPER='''#!/usr/bin/env python3
"""Rica's qualified measured placement and full STA profile; no fallback."""
import hashlib
import json
import os
from pathlib import Path
import sys

root=Path(__file__).resolve().parents[2]
profile=json.loads((root/'riscv/qualified-measured-sta.json').read_text())
def checked(key):
    path=root/profile[key]
    if hashlib.sha256(path.read_bytes()).hexdigest()!=profile[key+'_sha256']:
        raise SystemExit('Qualified '+key+' changed; requalify the profile explicitly.')
    return path
binary=checked('binary')
args=sys.argv[1:]
def supplied(flag):return any(a==flag or a.startswith(flag+'=') for a in args)
if args and args[0]=='pnr':
    for flag,key in [('--measured-placement-model','placement_model'),('--measured-timing-library','timing_library')]:
        if not supplied(flag):args += [flag,str(checked(key))]
    for flag in ['--setup-uncertainty-ps','--hold-uncertainty-ps']:
        if not supplied(flag):args += [flag,str(profile['setup_hold_guard_ps'])]
os.execv(binary,[str(binary),*args])
'''


def adopt(root,run):
    root=root.resolve();validate(root,run)
    validation=json.loads((root/'validation.json').read_text())
    assert validation['passed'] and validation['sta_table_replaced']
    directory=root/run;app=json.loads((directory/'application.json').read_text())
    checkpoint=json.loads((directory/'result.texo.json').read_text())
    timing_library=Path(checkpoint['target']['measured_timing']['path'])
    assert sha(timing_library)==validation['timing_library_sha256']
    bit_source=directory/'qualification/rv64-a-measured-sta-124p8.bit'
    bit=ROOT/'riscv/build/riscv-hardware-compact-atomic-measured-sta-124p8-20260922.bit'
    shutil.copyfile(bit_source,bit);assert sha(bit)==validation['bitstream_sha256']
    profile=dict(schema=1,device='LFE5UM5G-85F',package='CABGA381',speed_grade='8_5G',
        cpu_hz=124800000,memory_hz=62400000,guessed_placement_predictor_enabled=False,
        sta_table_replaced=True,legacy_table_fallback=False,hardware_tests=267,
        setup_hold_guard_ps=validation['setup_hold_guard_ps'],setup_slack_ps=validation['setup_slack_ps'],hold_slack_ps=validation['hold_slack_ps'])
    for key,path in [('binary',Path(app['command'][0])),
                     ('placement_model',ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay/measured-model.json'),
                     ('timing_library',timing_library),('checkpoint',directory/'result.texo.json'),
                     ('bitstream',bit),('validation',root/'validation.json')]:
        profile[key]=str(path.relative_to(ROOT));profile[key+'_sha256']=sha(path)
    target=ROOT/'riscv/qualified-measured-sta.json';temp=target.with_suffix('.json.tmp')
    temp.write_text(json.dumps(profile,indent=2)+'\n');temp.replace(target)
    wrapper=ROOT/'riscv/tools/texo-measured';temp=wrapper.with_suffix('.tmp');temp.write_text(WRAPPER);temp.chmod(0o755);temp.replace(wrapper)
    print(json.dumps(profile,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('root',type=Path);p.add_argument('--run',required=True);a=p.parse_args();adopt(a.root,a.run)
