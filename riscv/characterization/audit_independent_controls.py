"""Check half-cycle data preparation independently of the enable under test."""
import argparse
import json
from pathlib import Path
from audit_phase import audit as common_audit
from phase_sweep import sha


def steady_state(early, registered_memory):
    # Nonblocking RHS uses pre-edge state (IEEE 1800-2023 4.9.4).
    q = data = memory = output = 0; observations = []
    for cycle in range(100):
        if not early: q ^= 1
        if q: output = memory if registered_memory else data
        memory = data
        if early: q ^= 1
        if cycle > 10: observations.append(output)
        data = q
    assert len(set(observations)) == 1
    return observations[0]


def audit(out):
    common_audit(out)
    assert [steady_state(early, False) for early in [False, True]] == [0, 1]
    assert [steady_state(early, True) for early in [False, True]] == [1, 0]
    m = json.loads((out/'routed.json').read_text())['modules']['top']; cells = m['cells']
    launch = m['netnames']['$glbnet$launch']['bits']; capture = m['netnames']['$glbnet$capture']['bits']; rows = []
    for p in json.loads((out/'probes.json').read_text()):
        k = p['id']; kind = p['kind']; prep = cells[f'ready_data{k}_ff']
        assert prep['type'] == 'TRELLIS_FF' and prep['parameters']['CLKMUX'] == 'INV'
        assert prep['connections']['CLK'] == launch
        prepared = prep['connections']['Q']; previous = cells[f'q{k}_ff']['connections']['Q']
        prep_data=next(v for n,v in prep['connections'].items() if n in ['DI','M'] and v)
        assert prep_data==previous, 'data preparation must follow the same source FF, not an independently initialized oscillator'
        for j in range(p['lut_stages']):
            c = cells[f'path{k}_{j}_lut']; assert c['connections']['A'] == previous
            assert int(c['parameters']['INITVAL'],2) == 0xaaaa
            previous = c['connections']['F']
        if kind == 'ff_ce_independent':
            c = cells[f'cap{k}_ff']; assert c['parameters']['CEMUX'] == 'CE'
            assert c['connections']['CE'] == previous and c['connections']['CLK'] == capture
            data = next(v for n, v in c['connections'].items() if n in ['DI','M'] and v)
            assert data == prepared and not c['connections'].get('LSR')
        else:
            side = kind[-1]; c = cells[f'mem{k}_bram']; assert c['type'] == 'DP16KD'
            mode=kind.split('_')[1]
            assert c['parameters'][f'REGMODE_{side}'] == ('OUTREG' if mode=='oce' else 'NOREG')
            assert c['connections'][f'CLK{side}'] == capture
            pin=f'OCE{side}' if mode=='oce' else (f'CE{side}' if mode=='ce' else f'CS{side}{mode[-1]}')
            assert c['connections'][pin] == previous
            if mode.startswith('cs'):assert c['parameters'][f'CSDECODE_{side}']==f'0b{1<<int(mode[-1]):03b}'
            assert c['connections'][f'AD{side}4'] == prepared
        rows.append(dict(probe=k,kind=kind,data_launch_edge='falling',enable_launch_edge='rising',independent_control=True))
    result=dict(passed=True,probes=rows,input_sha256={str(out/n):sha(out/n) for n in ['measure.v','routed.json','probes.json']})
    (out/'sensitization-audit.json').write_text(json.dumps(result,indent=2)+'\n')
    print('Independent control audit passed:',len(rows))
    return result


if __name__ == '__main__':
    parser=argparse.ArgumentParser();parser.add_argument('directory',type=Path)
    audit(parser.parse_args().directory.resolve())
