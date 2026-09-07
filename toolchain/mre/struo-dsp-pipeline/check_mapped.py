"""Independent cycle check of this fixture's unsigned DSP/CCU2C/FF netlist."""
import collections
import itertools
import json
from pathlib import Path
import sys

module = next(iter(json.loads(Path(sys.argv[1]).read_text())["modules"].values()))
cells = module["cells"]
registers = {name: cell for name, cell in cells.items() if cell["type"] == "TRELLIS_FF"}
drivers = {}
for name, cell in cells.items():
    assert cell['type'] in ['TRELLIS_FF', 'CCU2C', 'MULT18X18D'], cell['type']
    for pin, direction in cell['port_directions'].items():
        if direction == 'output':
            for wire in cell['connections'][pin]:
                assert wire not in drivers
                drivers[wire] = name
state = {cell['connections']['Q'][0]: 0 for cell in registers.values()}
edge = [0, 1, (1 << 64)-1, (1 << 64)-2, 1 << 63, (1 << 32)-1, 0x123456789abcdef0]
vectors = list(itertools.product(edge, repeat=2))
random = 0xf05343218834934a
for _ in range(128):
    pair = []
    for _ in range(2):
        random ^= (random << 13) & ((1 << 64)-1)
        random ^= random >> 7
        random ^= (random << 17) & ((1 << 64)-1)
        pair.append(random)
    vectors.append(pair)
vectors.extend([(0, 0)]*6)
pending = collections.deque()
for cycle, (a, b) in enumerate(vectors):
    cache = dict(state)
    for name, value in [('a', a), ('b', b), ('clk', 0)]:
        cache.update({wire:(value >> i)&1 for i, wire in enumerate(module['ports'][name]['bits'])})
    def read(wire):
        if isinstance(wire, str):
            assert wire in ('0', '1')
            return int(wire)
        if wire not in cache:
            evaluate(cells[drivers[wire]])
        return cache[wire]
    def evaluate(cell):
        c = cell['connections']; params = cell['parameters']
        def bit(pin): return read(c[pin][0])
        def put(pin, value): cache[c[pin][0]] = value
        if cell['type'] == 'MULT18X18D':
            assert not any(bit(pin) for pin in ['SIGNEDA', 'SIGNEDB', 'SOURCEA', 'SOURCEB'])
            values = [sum(bit(f'{port}{i}') << i for i in range(18)) for port in ['A','B']]
            product = values[0] * values[1]
            for i in range(36): put(f'P{i}', (product >> i)&1)
        elif cell['type'] == 'CCU2C':
            carry = bit('CIN')
            for i in range(2):
                init = int(params[f'INIT{i}'], 2)
                values = [bit(f'{pin}{i}') for pin in 'ABCD']
                lut4 = (init >> sum(value << j for j,value in enumerate(values)))&1
                lut2 = (init >> (values[0] + 2*values[1]))&1
                injected = 0 if params[f'INJECT1_{i}'] == 'YES' else carry
                put(f'S{i}', lut4 ^ injected)
                carry = injected if lut4 else lut2
            put('COUT', carry)
        else:
            raise AssertionError('combinational traversal crossed an FF')
    updated = {}
    for cell in registers.values():
        c=cell['connections']; assert not read(c['LSR'][0]); assert read(c['CE'][0])
        updated[c['Q'][0]] = read(c['DI'][0])
    state = updated
    pending.append(a*b)
    if len(pending) == 7:
        expected = pending.popleft()
        actual = sum(state[wire] << i for i,wire in enumerate(module['ports']['product']['bits']))
        assert actual == expected, (cycle, hex(actual), hex(expected))
print(f'PASS: {len(vectors)-6} full products; seven-cycle latency; all FFs sample pre-edge values')
