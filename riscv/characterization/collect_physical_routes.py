"""Collect exact measured and CPU routing identities for timing replacement.

Coordinates and PIP classes are structural features; no STA delays are training
labels. Paths remain associated with their original measured image hashes.
"""
import argparse
import json
from pathlib import Path
import re
from phase_sweep import ROOT, sha

PIP=re.compile(r'X(\d+)/Y(\d+)/(-?\d+)_(-?\d+)_(.+)->(-?\d+)_(-?\d+)_(.+)')

def pair(name):
    m=PIP.fullmatch(name)
    if not m: raise ValueError('Unrecognized physical PIP: '+name)
    x,y,sx,sy,a,tx,ty,b=m.groups()
    return (f'R{int(y)+int(sy)}C{int(x)+int(sx)}/{a}',f'R{int(y)+int(ty)}C{int(x)+int(tx)}/{b}')

def collect(out):
    out.mkdir(parents=True,exist_ok=True)
    requests=set(); images=[]
    roots=[ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay'/f'registered-{pin}-{axis}' for pin in 'ABCD' for axis in 'xy']
    roots.append(ROOT/'toolchain/measurements/2026-09-22-ecp5-phase-aperture/aperture-01')
    for root in roots:
        module=json.loads((root/'routed.json').read_text())['modules']['top'];nets={}
        for name,net in module['netnames'].items():
            routing=net.get('attributes',{}).get('ROUTING','').strip()
            if not routing:continue
            fields=routing.split(';');assert len(fields)%3==0
            edges=[pair(fields[i+1]) for i in range(0,len(fields),3) if fields[i+1]]
            requests.update(edges);nets[name]=dict(bits=net['bits'],edges=edges)
        image=dict(root=str(root),routed_sha256=sha(root/'routed.json'),cells=module['cells'],nets=nets)
        dest=out/(root.name+'-physical.json');dest.write_text(json.dumps(image,separators=(',',':'))+'\n')
        images.append(dict(file=str(dest),sha256=sha(dest)))
    cpu=ROOT/'riscv/build/atomic-120.texo.json';cp=json.loads(cpu.read_text())
    cpu_pairs={tuple((p['from'],p['to'])) for net in cp['routes'] for p in net['pips']}
    requests.update(cpu_pairs)
    (out/'requested-pairs.json').write_text(json.dumps(sorted(requests),separators=(',',':'))+'\n')
    (out/'cpu-pairs.json').write_text(json.dumps(sorted(cpu_pairs),separators=(',',':'))+'\n')
    (out/'inputs.json').write_text(json.dumps(dict(measured_images=images,cpu_checkpoint=str(cpu),cpu_checkpoint_sha256=sha(cpu),cpu_physical_arcs=len(cpu_pairs),requested_physical_arcs=len(requests)),indent=2)+'\n')
    print('Physical pairs:',len(requests),'CPU:',len(cpu_pairs))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);a=p.parse_args();collect(a.output.resolve())
