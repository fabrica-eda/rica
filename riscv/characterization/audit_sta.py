"""Verify sensitization on the *packed* carry/mux measurement netlist."""
import argparse
import json
import re
from pathlib import Path
from phase_sweep import sha
from audit_phase import audit as audit_common


def audit(out):
    audit_common(out)
    m=json.loads((out/'routed.json').read_text())['modules']['top'];cells=m['cells'];drivers={}
    for name,c in cells.items():
        for port,direction in c['port_directions'].items():
            if direction=='output':
                for bit in c['connections'].get(port,[]):
                    assert bit not in drivers,(bit,drivers.get(bit),(name,port))
                    drivers[bit]=(name,port)
    enums={};tile=None
    for line in (out/'measure.config').read_text().splitlines():
        if line.startswith('.tile '):tile=line.split()[1].split(':')[0]
        elif line.startswith('enum: '):
            _,key,value=line.split(maxsplit=2);enums[tile,key]=value
    records=[]
    for p in json.loads((out/'probes.json').read_text()):
        k=p['id'];launch=cells[f'q{k}_ff']['connections']['Q'][0]
        cap=cells[f'cap{k}_ff'];data=next(bits[0] for pin,bits in cap['connections'].items() if pin in ['DI','M'] and bits)
        used=set()
        def evaluate(q):
            cache={launch:q,'0':0,'1':1};active=set()
            def bit_value(bit):
                if bit in cache:return cache[bit]
                assert bit not in active,('combinational cycle',bit)
                active.add(bit);name,port=drivers[bit];c=cells[name];used.add(name);pins=c['connections'];params=c['parameters']
                def pin(n):
                    if pins.get(n):return bit_value(pins[n][0])
                    if n in 'ABCD':
                        match=re.fullmatch(r'X(\d+)/Y(\d+)/(SLICE[A-D])\.K([01])',c['attributes']['NEXTPNR_BEL'])
                        assert match,(name,n)
                        x,y,sl,half=match.groups();setting=enums.get((f'R{y}C{x}',f'{sl}.{n}{half}MUX'))
                        if setting is not None:
                            assert setting in ['0','1'],(name,n,setting)
                            return int(setting)
                        assert params.get('MODE')!='CCU2',('unaccounted carry input',name,n)
                    return 0
                if c['type'] in ['GND','VLO']:value=0
                elif c['type'] in ['VCC','VHI']:value=1
                else:
                    assert c['type']=='TRELLIS_COMB',(name,c['type'],port)
                    init=int(params['INITVAL'],2)
                    address=sum(pin(n)<<i for i,n in enumerate('ABCD'))
                    lut=(init>>address)&1
                    if params.get('MODE')=='CCU2':
                        inject=params.get('CCU2_INJECT1')=='YES';ci=pin('FCI')
                        if port=='F':value=lut^(0 if inject else ci)
                        elif port=='FCO':value=(lut&ci)|((1-lut)&(0 if inject else ((init>>(pin('A')+2*pin('B')))&1)))
                        else:raise ValueError((name,port))
                    elif port=='F':value=lut
                    elif port=='OFX' and pins.get('F1'):value=pin('F1') if pin('M') else lut
                    elif port=='OFX' and pins.get('FXA') and pins.get('FXB'):value=pin('FXB') if pin('M') else pin('FXA')
                    else:raise ValueError((name,port,params,pins))
                active.remove(bit);cache[bit]=value;return value
            return bit_value(data)
        observed=[evaluate(0),evaluate(1)];expected=[1,0] if p['kind']=='carry_cin_sum' else [0,1]
        assert observed==expected,(p,observed,expected)
        if p['kind'].startswith('carry'):
            for j in range(p['stages']):
                for half in [0,1]:
                    c=cells[f'carry{k}_{j}$CCU2_COMB{half}']
                    assert c['parameters']['MODE']=='CCU2'
                    assert c['parameters']['CCU2_INJECT1']==('NO' if p['kind']=='carry_cin_sum' else 'YES')
        elif p['kind'].startswith('pfumx'):
            assert cells[f'wide{k}_b_lut']['connections']['OFX']==[data]
        elif p['kind'].startswith('l6mux'):
            owners=[name for name in used if cells[name]['connections'].get('OFX')==[data]]
            assert len(owners)==1 and cells[owners[0]]['connections'].get('FXA') and cells[owners[0]]['connections'].get('FXB')
        else:raise ValueError('Unreviewed measurement family '+p['kind'])
        records.append(dict(probe=k,kind=p['kind'],truth=observed,physical_cells={n:cells[n]['attributes'].get('NEXTPNR_BEL') for n in sorted(used)}))
    result=dict(passed=True,probes=records,input_sha256={str(out/n):sha(out/n) for n in ['measure.v','routed.json','probes.json']})
    (out/'sensitization-audit.json').write_text(json.dumps(result,indent=2)+'\n');print('Packed sensitization audit passed:',len(records));return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('directory',type=Path);audit(p.parse_args().directory.resolve())
