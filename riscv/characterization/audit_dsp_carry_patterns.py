"""Audit single-input DSP sensitization, including arithmetic dependency coverage."""
import argparse
import json
from pathlib import Path
from audit_phase import audit as common_audit
from phase_sweep import sha


def audit(out):
    common_audit(out)
    m = json.loads((out/'routed.json').read_text())['modules']['top']
    cells = m['cells']; coverage = {}; rows = []
    for p in json.loads((out/'probes.json').read_text()):
        k = p['id']; c = cells[f'dsp{k}']; pin = p['input_pin']
        assert c['type'] == 'MULT18X18D'
        for name in ['REG_INPUTA_CLK', 'REG_INPUTB_CLK', 'REG_INPUTC_CLK', 'REG_PIPELINE_CLK', 'REG_OUTPUT_CLK']:
            assert c['parameters'].get(name, 'NONE') == 'NONE'
        q = cells[f'q{k}_ff']['connections']['Q']
        assert c['connections'][pin] == q
        dynamic = [n for n, bits in c['connections'].items() if bits == q]
        assert dynamic == [pin], (k, dynamic)
        products = []
        for state, (a, b) in enumerate(p['operands']):
            if state and p['signed_side'] == 'A' and a & (1 << 17): a -= 1 << 18
            if state and p['signed_side'] == 'B' and b & (1 << 17): b -= 1 << 18
            products.append((a*b) & ((1 << 36)-1))
        assert products == p['products'] and products[0] != products[1]
        outputs = [j for j in range(36) if ((products[0] ^ products[1]) >> j) & 1]
        assert outputs == p['sensitized_outputs']
        coverage.setdefault(pin, set()).update(outputs)
        for j in range(36):
            cap = cells[f'dcap{k}_{j}_ff']
            data = next(v for n, v in cap['connections'].items() if n in ['DI', 'M'] and v)
            assert data == c['connections'][f'P{j}']
            assert cap['connections']['CLK'] == m['netnames']['$glbnet$capture']['bits']
        rows.append(dict(probe=k, input_pin=pin, sensitized_outputs=outputs, bel=c['attributes']['NEXTPNR_BEL']))
    for pin, outputs in coverage.items():
        first = 18 if pin.startswith('SIGNED') else int(pin[1:])
        assert outputs and outputs <= set(range(first, 36)), (pin, outputs)
    assert len(coverage) == 19
    assert {p[-1] for p in coverage if p.startswith('SIGNED')} == {p[0] for p in coverage if not p.startswith('SIGNED')}
    result = dict(passed=True, probes=rows, sensitized_arcs=sum(map(len, coverage.values())),
                  structural_non_arcs=[f'{pin}->P{j}' for pin in coverage for j in range(18 if pin.startswith('SIGNED') else int(pin[1:]))],
                  unobserved_real_arcs=[f'{pin}->P{j}' for pin in coverage for j in range(18 if pin.startswith('SIGNED') else int(pin[1:]),36) if j not in coverage[pin]],
                  supplemental_patterns=True,
                  structural_reason='Input bit i cannot affect product bits below i; signed correction is a multiple of 2^18.',
                  input_sha256={str(out/n): sha(out/n) for n in ['measure.v', 'routed.json', 'probes.json']})
    (out/'sensitization-audit.json').write_text(json.dumps(result, indent=2)+'\n')
    print('DSP single-input audit passed:', len(rows), 'probes,', result['sensitized_arcs'], 'sensitized arcs')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('directory', type=Path)
    audit(parser.parse_args().directory.resolve())
