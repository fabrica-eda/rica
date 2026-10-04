"""Check endpoint controls and reference-clock paths after packing."""
import argparse
import json
from pathlib import Path
from audit_phase import audit as common_audit
from phase_sweep import sha


def audit(out):
    common_audit(out)
    m = json.loads((out/'routed.json').read_text())['modules']['top']
    cells = m['cells']
    launch = m['netnames']['$glbnet$launch']['bits']
    capture = m['netnames']['$glbnet$capture']['bits']
    rows = []
    for p in json.loads((out/'probes.json').read_text()):
        k = p['id']; kind = p['kind']
        previous = m['netnames']['launch']['bits'] if kind == 'clock_reference' else cells[f'q{k}_ff']['connections']['Q']
        owned = [f'q{k}_ff', f'cap{k}_ff']
        for j in range(p['lut_stages']):
            name = f'path{k}_{j}_lut'; c = cells[name]
            assert c['type'] == 'TRELLIS_COMB'
            assert int(c['parameters']['INITVAL'], 2) == 0xaaaa
            assert c['connections']['A'] == previous
            previous = c['connections']['F']; owned.append(name)
        cap = cells[f'cap{k}_ff']; pins = cap['connections']; params = cap['parameters']
        assert cap['type'] == 'TRELLIS_FF' and pins['CLK'] == capture
        data = next(bits for pin, bits in pins.items() if pin in ['DI', 'M'] and bits)
        if kind in ['clock_reference', 'ff_data']:
            assert data == previous
        elif kind == 'ff_lsr':
            assert pins['LSR'] == previous
            assert params['SRMODE'] == 'LSR_OVER_CE' and params.get('REGSET', 'RESET') == 'RESET'
        elif kind == 'ff_ce_lsr':
            assert pins['CE'] == previous and params['CEMUX'] == 'CE'
            assert params['SRMODE'] == 'LSR_OVER_CE' and params.get('REGSET', 'RESET') == 'RESET'
            inv = cells[f'inv{k}_lut']; owned.append(f'inv{k}_lut')
            assert int(inv['parameters']['INITVAL'], 2) == 0x5555
            assert inv['connections']['A'] == previous and inv['connections']['F'] == pins['LSR']
        else:
            raise ValueError('Unreviewed endpoint family '+kind)
        rows.append(dict(probe=k, kind=kind, physical_cells={n: cells[n]['attributes']['NEXTPNR_BEL'] for n in owned},
                         joint_control_measurement=kind == 'ff_ce_lsr'))
    result = dict(passed=True, probes=rows, input_sha256={str(out/n): sha(out/n) for n in ['measure.v', 'routed.json', 'probes.json']})
    (out/'sensitization-audit.json').write_text(json.dumps(result, indent=2)+'\n')
    print('Endpoint physical audit passed:', len(rows))
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('directory', type=Path)
    audit(parser.parse_args().directory.resolve())
