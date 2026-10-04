"""Extract only physically traversed probe paths; never train on observer branches."""
import argparse
from collections import Counter, defaultdict
import json
import re
import statistics
from pathlib import Path
from collect_sta_identities import native_bel
from phase_sweep import sha


class PhysicalImage:
    def __init__(self, image, identities, classes):
        self.image = image; self.cells = image['cells']; self.bels = identities['bel_pins']; self.classes = classes
        self.drivers = {}; self.nets = {}; self.parents = {}; self.degree = Counter()
        self.implicit_clock_roots=[]
        all_edges = set()
        for net in image['nets'].values():
            if len(net['bits']) != 1: continue
            bit = net['bits'][0]; edges = {tuple(e) for e in net['edges']}
            if bit in self.nets: assert edges == self.nets[bit]
            self.nets[bit] = edges; all_edges.update(edges)
        for a, _ in all_edges: self.degree[a] += 1
        for bit, edges in self.nets.items():
            parents = defaultdict(set)
            for a, b in edges: parents[b].add(a)
            self.parents[bit] = parents
        for name, c in self.cells.items():
            for pin, direction in c['port_directions'].items():
                if direction == 'output':
                    for bit in c['connections'].get(pin, []):
                        assert bit not in self.drivers
                        self.drivers[bit] = (name, pin)

    def wire(self, name, pin):
        return self.bels[native_bel(self.cells[name]['attributes']['NEXTPNR_BEL'])][pin]

    def route(self, bit, sink_name, sink_pin):
        current = self.wire(sink_name, sink_pin); parents = self.parents[bit]; seen = set(); result = Counter()
        while current in parents:
            assert current not in seen; seen.add(current)
            assert len(parents[current]) == 1
            previous = next(iter(parents[current])); category = self.classes[previous, current]
            result['pip:'+category] += 1
            result['fanout:'+category] += self.degree[previous]
            current = previous
        source_name, source_pin = self.drivers[bit]
        if self.cells[source_name]['type'] in ['TRELLIS_COMB', 'TRELLIS_FF', 'DP16KD', 'MULT18X18D', 'DCCA']:
            expected=self.wire(source_name, source_pin)
            if current!=expected and self.cells[source_name]['type']=='DCCA' and sink_pin=='CLK' and '/G_HPBX' in current:
                # nextpnr omits implicit global backbone edges from ROUTING.
                # Their complete measured latency belongs to the common clock
                # parameter; do not fabricate physical PIPs to connect the tree.
                self.implicit_clock_roots.append(dict(source=expected,leaf_root=current,sink=self.wire(sink_name,sink_pin)))
            else:assert current == expected, (source_name, source_pin, current)
        return result

    def timing_type(self, name, output):
        c = self.cells[name]
        if c['type'] != 'TRELLIS_COMB':
            return 'MULT18X18D:REGS=NONE' if c['type'] == 'MULT18X18D' else c['type']
        if c['parameters'].get('MODE') == 'CCU2':
            return 'TRELLIS_CARRY'+c['attributes']['NEXTPNR_BEL'][-1]
        if output == 'OFX':
            return 'TRELLIS_L6MUX21' if c['connections'].get('FXA') else 'TRELLIS_PFUMX'
        return 'TRELLIS_COMB'

    def paths(self, source, sink_name, sink_pin):
        """Enumerate source-dependent combinational paths to one endpoint.

        Constant select pins are resolved before walking mux inputs. Every returned
        path carries the actual PIP branch to this sink, including loading from
        other enabled branches, but excluding their delay from this path.
        """
        active = set(); memo = {}
        def constant(bit):
            if bit in ['0', '1']: return int(bit)
            name, _ = self.drivers[bit]; c = self.cells[name]
            if c['type'] in ['GND', 'VLO']: return 0
            if c['type'] in ['VCC', 'VHI']: return 1
            if c['type'] == 'TRELLIS_COMB':
                init = int(c['parameters']['INITVAL'], 2)
                if init in [0, 65535]: return int(init != 0)
            return None
        def to_bit(bit):
            if bit == source: return [Counter()]
            if bit in memo: return memo[bit]
            if bit in ['0', '1']: return []
            name, output = self.drivers[bit]; c = self.cells[name]
            if c['type'] != 'TRELLIS_COMB': return []
            assert bit not in active; active.add(bit)
            pins = c['connections']; category = self.timing_type(name, output)
            if category == 'TRELLIS_L6MUX21':
                selected = constant(pins['M'][0]) if pins.get('M') else 0
                inputs = ['M'] + (['FXA', 'FXB'] if selected is None else ['FXB' if selected else 'FXA'])
            elif category == 'TRELLIS_PFUMX':
                selected = constant(pins['M'][0]) if pins.get('M') else 0
                inputs = ['M'] + (['A', 'B', 'C', 'D', 'F1'] if selected is None else (['F1'] if selected else ['A', 'B', 'C', 'D']))
            else:
                inputs = list('ABCD') + (['FCI'] if category.startswith('TRELLIS_CARRY') else [])
            result = []
            for pin in inputs:
                if not pins.get(pin): continue
                value = pins[pin][0]
                for prefix in to_bit(value):
                    row = prefix.copy(); row.update(self.route(value, name, pin))
                    row[f'arc:{category}:{pin}:{output}'] += 1
                    result.append(row)
            active.remove(bit); memo[bit] = result
            return result
        bit = self.cells[sink_name]['connections'][sink_pin][0]
        route = self.route(bit, sink_name, sink_pin)
        result = []
        for prefix in to_bit(bit):
            row = prefix.copy(); row.update(route); result.append(dict(sorted(row.items())))
        return result


def extract(directory):
    requests = json.loads((directory/'requests.json').read_text())['pips']
    identities = json.loads((directory/'identities.json').read_text())
    assert identities['matched'] == len(requests)
    classes = {}
    for edge, matches in zip(requests, identities['matches']):
        choices = {m['class'] for m in matches}; assert len(choices) == 1
        classes[tuple(edge)] = next(iter(choices))
    rows = []; inputs = {}
    for file in sorted(directory.glob('*-physical.json')):
        image = json.loads(file.read_text()); root = Path(image['root'])
        if (root/'measurement-invalid.json').exists():
            inputs[str(root/'measurement-invalid.json')]=sha(root/'measurement-invalid.json')
            continue
        assert sha(root/'routed.json') == image['routed_sha256']
        physical = PhysicalImage(image, identities, classes)
        probes = json.loads((root/'probes.json').read_text())
        if (root/'clock-loop-model.json').exists():
            model=json.loads((root/'clock-loop-model.json').read_text())
            for p in model['rows']:
                k=p['probe']; features=Counter(); buffer=f'clock_buffer{k}'; feedback=f'feedback{k}'
                for name,pin in [(f'positive{k}_ff','A'),(f'negative{k}_ff','B')]:
                    bit=physical.cells[name]['connections']['Q'][0]
                    features.update(physical.route(bit,feedback,pin))
                    features[f'arc:TRELLIS_COMB:{pin}:F']+=1
                    features['arc:TRELLIS_FF:CLK:Q']+=1
                    clock=physical.cells[buffer]['connections']['CLKO'][0]
                    features.update(physical.route(clock,name,'CLK'))
                returned=physical.cells[feedback]['connections']['F'][0]
                return_route=physical.route(returned,buffer,'CLKI')
                features.update(return_route);features.update(return_route)
                features['arc:DCCA:CLKI:CLKO']=2
                rows.append(dict(image=root.name,probe=k,holdout=p['holdout'],measurement='clock_loop_period',
                                 observed_interval_ps=p['observed_period_range_ps'],paths=[dict(features)],
                                 global_backbone_in_common_clock_parameter=True))
            inputs[str(root/'clock-loop-model.json')]=sha(root/'clock-loop-model.json')
        elif root.name.startswith('registered-'):
            raw = json.loads((root/'raw-counts.json').read_text())
            for p in probes:
                features = Counter()
                for j in range(p['stages']):
                    source = f"ring_{p['id']}_{j}"; sink = f"ring_{p['id']}_{(j+1)%p['stages']}"
                    bit = physical.cells[source]['connections']['F'][0]
                    features.update(physical.route(bit, sink, p['input_pin']))
                    features[f"arc:TRELLIS_COMB:{p['input_pin']}:F"] += 1
                counts = [s['count'] for s in raw['samples'] if s['probe'] == p['id'] and s['sequence'] > 12]
                assert len(counts) >= 4 and min(counts) > 1000
                delay = raw['gate_reference_cycles']/raw['reference_hz_nominal']/statistics.median(counts)/raw['divider']/2*1e12
                rows.append(dict(image=root.name, probe=p['id'], holdout=p['holdout'], measurement='loop_average',
                                 observed_ps=delay, paths=[dict(features)]))
        elif (root/'aperture-model.json').exists():
            model = json.loads((root/'aperture-model.json').read_text())
            for p in model['probes']:
                if not p['usable_boundary']: continue
                k = p['id']; kind = p['kind']; sinks = []; launch_arc = 'arc:TRELLIS_FF:CLK:Q'
                source = physical.cells[f'q{k}_ff']['connections']['Q'][0]
                paths = []
                def capture_pin(name):
                    return next(n for n,v in physical.cells[name]['connections'].items() if n in ['DI','M'] and v)
                if kind == 'dsp_pin':
                    dsp=f'dsp{k}'
                    prefixes=physical.paths(source,dsp,p['input_pin'])
                    assert len(prefixes)==1
                    # The observer compares the full product, including
                    # transient mismatches on steady-state unchanged bits.
                    # Retain every structurally reachable observed output;
                    # steady-state XOR alone does not bound glitch settling.
                    first=18 if p['input_pin'].startswith('SIGNED') else int(p['input_pin'][1:])
                    for j in range(first,36):
                        cap=f'dcap{k}_{j}_ff'; pin=capture_pin(cap)
                        output=physical.cells[dsp]['connections'][f'P{j}'][0]
                        suffixes=physical.paths(output,cap,pin); assert len(suffixes)==1
                        path=Counter(prefixes[0]);path.update(suffixes[0])
                        path[launch_arc]+=1
                        path[f'arc:MULT18X18D:REGS=NONE:{p["input_pin"]}:P{j}']+=1
                        path[f'check:TRELLIS_FF:{pin}:CLK']+=1
                        paths.append(dict(path))
                    names=[]
                elif kind in ['bram_output','ebr_output','ebr_registered_output']:
                    side=p.get('port','A'); name=f'mem{k}_bram'
                    for j in range(18):
                        cap=f'mcap{k}_{j}_ff'; pin=capture_pin(cap)
                        output=physical.cells[name]['connections'][f'DO{side}{j}'][0]
                        for path in physical.paths(output,cap,pin):
                            path[f'arc:DP16KD:CLK{side}:DO{side}{j}']=1
                            path[f'check:TRELLIS_FF:{pin}:CLK']=1
                            paths.append(path)
                    names=[]
                elif kind=='bram_input' or kind.startswith('ebr_'):
                    side=p.get('port',kind[-1] if kind[-1] in 'AB' else 'A'); name=f'mem{k}_bram'
                    if kind in ['bram_input','ebr_address']:pins=[f'AD{side}{p.get("index",4)}']
                    elif kind=='ebr_write_data':pins=[f'DI{side}{j}' for j in range(18)]
                    elif kind=='ebr_we':pins=[f'WE{side}']
                    elif kind=='ebr_reset':pins=[f'RST{side}']
                    elif kind.startswith('ebr_oce_'):pins=[f'OCE{side}']
                    elif kind.startswith('ebr_ce_'):pins=[f'CE{side}']
                    elif kind.startswith('ebr_cs'):pins=[f'CS{side}{kind.split("_")[1][-1]}']
                    else:raise ValueError('unreviewed EBR path '+kind)
                    for pin in pins:
                        for path in physical.paths(source,name,pin):
                            path[launch_arc]=1
                            path[f'check:DP16KD:{pin}:CLK{side}']=1
                            paths.append(path)
                    names=[]
                elif kind == 'loaded_ff_route':
                    names = [f'load{k}_{j}_ff' for j in range(p['fanout'])]
                elif kind == 'dsp_comb':
                    # Multi-input old probes are a separate validation workload.
                    continue
                else:
                    names = [f'cap{k}_ff']
                if kind == 'clock_reference':
                    source = physical.cells['pll']['connections']['CLKOS'][0]; launch_arc = None
                for name in names:
                    cap = physical.cells[name]
                    pins = ['LSR'] if kind == 'ff_lsr' else (['CE'] if kind=='ff_ce_independent' else (['CE', 'LSR'] if kind == 'ff_ce_lsr' else [capture_pin(name)]))
                    for pin in pins:
                        for path in physical.paths(source, name, pin):
                            if launch_arc: path[launch_arc] = 1
                            path[f'check:TRELLIS_FF:{pin}:CLK'] = 1
                            paths.append(path)
                assert paths, (root, p)
                rows.append(dict(image=root.name, probe=k, holdout=p['holdout'], measurement='capture_aperture',
                                 observed_interval_ps=p['observed_aperture_envelope_ps'], paths=paths,
                                 kind=kind, joint_controls=kind == 'ff_ce_lsr'))
            inputs[str(root/'aperture-model.json')] = sha(root/'aperture-model.json')
        inputs[str(file)] = sha(file)
    inputs[str(Path(__file__).resolve())]=sha(Path(__file__))
    result = dict(schema=1, rows=rows, input_sha256=inputs, legacy_delay_values_used=False,
                  dsp_observer_policy='All structurally reachable full-product observer bits, including possible transient mismatches on steady-state unchanged outputs.',
                  warning='Feature extraction only; clock-relative endpoint gauge and parameter identifiability must be resolved before qualification.')
    (directory/'path-features.json').write_text(json.dumps(result, indent=2)+'\n')
    print('Extracted', len(rows), 'measured path observations')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('directory', type=Path)
    extract(parser.parse_args().directory.resolve())
