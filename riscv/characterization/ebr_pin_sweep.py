"""Sequential endpoint and clock-reference paths for a measured STA library."""
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
BASELINE = ROOT / 'riscv/build/riscv-hardware-compact-atomic-measured-124p8-20260922.bit'

def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()

def run(out, name, cmd):
    with (out / (name + '.log')).open('w') as f:
        subprocess.run(cmd, cwd=ROOT, stdout=f, stderr=subprocess.STDOUT, check=True)

def generate(out, side='A'):
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
    for repeat in range(2):
        for kind,index in [('ebr_address',j) for j in range(14)]+[(name,0) for name in ['ebr_write_data','ebr_output','ebr_reset','ebr_ce','ebr_we']]+[('ebr_cs',j) for j in range(3)]:
            def circuit(k,kind=kind,index=index):
                other='B' if side=='A' else 'A'
                width=1 if kind=='ebr_address' else 18
                init={}
                if kind=='ebr_address':
                    address=1<<index; word,bit=divmod(address,16)
                    physical_bit=bit+(bit>=8)
                    segment,word_offset=divmod(word,16)
                    init[segment]=1<<(20*word_offset+physical_bit)
                elif kind=='ebr_output':init[0]=((1<<18)-1)<<20
                else:init[0]=(1<<18)-1
                ports={f'{prefix}{port}{j}':"1'b0" for port in 'AB' for prefix,count in [('AD',14),('DI',18),('CS',3)] for j in range(count)}
                for port in 'AB':
                    ports.update({f'CLK{port}':"1'b0",f'CE{port}':"1'b0",f'OCE{port}':"1'b1",f'WE{port}':"1'b0",f'RST{port}':"1'b0"})
                ports[f'CLK{side}']='launch' if kind=='ebr_output' else 'capture'
                ports[f'CE{side}']="1'b1"
                for j in range(18):ports[f'DO{side}{j}']=f'mem{k}[{j}]'
                if kind=='ebr_address':ports[f'AD{side}{index}']=f'q{k}'
                if kind=='ebr_output':ports[f'AD{side}4']=f'~q{k}'
                if kind in ['ebr_write_data','ebr_we']:
                    ports[f'WE{side}']="1'b1" if kind=='ebr_write_data' else f'q{k}'
                    ports[f'AD{side}0']=ports[f'AD{side}1']="1'b1"
                    for j in range(18):ports[f'DI{side}{j}']=f'q{k}' if kind=='ebr_write_data' else "1'b0"
                if kind=='ebr_we':
                    ports[f'CLK{other}']='launch'
                    ports[f'CE{other}']=ports[f'WE{other}']="1'b1"
                    ports[f'AD{other}0']=ports[f'AD{other}1']="1'b1"
                    for j in range(18):ports[f'DI{other}{j}']="1'b1"
                if kind=='ebr_reset':ports[f'RST{side}']=f'q{k}'
                if kind in ['ebr_ce','ebr_cs']:
                    ports[f'RST{side}']=f'~q{k}'
                    ports[f'CE{side}' if kind=='ebr_ce' else f'CS{side}{index}']=f'q{k}'
                params=[f'.DATA_WIDTH_{port}({width})' for port in 'AB']
                params += [f'.REGMODE_{port}("NOREG")' for port in 'AB']
                params += ['.RESETMODE("SYNC")','.ASYNC_RESET_RELEASE("SYNC")']
                params += [f'.WRITEMODE_{port}("WRITETHROUGH")' for port in 'AB']
                if kind=='ebr_we':params += [f'.CLK{other}MUX("INV")']
                if kind=='ebr_cs':params += [f'.CSDECODE_{side}("0b{1<<index:03b}")']
                params += [f".INITVAL_{segment:02X}(320'h{value:080x})" for segment,value in init.items()]
                v.append(f'wire [17:0] mem{k}; (* keep=1 *) DP16KD #('+','.join(params)+f') mem{k}_bram ('+','.join(f'.{pin}({net})' for pin,net in ports.items())+');')
                if kind=='ebr_output':
                    for j in range(18):ff(f'mcap{k}_{j}',f'mem{k}[{j}]')
                    got='{'+','.join(f'mcap{k}_{j}' for j in range(18))+'}'
                else:got=f'mem{k}[0]' if width==1 else f'mem{k}'
                expected=f'~q{k}' if kind in ['ebr_reset','ebr_we'] else f'q{k}'
                previous=f'q{k}' if kind in ['ebr_reset','ebr_we'] else f'~q{k}'
                if width==18:expected=f'{{18{{{expected}}}}}';previous=f'{{18{{{previous}}}}}'
                return got,expected,previous
            probe(kind,circuit,port=side,index=index,width=1 if kind=='ebr_address' else 18,
                  repeat=repeat,holdout=repeat==1,joint_reset=kind in ['ebr_ce','ebr_cs'])
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
    run(out,'yosys',[YOSYS,'-Q','-T','-p',f'read_verilog {out}/measure.v; synth_ecp5 -noccu2 -top measure -json {out}/measure.json'])
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
    from audit_ebr_pins import audit
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
    p=argparse.ArgumentParser();p.add_argument('action',choices=['build','sweep']);p.add_argument('--output',type=Path,required=True);p.add_argument('--steps',default='-10:71:2');p.add_argument('--repeats',type=int,default=2);p.add_argument('--repeat-start',type=int,default=0);p.add_argument('--steps-list');p.add_argument('--side',choices=['A','B'],default='A');a=p.parse_args();out=a.output.resolve()
    if a.action=='build':generate(out,a.side);build(out)
    else:sweep(out,list(map(int,a.steps_list.split(','))) if a.steps_list else list(range(*map(int,a.steps.split(':')))),a.repeats,a.repeat_start)
