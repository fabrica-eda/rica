"""Reimplement the CPU with the complete measured STA replacement.

No Python placement proposal or guessed timing predictor is used. Fresh native
routing and STA accept/reject proposals transactionally inside Texo.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
from phase_sweep import ROOT,sha


def run(out,baseline=None,setup_budget=None,library=None):
    out=out.resolve();out.mkdir(parents=True,exist_ok=False)
    old=ROOT/'toolchain/measurements/2026-09-21-rv64-a'
    baseline=(baseline or ROOT/'toolchain/measurements/2026-09-22-ecp5-model-replacement/native-01/result.texo.json').resolve()
    cp=json.loads(baseline.read_text())
    allowed={r['net'] for r in json.loads((old/'atomic-120-warm-routes.json').read_text())}
    routes=[dict(r,allow_reroute=True) for r in cp['routes'] if r['net'] in allowed]
    placement={r['cell']:r['bel'] for r in cp['placement']}
    names={r['cell_id']:r['cell'] for r in cp['placement']}
    pairs={names[r['lut']]:names[r['ff']] for r in cp['packing']['lut_ff_pairs']}
    for name,data in [('placement.json',placement),('routes.json',routes),('pairs.json',pairs)]:
        (out/name).write_text(json.dumps(data,separators=(',',':'))+'\n')
    cmd=json.loads((old/'atomic-120-start.json').read_text())['command']
    cmd[0]=str(ROOT/'target/texo-sta-replacement/release/texo')
    for flag in ['--setup-optimization-budget-seconds']:
        if flag in cmd:i=cmd.index(flag);del cmd[i:i+2]
    if setup_budget is not None:cmd += ['--setup-optimization-budget-seconds',str(setup_budget)]
    for flag,path in [('--output',out/'result.texo.json'),('--initial-placement',out/'placement.json'),('--initial-routes',out/'routes.json'),('--lut-ff-pairs',out/'pairs.json')]:
        cmd[cmd.index(flag)+1]=str(path)
    model=ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay/measured-model.json'
    arch=ROOT/'toolchain/worktrees/dsp/deps/ecp5-dsp-architecture-v6.txdb'
    if '--architecture' in cmd:cmd[cmd.index('--architecture')+1]=str(arch)
    else:cmd+=['--architecture',str(arch)]
    library=(library or ROOT/'toolchain/measurements/2026-09-22-ecp5-sta-replacement/measured-sta-library.json').resolve()
    timing=json.loads(library.read_text());guard=timing['qualification']['required_setup_hold_guard_ps']
    for flag in ['--setup-uncertainty-ps','--hold-uncertainty-ps']:
        if flag in cmd:cmd[cmd.index(flag)+1]=str(guard)
        else:cmd += [flag,str(guard)]
    cmd+=['--measured-placement-model',str(model),'--measured-timing-library',str(library)]
    inputs=[baseline,Path(cmd[0]),model,library,arch,*[out/n for n in ['placement.json','routes.json','pairs.json']]]
    inputs+=list((Path(cmd[2])/'src').glob('*.veryl'))
    inputs+=list(Path(cmd[2]).glob('Veryl.*'))
    inputs += [Path(cmd[cmd.index(f)+1]) for f in ['--lpf','--pll-binding','--clock-constraints','--timing-exceptions'] if f in cmd]
    manifest=dict(command=cmd,model_sha256=sha(model),input_sha256={str(p):sha(p) for p in inputs},
        guessed_placement_predictor_enabled=False,model_used_by='native texo-flow measured_placement',
        sta_model='complete measured joint cell/route library; no legacy numerical timing fallback',sta_table_replaced=True,timing_library_sha256=sha(library),setup_hold_guard_ps=guard)
    (out/'application.json').write_text(json.dumps(manifest,indent=2)+'\n')
    env=os.environ.copy();env.update(TEXO_METRICS='1',TEXO_PNR_METRICS='1');env.pop('TEXO_PLACEMENT_SNAPSHOT',None);env.pop('TEXO_ROUTE_LIMIT',None)
    started=time.time()
    with (out/'pnr.log').open('w') as log:
        result=subprocess.run(cmd,cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT)
    manifest.update(exit_code=result.returncode,seconds=time.time()-started)
    (out/'application.json').write_text(json.dumps(manifest,indent=2)+'\n')
    if result.returncode:raise SystemExit(result.returncode)
    print(out)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--baseline',type=Path);p.add_argument('--setup-budget',type=int);p.add_argument('--library',type=Path);a=p.parse_args();run(a.output,a.baseline,a.setup_budget,a.library)
