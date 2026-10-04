"""Audit physical EBR address/data/control probes and the writeback clock."""
import argparse
import json
from pathlib import Path
from audit_phase import audit as common_audit
from phase_sweep import sha


def number(value):
    value = str(value).strip()
    return int(value, 2) if set(value) <= {'0','1'} else int(value.removeprefix('0x'), 16)


def audit(out):
    common_audit(out)
    m = json.loads((out/'routed.json').read_text())['modules']['top']; cells = m['cells']
    launch = m['netnames']['$glbnet$launch']['bits']; capture = m['netnames']['$glbnet$capture']['bits']
    rows = []
    for p in json.loads((out/'probes.json').read_text()):
        k = p['id']; side = p['port']; other = 'B' if side == 'A' else 'A'; kind = p['kind']
        c = cells[f'mem{k}_bram']; params = c['parameters']; pins = c['connections']
        q = cells[f'q{k}_ff']['connections']['Q']
        assert c['type'] == 'DP16KD'
        assert number(params[f'DATA_WIDTH_{side}']) == p['width']
        assert params[f'REGMODE_{side}'] == p.get('regmode','NOREG') and params['RESETMODE'] == 'SYNC'
        assert pins[f'CLK{side}'] == (launch if kind in ['ebr_output','ebr_registered_output'] else capture)
        if kind == 'ebr_address':
            assert pins[f'AD{side}{p["index"]}'] == q
            address = 1 << p['index']; word, bit = divmod(address, 16)
            segment, offset = divmod(word, 16)
            expected = 1 << (20*offset+bit+(bit >= 8))
            for n in range(64):
                assert number(params.get(f'INITVAL_{n:02X}', '0')) == (expected if n == segment else 0)
        elif kind == 'ebr_write_data':
            assert params[f'WRITEMODE_{side}'] == 'WRITETHROUGH'
            assert all(pins[f'DI{side}{j}'] == q for j in range(18))
        elif kind == 'ebr_we':
            assert params[f'WRITEMODE_{side}'] == 'WRITETHROUGH'
            assert pins[f'WE{side}'] == q
            assert pins[f'CLK{other}'] == launch and params[f'CLK{other}MUX'] == 'INV'
        elif kind == 'ebr_reset': assert pins[f'RST{side}'] == q
        elif kind == 'ebr_ce': assert pins[f'CE{side}'] == q
        elif kind == 'ebr_cs':
            assert pins[f'CS{side}{p["index"]}'] == q
            assert params[f'CSDECODE_{side}'] == f'0b{1 << p["index"]:03b}'
        elif kind in ['ebr_output','ebr_registered_output']:
            assert number(params['INITVAL_00']) == ((1 << 18)-1) << 20
            for j in range(18):
                cap = cells[f'mcap{k}_{j}_ff']
                data = next(v for n, v in cap['connections'].items() if n in ['DI','M'] and v)
                assert data == pins[f'DO{side}{j}'] and cap['connections']['CLK'] == capture
        else: raise ValueError(kind)
        rows.append(dict(probe=k, kind=kind, port=side, index=p['index'], bel=c['attributes']['NEXTPNR_BEL'],
                         joint_reset=p['joint_reset'], writeback_on_launch_falling=kind == 'ebr_we'))
    result = dict(passed=True, probes=rows, source='https://www.latticesemi.com/view_document?document_id=50466',
                  input_sha256={str(out/n):sha(out/n) for n in ['measure.v','routed.json','probes.json']})
    (out/'sensitization-audit.json').write_text(json.dumps(result,indent=2)+'\n')
    print('EBR physical probe audit passed:',len(rows))
    return result


if __name__ == '__main__':
    parser=argparse.ArgumentParser();parser.add_argument('directory',type=Path)
    audit(parser.parse_args().directory.resolve())
