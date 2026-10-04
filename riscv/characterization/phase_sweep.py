"""Board-only, fixed-route PLL phase aperture measurements (not standalone STA arcs)."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time
from readout import Xvc

ROOT = Path(__file__).resolve().parents[2]
YOSYS = '/nix/store/slvgs8kvy4vxvf9li522vj6fp2iv4fsd-yosys-0.68/bin/yosys'
NEXTPNR = '/nix/store/zhq1h4dygy4kjqm07ip7zdimcqil4fl0-nextpnr-0.11.1/bin/nextpnr-ecp5'
MAGIC = 0x50485331
SAMPLES = 1 << 18
STEP_PS = 1e12 / (624e6 * 8)
BASELINE = ROOT / 'riscv/build/riscv-hardware-compact-atomic-124p8-20260922.bit'

def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()

def run(out, name, cmd):
    with (out / (name + '.log')).open('w') as f:
        subprocess.run(cmd, cwd=ROOT, stdout=f, stderr=subprocess.STDOUT, check=True)

def generate(out):
    out.mkdir(parents=True, exist_ok=False)
    probes = []
    available=set(json.loads((ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay/bels.json').read_text()))
    v = ['''module measure(input clk, output [7:0] led);
wire launch, capture, locked, fb;
(* FREQUENCY_PIN_CLKI="12", FREQUENCY_PIN_CLKOS="15.6", FREQUENCY_PIN_CLKOS2="15.6", ICP_CURRENT="12", LPF_RESISTOR="8", MFG_ENABLE_FILTEROPAMP="1", MFG_GMCREF_SEL="2" *)
EHXPLLL #(.CLKI_DIV(1),.CLKFB_DIV(1),.CLKOP_DIV(52),.CLKOP_CPHASE(9),
 .CLKOS_ENABLE("ENABLED"),.CLKOS_DIV(40),.CLKOS_CPHASE(19),
 .CLKOS2_ENABLE("ENABLED"),.CLKOS2_DIV(40),.CLKOS2_CPHASE(19),
 .OUTDIVIDER_MUXA("DIVA"),.OUTDIVIDER_MUXB("DIVB"),.OUTDIVIDER_MUXC("DIVC"),.OUTDIVIDER_MUXD("DIVD"),
 .CLKOP_ENABLE("ENABLED"),.CLKOS3_ENABLE("DISABLED"),
 .FEEDBK_PATH("INT_OP"),.PLLRST_ENA("DISABLED")) pll
 (.CLKI(clk),.CLKFB(fb),.CLKINTFB(fb),.CLKOS(launch),.CLKOS2(capture),.LOCK(locked),
 .RST(1'b0),.STDBY(1'b0),.PHASESEL0(1'b0),.PHASESEL1(1'b0),.PHASEDIR(1'b0),
 .PHASESTEP(1'b0),.PHASELOADREG(1'b0),.PLLWAKESYNC(1'b0),
 .ENCLKOP(1'b0),.ENCLKOS(1'b0),.ENCLKOS2(1'b0),.ENCLKOS3(1'b0));
reg [20:0] warm=0;
always @(posedge clk) if (!locked) warm<=0; else if (!warm[20]) warm<=warm+1'b1;
reg ready1=0,ready2=0;
always @(negedge launch) begin ready1<=warm[20]; ready2<=ready1; end
reg [18:0] trials=0;
reg lock_lost=0;
always @(negedge launch) begin
 if (ready2 && trials<262144) trials<=trials+1'b1;
 if (ready2 && !locked) lock_lost<=1;
end
// Count both PLL outputs over exactly 2^19 nominal reference cycles.
// Independent two-FF gate synchronizers leave a +/-2 edge boundary uncertainty.
reg [1:0] l_gate=0,c_gate=0;
reg [31:0] launch_edges=0,capture_edges=0;
always @(posedge launch) begin
 l_gate<={l_gate[0],locked && warm[19] && !warm[20]};
 if(l_gate[1]) launch_edges<=launch_edges+1'b1;
end
always @(posedge capture) begin
 c_gate<={c_gate[0],locked && warm[19] && !warm[20]};
 if(c_gate[1]) capture_edges<=capture_edges+1'b1;
end
wire measuring=ready2 && trials<262144;
assign led={locked,lock_lost,trials[18],5'b0};
''']
    def ff(name, din, clk='capture', bel=None):
        attr = '(* keep=1' + (f', BEL="{bel}"' if bel else '') + ' *)'
        v.append(f'wire {name}; {attr} TRELLIS_FF {name}_ff (.CLK({clk}),.DI({din}),.CE(1\'b1),.LSR(1\'b0),.Q({name}));')
    def probe(kind, builder, **metadata):
        k = len(probes)
        # Distinct explicit FF primitives survive synthesis merging.
        ff(f'q{k}', f'~q{k}', 'launch')
        got, expected, previous = builder(k)
        v.append(f'''reg [17:0] up{k}=0,down{k}=0,old_up{k}=0,old_down{k}=0;
always @(negedge launch) if (measuring && ({got} != {expected})) begin
 if(q{k}) up{k}<=up{k}+1'b1; else down{k}<=down{k}+1'b1;
end
always @(negedge launch) if (measuring && ({got} == {previous})) begin
 if(q{k}) old_up{k}<=old_up{k}+1'b1; else old_down{k}<=old_down{k}+1'b1;
end''')
        probes.append(dict(id=k, kind=kind, **metadata))
    # Four physical replicates per class, two designated holdouts.
    for repeat in range(4):
        for length in [0,2,6,12]:
            def chain(k, length=length, repeat=repeat):
                prev=f'q{k}'
                row=[12,32,52,72][repeat]
                for j in range(length):
                    name=f'path{k}_{j}'
                    v.append(f'wire {name}; (* keep=1, BEL="X{12+j}/Y{row}/SLICE{"ABCD"[length//2%4]}.K0" *) LUT4 #(.INIT(16\'haaaa)) {name}_lut (.A({prev}),.B(1\'b0),.C(1\'b0),.D(1\'b0),.Z({name}));')
                    prev=name
                ff(f'cap{k}',prev)
                return f'cap{k}',f'q{k}',f'~q{k}'
            probe('ff_lut_ff',chain,lut_stages=length,repeat=repeat,holdout=repeat>=2)
    for repeat in range(2):
        for loads in [1,4,16,64]:
            def fanout(k, loads=loads, repeat=repeat):
                # Each load is a preserved FF and is also checked, preventing dead loads.
                rows=[r for r in range([18,60][repeat],94) if all(f'X{x}/Y{r}/SLICEB.FF0' in available for x in range(45,61))]
                for j in range(loads):
                    ff(f'load{k}_{j}',f'q{k}',bel=f'X{45+j%16}/Y{rows[[1,4,16,64].index(loads)*4+j//16]}/SLICEB.FF0')
                return '{'+','.join(f'load{k}_{j}' for j in range(loads))+'}',f'{{{loads}{{q{k}}}}}',f'{{{loads}{{~q{k}}}}}'
            probe('loaded_ff_route',fanout,fanout=loads,repeat=repeat,holdout=repeat==1)
    for mode in ['output','input']:
        for repeat in range(2):
            def bram(k, mode=mode):
                # 1024x18 ROM, alternating zero/all-one words. Explicit memory primitive.
                # ECP5 INIT packs 18 data bits in each 20-bit slot.
                init=sum(((1<<18)-1) << (20*j) for j in range(16) if j%2)
                address=f'~q{k}' if mode=='output' else f'q{k}'
                v.append(f'wire [17:0] mem{k};')
                ports=[f'.ADA{i}({address if i==4 else "1\'b0"})' for i in range(14)]
                ports += [f'.DOA{i}(mem{k}[{i}])' for i in range(18)]
                ports += [f'.DIA{i}(1\'b0)' for i in range(18)]
                ports += ['.CEA(1\'b1)', '.OCEA(1\'b1)',f'.CLKA({"launch" if mode=="output" else "capture"})', '.WEA(1\'b0)', '.RSTA(1\'b0)']
                ports += [f'.CSA{i}(1\'b0)' for i in range(3)]
                ports += ['.CEB(1\'b0)', '.OCEB(1\'b0)', '.CLKB(1\'b0)', '.WEB(1\'b0)', '.RSTB(1\'b0)']
                ports += [f'.{p}{i}(1\'b0)' for p,n in [('ADB',14),('DIB',18),('CSB',3)] for i in range(n)]
                v.append(f'(* keep=1 *) DP16KD #(.DATA_WIDTH_A(18),.DATA_WIDTH_B(18),.REGMODE_A("NOREG"),.INITVAL_00(320\'h{init:080x})) mem{k}_bram ('+','.join(ports)+');')
                if mode=='output':
                    for j in range(18): ff(f'mcap{k}_{j}',f'mem{k}[{j}]')
                    return '{'+','.join(f'mcap{k}_{j}' for j in range(18))+'}',f'{{18{{q{k}}}}}',f'{{18{{~q{k}}}}}'
                return f'mem{k}',f'{{18{{q{k}}}}}',f'{{18{{~q{k}}}}}'
            probe('bram_'+mode,bram,repeat=repeat,holdout=repeat==1,regmode='NOREG',width=18)
    for pattern,(a0,a1,b0,b1) in enumerate([(0,262143,0,262143),(0x15555,0x2aaaa,0x2aaaa,0x15555),(1,262143,131071,131072),(0x12345,0x2abcd,0x34567,0x1fedc)]):
        # Ensure the product changes; swapped equal-product operands are still a useful
        # glitch probe, but not a propagation-boundary training sample.
        def dsp(k,a0=a0,a1=a1,b0=b0,b1=b1):
            v.append(f'wire [17:0] a{k}=q{k}?18\'d{a1}:18\'d{a0}; wire [17:0] b{k}=q{k}?18\'d{b1}:18\'d{b0}; wire [35:0] product{k};')
            ports=[f'.{p}{j}({n}{k}[{j}])' for p,n in [('A','a'),('B','b')] for j in range(18)]
            ports += [f'.P{j}(product{k}[{j}])' for j in range(36)]
            ports += [f'.{p}{j}(1\'b0)' for p in ['C'] for j in range(18)]
            ports += [f'.{p}(1\'b0)' for p in ['SIGNEDA','SIGNEDB','SOURCEA','SOURCEB']]
            v.append(f'(* keep=1 *) MULT18X18D dsp{k} ('+','.join(ports)+');')
            for j in range(36): ff(f'dcap{k}_{j}',f'product{k}[{j}]')
            return '{'+','.join(f'dcap{k}_{j}' for j in reversed(range(36)))+'}',f'(q{k}?36\'d{a1*b1}:36\'d{a0*b0})',f'(q{k}?36\'d{a0*b0}:36\'d{a1*b1})'
        probe('dsp_comb',dsp,pattern=pattern,operands=[[a0,b0],[a1,b1]],constant_product=a0*b0==a1*b1,holdout=pattern>=2)
    words=['32\'h50485331',f'32\'d{len(probes)}',"{30'b0,lock_lost,locked}","{13'b0,trials}","launch_edges","capture_edges"]
    words += [f"{{14'b0,{d}{k}}}" for k in range(len(probes)) for d in ['up','down','old_up','old_down']]
    width=32*len(words)
    v.append(f'''wire jtck,jshift,jce1;
reg [{width-1}:0] scan=0;
reg shift_d=0;
wire [{width-1}:0] report={{{','.join(reversed(words))}}};
JTAGG jtag(.JTCK(jtck),.JSHIFT(jshift),.JCE1(jce1),.JTDO1(scan[0]),.JTDO2(scan[0]));
always @(negedge jtck) begin
 shift_d<=jshift && jce1;
 if(shift_d) scan<={{1'b0,scan[{width-1}:1]}};
 else if(!jshift) scan<=report;
end
endmodule''')
    (out/'measure.v').write_text('\n'.join(v)+'\n')
    (out/'design.json').write_text(json.dumps(dict(header_words=6,words_per_probe=4,frequency_gate_reference_cycles=1<<19))+'\n')
    (out/'probes.json').write_text(json.dumps(probes,indent=2)+'\n')
    (out/'measure.lpf').write_text((ROOT/'riscv/lfe5um5g-85f-evn.lpf').read_text())
    (out/'clocks.py').write_text("ctx.addClock('jtck',6)\n")

def build(out):
    (out/'generator-at-build.py').write_text(Path(__file__).read_text())
    run(out,'yosys',[YOSYS,'-Q','-T','-p',f'read_verilog {out}/measure.v; synth_ecp5 -top measure -json {out}/measure.json'])
    run(out,'nextpnr',[NEXTPNR,'--um5g-85k','--package','CABGA381','--speed','8','--json',str(out/'measure.json'),'--lpf',str(out/'measure.lpf'),'--textcfg',str(out/'measure.config'),'--write',str(out/'routed.json'),'--pre-pack',str(out/'clocks.py'),'--freq','15.6','--placer','sa','--disable-router-lutperm'])
    cells=json.loads((out/'routed.json').read_text())['modules']['top']['cells']
    for p in json.loads((out/'probes.json').read_text()):
        k=p['id']; assert cells[f'q{k}_ff']['type']=='TRELLIS_FF'
        if p['kind'].startswith('bram'): assert cells[f'mem{k}_bram']['type']=='DP16KD'
        if p['kind']=='dsp_comb': assert cells[f'dsp{k}']['type']=='MULT18X18D'
        if p['kind']=='loaded_ff_route':
            for j in range(p['fanout']): assert cells[f'load{k}_{j}_ff']['type']=='TRELLIS_FF'
    assert 'FAIL at' not in (out/'nextpnr.log').read_text()
    (out/'build.json').write_text(json.dumps({'measurement_only':True,'sha256':{p.name:sha(p) for p in out.iterdir() if p.is_file()},'generator_source_sha256':sha(out/'generator-at-build.py')},indent=2)+'\n')

def frame(x,width):
    n=width+8
    bits=x.shift([1,0,0]+[0]*(n-1)+[1]+[1,0],[0]*(n+5))[3:3+n]
    raw=sum(b<<i for i,b in enumerate(bits))
    for offset in range(5):
        v=raw>>offset
        if v&0xffffffff==MAGIC:
            words=[(v>>(32*i))&0xffffffff for i in range(width//32)]
            return words,hex(raw),offset
    raise RuntimeError(f'Invalid measurement frame {raw:x}')

def sweep(out,steps,repeats,repeat_start=0):
    assert steps and len(set(steps))==len(steps) and all(-128<=s<=110 for s in steps)
    assert repeats>=1 and repeat_start>=0
    from audit_phase import audit
    audit(out)
    probes=json.loads((out/'probes.json').read_text())
    header=json.loads((out/'design.json').read_text())['header_words'] if (out/'design.json').exists() else 4
    stride=json.loads((out/'design.json').read_text()).get('words_per_probe',2) if (out/'design.json').exists() else 2
    width=32*(header+stride*len(probes))
    config=(out/'measure.config').read_text()
    assert len(re.findall(r'^word: CLKOS2_CPHASE ',config,re.M))==1
    try:
        for repeat in range(repeat_start,repeat_start+repeats):
            for step in (steps if repeat%2==0 else list(reversed(steps))):
                dest=out/f'step-{step:+04d}-repeat-{repeat}'
                dest.mkdir(exist_ok=False)
                coarse,fine=divmod(19*8+step,8)
                # CPHASE wraps by output divider; preserves duty-cycle relation.
                coarse%=40
                changed=re.sub(r'(?m)^word: CLKOS2_CPHASE [01]+$',f'word: CLKOS2_CPHASE {coarse:07b}',config)
                changed=re.sub(r'(?m)^word: CLKOS2_FPHASE [01]+$',f'word: CLKOS2_FPHASE {fine:03b}',changed)
                (dest/'measure.config').write_text(changed)
                run(dest,'ecppack',['/usr/bin/ecppack',str(dest/'measure.config'),str(dest/'measure.bit'),'--compress'])
                run(dest,'program',['openFPGALoader','-c','xvc-client','--ip','127.0.0.1','--port','3721','--write-sram',str(dest/'measure.bit')])
                time.sleep(.25)
                x=Xvc('127.0.0.1:3721')
                try:
                    x.select();a=frame(x,width);b=frame(x,width)
                finally:x.close()
                assert a[0]==b[0] and a[0][1:4]==[len(probes),1,SAMPLES],a[0][:4]
                assert all(c<=SAMPLES//2 for c in a[0][header:])
                if header==6:
                    assert all(abs(c-(1<<19)*1.3)<=2 for c in a[0][4:6]),a[0][:6]
                rec=dict(header_words=header,words_per_probe=stride,step=step,nominal_phase_ps=step*STEP_PS,repeat=repeat,words=a[0],raw_frames=[a[1],b[1]],offsets=[a[2],b[2]],bitstream_sha256=sha(dest/'measure.bit'),config_sha256=sha(dest/'measure.config'),reference_hz_nominal=12000000,reference_externally_calibrated=False)
                (dest/'raw.json').write_text(json.dumps(rec,indent=2)+'\n')
                print(json.dumps(dict(step=step,repeat=repeat,probe_words=a[0][header:])),flush=True)
    finally:
        run(out,'restore',['target/release/hardware-riscv-tests','--xvc','127.0.0.1:3721','--bitstream',str(BASELINE),'riscv/build/rust-kernel/kernel-atomics.elf'])

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('action',choices=['build','sweep']);p.add_argument('--output',type=Path,required=True);p.add_argument('--steps',default='-10:71:2');p.add_argument('--repeats',type=int,default=2);p.add_argument('--repeat-start',type=int,default=0);p.add_argument('--steps-list');a=p.parse_args();out=a.output.resolve()
    if a.action=='build':generate(out);build(out)
    else:sweep(out,list(map(int,a.steps_list.split(','))) if a.steps_list else list(range(*map(int,a.steps.split(':')))),a.repeats,a.repeat_start)
