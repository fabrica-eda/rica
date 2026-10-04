"""Export physical identity requests, with no timing values as model inputs."""
import argparse
import json
import re
from pathlib import Path
from collect_physical_routes import pair
from phase_sweep import sha


def native_bel(name):
    m = re.fullmatch(r'X(\d+)/Y(\d+)/(.*)', name)
    assert m, name
    x, y, suffix = m.groups()
    return f'R{y}C{x}/{suffix}'


def collect(roots, output):
    output.mkdir(parents=True, exist_ok=False)
    requests = set(); bels = set(); images = []
    supported = {'TRELLIS_COMB', 'TRELLIS_FF', 'DP16KD', 'MULT18X18D', 'DCCA'}
    for root in roots:
        root = root.resolve()
        module = json.loads((root/'routed.json').read_text())['modules']['top']
        nets = {}
        for name, net in module['netnames'].items():
            routing = net.get('attributes', {}).get('ROUTING', '').strip()
            if not routing:
                continue
            fields = routing.split(';'); assert len(fields) % 3 == 0
            edges = [pair(fields[i+1]) for i in range(0, len(fields), 3) if fields[i+1]]
            requests.update(edges)
            nets[name] = dict(bits=net['bits'], edges=edges)
        for c in module['cells'].values():
            if c['type'] in supported:
                bels.add(native_bel(c['attributes']['NEXTPNR_BEL']))
        image = dict(root=str(root), routed_sha256=sha(root/'routed.json'), cells=module['cells'], nets=nets)
        destination = output/(root.name+'-physical.json')
        assert not destination.exists(), 'duplicate image name'
        destination.write_text(json.dumps(image, separators=(',', ':'))+'\n')
        images.append(dict(path=str(destination.resolve()), sha256=sha(destination)))
    (output/'requests.json').write_text(json.dumps(dict(pips=sorted(requests), bels=sorted(bels)), separators=(',', ':'))+'\n')
    (output/'inputs.json').write_text(json.dumps(dict(images=images, requests_sha256=sha(output/'requests.json')), indent=2)+'\n')
    print('Requested', len(requests), 'physical PIPs and', len(bels), 'BELs')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('images', nargs='+', type=Path); args = parser.parse_args()
    collect(args.images, args.output)
