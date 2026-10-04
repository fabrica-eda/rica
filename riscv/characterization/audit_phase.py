"""Check that the physical circuit still implements the intended probes."""
import argparse
import json
from pathlib import Path
from phase_sweep import sha


def audit(out):
    m=json.loads((out/'routed.json').read_text())['modules']['top']
    cells=m['cells'];nets=m['netnames'];probes=json.loads((out/'probes.json').read_text())
    summaries=[]
    launch=nets['$glbnet$launch']['bits'];capture=nets['$glbnet$capture']['bits']
    assert launch!=capture
    if (out/'build.json').exists():
        build=json.loads((out/'build.json').read_text())
        for name,h in build['sha256'].items():
            assert sha(out/name)==h,(name,'build artifact changed')
    config=(out/'measure.config').read_text()
    for letter in 'ABCD':
        assert f'enum: OUTDIVIDER_MUX{letter} DIV{letter}' in config, 'PLL bypass: phase sweep is invalid'
    for clock in ['CLKOS','CLKOS2']:
        assert f'word: {clock}_DIV 0100111' in config
        assert f'word: {clock}_CPHASE 0010011' in config
        assert f'word: {clock}_FPHASE 000' in config
        assert f'enum: {clock}_ENABLE ENABLED' in config
    assert 'word: CLKOP_DIV 0110011' in config
    def din(c):
        candidates=[v for pin,v in c['connections'].items() if pin in ['DI','M'] and v]
        assert len(candidates)==1,(c,candidates)
        return candidates[0]
    for p in probes:
        k=p['id'];q=cells[f'q{k}_ff'];assert q['type']=='TRELLIS_FF'
        assert q['connections']['CLK']==launch
        qb=q['connections']['Q'];owned=[f'q{k}_ff']
        if p['kind']=='ff_lut_ff':
            previous=qb
            for j in range(p['lut_stages']):
                name=f'path{k}_{j}_lut';c=cells[name]
                assert c['type']=='TRELLIS_COMB' and c['connections']['A']==previous
                assert int(c['parameters']['INITVAL'],2)==0xaaaa
                previous=c['connections']['F'];owned.append(name)
            c=cells[f'cap{k}_ff'];assert din(c)==previous and c['connections']['CLK']==capture;owned.append(f'cap{k}_ff')
        if p['kind']=='loaded_ff_route':
            for j in range(p['fanout']):
                name=f'load{k}_{j}_ff';c=cells[name]
                assert c['type']=='TRELLIS_FF' and din(c)==qb and c['connections']['CLK']==capture
                owned.append(name)
        if p['kind'].startswith('bram'):
            name=f'mem{k}_bram';c=cells[name]
            assert c['type']=='DP16KD'
            assert c['parameters']['REGMODE_A']=='NOREG'
            assert c['connections']['CLKA']==(launch if p['kind']=='bram_output' else capture)
            assert int(c['parameters']['DATA_WIDTH_A'],2)==18
            init=c['parameters']['INITVAL_00']
            # nextpnr accepts a binary or hex string depending on the frontend.
            actual=int(init,2) if set(init)<=set('01') else int(init.removeprefix('0x'),16)
            assert actual==sum(((1<<18)-1)<<(20*j) for j in range(16) if j%2)
            owned.append(name)
            if p['kind']=='bram_output':
                for j in range(18):
                    name=f'mcap{k}_{j}_ff';assert din(cells[name])==c['connections'][f'DOA{j}'];owned.append(name)
        if p['kind']=='dsp_comb':
            name=f'dsp{k}';c=cells[name];assert c['type']=='MULT18X18D'
            for param in ['REG_INPUTA_CLK','REG_INPUTB_CLK','REG_INPUTC_CLK','REG_PIPELINE_CLK','REG_OUTPUT_CLK']:
                assert c['parameters'].get(param,'NONE')=='NONE'
            owned.append(name)
            for j in range(36):
                name=f'dcap{k}_{j}_ff';assert din(cells[name])==c['connections'][f'P{j}'];owned.append(name)
        summaries.append(dict(id=k,kind=p['kind'],cells={name:cells[name]['attributes']['NEXTPNR_BEL'] for name in owned},launch_net_logical_users=sum(qb==bs for c in cells.values() for pin,bs in c['connections'].items() if c.get('port_directions',{}).get(pin)=='input')))
    report=dict(passed=True,probes=summaries,input_sha256={str(out/n):sha(out/n) for n in ['measure.v','probes.json','routed.json']})
    (out/'physical-audit.json').write_text(json.dumps(report,indent=2)+'\n')
    print('Physical probe audit passed:',len(probes))
    return report

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);a=p.parse_args();audit(a.output.resolve())
