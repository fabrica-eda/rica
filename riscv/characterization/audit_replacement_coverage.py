"""Audit structural timing coverage; never treat a routed observer as a sample."""
import argparse
import collections
import json
from pathlib import Path
import re


def wire(cell,pin):
    x,y,sl,index=re.fullmatch(r'X(\d+)/Y(\d+)/SLICE([A-D])\.K([01])',cell['attributes']['NEXTPNR_BEL']).groups()
    return f'R{y}C{x}/{pin}{2*(ord(sl)-ord("A"))+int(index)}_SLICE'


def audit(root):
    requests=json.loads((root/'requested-pairs.json').read_text())
    surface=json.loads((root/'physical-surface.json').read_text())
    assert surface['matched']==len(requests)
    mapping={tuple(pair):matches for pair,matches in zip(requests,surface['matches'])}
    def classes(edges):
        result=collections.Counter()
        for edge in edges:
            values={m['class'] for m in mapping[tuple(edge)]}
            assert len(values)==1,('ambiguous PIP class',edge,values)
            result.update(values)
        return result
    cpu=classes(json.loads((root/'cpu-pairs.json').read_text()))
    upper=collections.Counter();ro=collections.Counter();rows=[]
    for f in sorted(root.glob('*-physical.json')):
        image=json.loads(f.read_text())
        for n in image['nets'].values():upper.update(classes(n['edges']))
        if not f.name.startswith('registered-'):continue
        by_bit={tuple(n['bits']):n for n in image['nets'].values()}
        probes=json.loads((Path(image['root'])/'probes.json').read_text())
        for probe in probes:
            counts=collections.Counter()
            for i in range(probe['stages']):
                source=image['cells'][f"ring_{probe['id']}_{i}"]
                sink=image['cells'][f"ring_{probe['id']}_{(i+1)%probe['stages']}"]
                assert source['connections']['F']==sink['connections'][probe['input_pin']]
                net=by_bit[tuple(source['connections']['F'])]
                parents=collections.defaultdict(set)
                for a,b in net['edges']:parents[b].add(a)
                current=wire(sink,probe['input_pin']);start=wire(source,'F');edges=[];seen=set()
                while current!=start:
                    assert current not in seen;seen.add(current)
                    assert len(parents[current])==1,(current,parents[current])
                    prev=next(iter(parents[current]));edges.append((prev,current));current=prev
                counts.update(classes(edges))
            ro.update(counts)
            rows.append(dict(image=f.name,probe=probe['id'],holdout=probe['holdout'],pip_counts=dict(sorted(counts.items()))))
    report=dict(cpu_pip_class_count=len(cpu),ro_measured_pip_class_count=len(ro),
        cpu_classes_absent_even_from_all_measurement_routes=sorted(set(cpu)-set(upper)),
        cpu_classes_absent_from_measured_ro_paths=sorted(set(cpu)-set(ro)),
        all_measurement_route_union_is_not_measured_coverage=True,
        missing_cell_families=['TRELLIS_CARRY0','TRELLIS_CARRY1','TRELLIS_PFUMX','TRELLIS_L6MUX21'],
        independent_per_class_sta_replacement_qualified=False,
        reason='Missing sensitized cell families, insufficient independent path equations, and no independent clkQ/setup/hold decomposition. No fabricated replacement table is emitted.',
        ro_rows=rows)
    (root/'coverage-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ['ro_rows','cpu_classes_absent_from_measured_ro_paths']},indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('root',type=Path);audit(p.parse_args().root)
