"""Generate a small, explicitly placed ring-oscillator measurement image."""
from pathlib import Path
import json,argparse
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay'
OUT.mkdir(parents=True,exist_ok=True)
# One active oscillator, followed by 1024 reference cycles of settling.
# 2**20 ticks at the board's nominal 12 MHz reference form each gate window.
parser=argparse.ArgumentParser();parser.add_argument("--axis",choices=["x","y"],default="x");parser.add_argument("--pin",choices=list("ABCD"),default="A");args=parser.parse_args()
probes=[]
available=set(json.loads((OUT/'bels.json').read_text()))
used=set()
for k,(n,span) in enumerate([(33,1),(65,1),(97,1),(33,4),(65,4),(97,4),(33,12),(65,12),(97,12),(49,1),(49,4),(49,12)]):
 base={1:20,4:48,12:78}[span]
 start=6+(k%3)*18 if k<9 else 70
 if args.axis=='x':
  rows=[y for y in range(start,min(start+18,94)) if all(f'X{x}/Y{y}/SLICEA.K0' in available for x in [base,base+span])]
  coords=[(base+(i%2)*span,rows[i//8]) for i in range(n)]
 else:
  base=next(b for b in range(base,base+6) if all(f'X6/Y{y}/SLICEA.K0' in available for y in [b,b+span]))
  cols=[x for x in range(start,min(start+18,124)) if all(f'X{x}/Y{y}/SLICEA.K0' in available for y in [base,base+span])]
  coords=[(cols[i//8],base+(i%2)*span) for i in range(n)]
 sites=[f'X{x}/Y{y}/SLICE{"ABCD"[(i//2)%4]}.K0' for i,(x,y) in enumerate(coords)]
 assert set(sites)<=available and len(set(sites))==n and not (set(sites)&used)
 used.update(sites)
 probes.append(dict(id=k,stages=n,span=span,sites=sites,axis=args.axis,input_pin=args.pin,holdout=k>=9))
lines=['''module measure(input clk, output [7:0] led);
wire jtck, jshift, jce1, jce2, jrstn, jupdate, jrti1, jrti2, jtdi;
reg [127:0] scan=0;
reg shift_d=0;
wire [127:0] report;
JTAGG jtag(.JTCK(jtck),.JTDI(jtdi),.JSHIFT(jshift),.JUPDATE(jupdate),
 .JRSTN(jrstn),.JCE1(jce1),.JCE2(jce2),.JRTI1(jrti1),.JRTI2(jrti2),
 .JTDO1(scan[0]),.JTDO2(scan[0]));
always @(negedge jtck) begin
 shift_d <= jshift && jce1;
 if (shift_d) scan <= {1'b0,scan[127:1]};
 else if (!jshift) scan <= report;
end
reg [21:0] phase=0;
reg [3:0] active=0;
reg [31:0] sequence=0;
reg [31:0] measured=0;
reg [3:0] measured_id=0;
reg [31:0] before_count=0;
reg [11:0] enables=0;
wire [31:0] counts [0:11];
always @(posedge clk) begin
 phase <= phase + 1'b1;
 if (phase==22'd4095) enables <= 12'd1 << active;
 if (phase==22'd1052671) enables <= 0;
 if (phase==22'd2048) before_count <= counts[active];
 if (phase==22'd1053696) begin
  measured <= counts[active]-before_count;
  measured_id <= active;
  sequence <= sequence+1'b1;
 end
 if (phase==22'h3fffff) active <= active==11 ? 0 : active+1'b1;
end
assign report={32'h524f4d31,sequence,28'd0,measured_id,measured};
assign led={4'b0,active};
''']
for p in probes:
 k,n=p['id'],p['stages'];lines.append(f'wire [{n-1}:0] ro_{k};\nwire en_{k}=enables[{k}];')
 for i,site in enumerate(p['sites']):
  pin="ABCD".index(args.pin);gatepin=(pin+1)%4
  value=sum((not ((v>>pin&1) and (v>>gatepin&1)) if i==0 else not (v>>pin&1))<<v for v in range(16))
  connections={x:"1'b0" for x in "ABCD"};connections[args.pin]=f'ro_{k}[{(i-1)%n}]'
  if i==0:connections["ABCD"[gatepin]]=f'en_{k}'
  ports=','.join(f'.{x}({v})' for x,v in connections.items())
  lines.append(f'(* keep=1, BEL="{site}" *) LUT4 #(.INIT(16\'h{value:04x})) ring_{k}_{i} ({ports},.Z(ro_{k}[{i}]));')
 lines.append(f'''reg [3:0] div_{k}=0;
reg [31:0] count_{k}=0;
always @(posedge ro_{k}[{n-1}]) div_{k} <= div_{k}+1'b1;
always @(posedge div_{k}[3]) count_{k} <= count_{k}+1'b1;
assign counts[{k}]=count_{k};
''')
lines.append('endmodule\n')
(OUT/'measure.v').write_text('\n'.join(lines));(OUT/'probes.json').write_text(json.dumps(probes,indent=2)+'\n')
(OUT/'measure.lpf').write_text((ROOT/'riscv/lfe5um5g-85f-evn.lpf').read_text())
print(OUT)
