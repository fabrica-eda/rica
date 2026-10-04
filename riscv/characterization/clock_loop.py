"""Board-only positive-delay bounds for DCCA/global-clock paths.

Each selected oscillator alternates a rising-edge and falling-edge FF through
an explicit DCCA. Its period includes both clock-tree traversals, both clkQ
delays, and the return logic/routes. It is a bound on that complete physical
loop, not an independently identified DCCA delay.
"""
import argparse
import json
from pathlib import Path
import subprocess
import statistics
from sta_sweep import ROOT, YOSYS, NEXTPNR, BASELINE, sha, run


def audit(out):
    build=json.loads((out/'build.json').read_text())
    for name,digest in build['sha256'].items():assert sha(out/name)==digest
    m=json.loads((out/'routed.json').read_text())['modules']['top'];cells=m['cells'];rows=[]
    def data(c):return next(v for p,v in c['connections'].items() if p in ['DI','M'] and v)
    for p in json.loads((out/'probes.json').read_text()):
        k=p['id'];buf=cells[f'clock_buffer{k}'];pos=cells[f'positive{k}_ff'];neg=cells[f'negative{k}_ff']
        feedback=cells[f'feedback{k}'];invert=cells[f'invert{k}']
        assert buf['type']=='DCCA' and buf['connections']['CLKO']==pos['connections']['CLK']==neg['connections']['CLK']
        assert pos['parameters'].get('CLKMUX','CLK')=='CLK' and neg['parameters']['CLKMUX']=='INV'
        assert data(pos)==invert['connections']['F'] and invert['connections']['A']==neg['connections']['Q']
        assert data(neg)==pos['connections']['Q']
        assert int(invert['parameters']['INITVAL'],2)==0x5555 and int(feedback['parameters']['INITVAL'],2)==0x9090
        assert feedback['connections']['A']==pos['connections']['Q'] and feedback['connections']['B']==neg['connections']['Q']
        assert feedback['connections']['F']==buf['connections']['CLKI']
        actual_pos=pos['attributes']['NEXTPNR_BEL'];actual_neg=neg['attributes']['NEXTPNR_BEL']
        # SA may relocate a LUT/FF pair during legalization. Measure the final
        # routed loop and retain that discrepancy instead of mislabelling its site.
        rows.append(dict(probe=k,buffer_bel=buf['attributes']['NEXTPNR_BEL'],positive_bel=actual_pos,negative_bel=actual_neg,
                         requested_positive_bel=p['positive_bel'],requested_negative_bel=p['negative_bel'],
                         requested_placement_preserved=(actual_pos==p['positive_bel'] and actual_neg==p['negative_bel'])))
    result=dict(passed=True,probes=rows,input_sha256={str(out/n):sha(out/n) for n in ['build.json','routed.json','measure.bit']})
    (out/'physical-audit.json').write_text(json.dumps(result,indent=2)+'\n')
    return result


def measure(out):
    audit(out)
    try:
        run(out,'program',['openFPGALoader','-c','xvc-client','--ip','127.0.0.1','--port','3721','--write-sram',str(out/'measure.bit')])
        run(out,'readout',['python3','riscv/characterization/readout.py','--seconds','20','--output',str(out/'raw-counts.json'),'--bitstream',str(out/'measure.bit')])
    finally:
        run(out,'restore',['target/release/hardware-riscv-tests','--xvc','127.0.0.1:3721','--bitstream',str(BASELINE),'riscv/build/rust-kernel/kernel-atomics.elf'])
    raw=json.loads((out/'raw-counts.json').read_text());rows=[]
    assert raw['bitstream_sha256']==sha(out/'measure.bit')
    for p in json.loads((out/'probes.json').read_text()):
        counts=[r['count'] for r in raw['samples'] if r['probe']==p['id'] and r['sequence']>6]
        assert len(counts)>=4 and min(counts)>1000,(p,counts)
        assert (max(counts)-min(counts))/statistics.median(counts)<.01,(p,counts)
        period=lambda count:raw['gate_reference_cycles']/raw['reference_hz_nominal']/raw['divider']/count*1e12
        rows.append(dict(probe=p['id'],holdout=p['holdout'],counts=counts,period_ps=period(statistics.median(counts)),
                         observed_period_range_ps=[period(max(counts)),period(min(counts))]))
    result=dict(kind='measured_complete_clock_feedback_loop_periods',sta_qualified=False,
                independent_dcca_delay_identified=False,rows=rows,
                input_sha256={str(out/n):sha(out/n) for n in ['raw-counts.json','measure.bit','routed.json','physical-audit.json','build.json']})
    (out/'clock-loop-model.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(rows,indent=2))


def build(out):
    out.mkdir(parents=True,exist_ok=False)
    available=set(json.loads((ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay/bels.json').read_text()))
    probes=[]
    v=['''module measure(input clk, output [7:0] led);
wire jtck,jshift,jce1;
reg [127:0] scan=0;
reg shift_d=0;
wire [127:0] report;
JTAGG jtag(.JTCK(jtck),.JSHIFT(jshift),.JCE1(jce1),.JTDO1(scan[0]),.JTDO2(scan[0]));
always @(negedge jtck) begin
 shift_d<=jshift && jce1;
 if(shift_d) scan<={1'b0,scan[127:1]};
 else if(!jshift) scan<=report;
end
reg [21:0] phase=0;
reg [2:0] active=0;
reg [31:0] sequence=0,measured=0,before_count=0;
reg [2:0] measured_id=0;
reg [5:0] enables=0;
wire [31:0] counts[0:5];
always @(posedge clk) begin
 phase<=phase+1'b1;
 if(phase==22'd4095) enables<=6'd1<<active;
 if(phase==22'd1052671) enables<=0;
 if(phase==22'd2048) before_count<=counts[active];
 if(phase==22'd1053696) begin
  measured<=counts[active]-before_count;
  measured_id<=active; sequence<=sequence+1'b1;
 end
 if(phase==22'h3fffff) active<=active==5?0:active+1'b1;
end
assign report={32'h524f4d31,sequence,29'b0,measured_id,measured};
assign led={5'b0,active};
''']
    for k,(x,target_y) in enumerate([(15,15),(50,15),(100,15),(15,75),(50,75),(100,75)]):
        y=next(y for y in range(target_y,target_y+8) if f'X{x}/Y{y}/SLICEA.FF0' in available and f'X{x+2}/Y{y}/SLICEA.FF0' in available)
        v.append(f'''wire raw{k},osc{k},positive{k},negative{k},not_negative{k};
(* keep=1 *) LUT4 #(.INIT(16'h5555)) invert{k} (.A(negative{k}),.B(1'b0),.C(1'b0),.D(1'b0),.Z(not_negative{k}));
// C enables an XNOR of the two edge states; only one loop is enabled.
(* keep=1 *) LUT4 #(.INIT(16'h9090)) feedback{k} (.A(positive{k}),.B(negative{k}),.C(enables[{k}]),.D(1'b0),.Z(raw{k}));
(* keep=1 *) DCCA clock_buffer{k} (.CLKI(raw{k}),.CE(1'b1),.CLKO(osc{k}));
(* keep=1, BEL="X{x}/Y{y}/SLICEA.FF0" *) TRELLIS_FF positive{k}_ff (.CLK(osc{k}),.DI(not_negative{k}),.CE(1'b1),.LSR(1'b0),.Q(positive{k}));
(* keep=1, BEL="X{x+2}/Y{y}/SLICEA.FF0" *) TRELLIS_FF #(.CLKMUX("INV")) negative{k}_ff (.CLK(osc{k}),.DI(positive{k}),.CE(1'b1),.LSR(1'b0),.Q(negative{k}));
reg [3:0] divide{k}=0;
reg [31:0] count{k}=0;
always @(posedge osc{k}) divide{k}<=divide{k}+1'b1;
always @(posedge divide{k}[3]) count{k}<=count{k}+1'b1;
assign counts[{k}]=count{k};
''')
        probes.append(dict(id=k,holdout=k>=3,positive_bel=f'X{x}/Y{y}/SLICEA.FF0',negative_bel=f'X{x+2}/Y{y}/SLICEA.FF0'))
    v.append('endmodule')
    (out/'measure.v').write_text('\n'.join(v)+'\n')
    (out/'probes.json').write_text(json.dumps(probes,indent=2)+'\n')
    (out/'measure.lpf').write_text((ROOT/'riscv/lfe5um5g-85f-evn.lpf').read_text())
    (out/'clocks.py').write_text("ctx.addClock('clk',12)\nctx.addClock('jtck',6)\n")
    (out/'generator-at-build.py').write_text(Path(__file__).read_text())
    run(out,'yosys',[YOSYS,'-Q','-T','-p',f'read_verilog {out}/measure.v; synth_ecp5 -noccu2 -top measure -json {out}/measure.json'])
    run(out,'nextpnr',[NEXTPNR,'--um5g-85k','--package','CABGA381','--speed','8','--json',str(out/'measure.json'),'--lpf',str(out/'measure.lpf'),'--textcfg',str(out/'measure.config'),'--write',str(out/'routed.json'),'--pre-pack',str(out/'clocks.py'),'--ignore-loops','--no-tmdriv','--placer','sa','--freq','12','--disable-router-lutperm'])
    run(out,'ecppack',['/usr/bin/ecppack',str(out/'measure.config'),str(out/'measure.bit'),'--compress'])
    (out/'build.json').write_text(json.dumps(dict(measurement_only=True,intentional_sequential_feedback=True,
        sha256={p.name:sha(p) for p in out.iterdir() if p.is_file()}),indent=2)+'\n')


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--measure',action='store_true');args=parser.parse_args()
    (measure if args.measure else build)(args.output.resolve())
