"""Use measured hop costs to propose a LUT move, then run native PnR without
its uncalibrated placement predictor. This is ranking, never STA evidence.
"""
from pathlib import Path
import argparse,json,re,hashlib,collections,subprocess,os,time
ROOT=Path(__file__).resolve().parents[2];M=ROOT/'toolchain/measurements/2026-09-22-ecp5-measured-delay';OLD=ROOT/'toolchain/measurements/2026-09-21-rv64-a'
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
def xy(bel):
 m=re.match(r'R(\d+)C(\d+)/',bel);return int(m[2]),int(m[1])
def select(model,cp,g):
 place={x['cell']:x['bel'] for x in cp['placement']};ids={x['id']:x for x in g};names={x['name']:x for x in g};pairs={p['lut'] for p in cp['packing']['lut_ff_pairs']};out=collections.defaultdict(list);fanout=collections.Counter();edges=[]
 for c in g:
  for e in c['inputs']:
   fanout[e['net']]+=1;out[e['driver']].append((c,e));edges.append((c,e))
 available=[]
 for b in json.loads((M/'bels.json').read_text()):
  match=re.fullmatch(r'X(\d+)/Y(\d+)/(SLICE[A-D]\.K0)',b)
  if match:available.append(f'R{match[2]}C{match[1]}/{match[3]}')
 occupied=set(place.values());carry_slices={place[c['name']].split('.')[0] for c in g if c['name'] in place and c['kind'].startswith('Carry')}
 free=[b for b in available if b not in occupied and b.split('.')[0] not in carry_slices];byxy=collections.defaultdict(list)
 for b in free:byxy[xy(b)].append(b)
 slack={}
 for e in cp['timing']['net_setup_slacks']:slack[e['net']]=min(slack.get(e['net'],10**9),e['slack_ps'])
 coeff=model['coefficients_by_input_pin_ps']
 def edge_score(c,e,moved,new):
  driver=ids[e['driver']];sb=new if driver['name']==moved else place[driver['name']];tb=new if c['name']==moved else place[c['name']]
  if driver['kind']!='Lut(4)' or c['kind']!='Lut(4)' or e['driver_pin']!='F' or e['pin'] not in coeff or fanout[e['net']]!=1:return None
  # Measured rings used K0 LUTs with one logic sink, never FF/BRAM/carry arcs.
  if not sb.endswith('.K0') or not tb.endswith('.K0'):return None
  x,y=xy(sb);xx,yy=xy(tb);dx,dy=abs(x-xx),abs(y-yy)
  if dx+dy==0 or max(dx,dy)>12 or min(dx,dy)>2:return None
  f=[1,min(dx,5),max(dx-5,0),min(dy,5),max(dy-5,0),0]
  return sum(a*b for a,b in zip(coeff[e['pin']],f))
 proposals=[]
 for c in g:
  if c['kind']!='Lut(4)' or c['id'] in pairs or not place[c['name']].endswith('.K0'):continue
  incident=[(c,e) for e in c['inputs']]+out[c['id']]
  if not incident or any(e['pin'] in ['FXA','FXB','FCI','DI'] for _,e in out[c['id']]):continue
  values=[edge_score(t,e,c['name'],place[c['name']]) for t,e in incident]
  covered=[i for i,x in enumerate(values) if x is not None]
  if not covered or 2*len(covered)<len(incident):continue
  before=[values[i] for i in covered]
  min_slack=min(slack.get(incident[i][1]['net'],10**9) for i in covered)
  if min_slack>2000:continue
  x,y=xy(place[c['name']])
  for dx in range(-3,4):
   for dy in range(-3,4):
    for b in byxy[(x+dx,y+dy)]:
     after=[edge_score(incident[i][0],incident[i][1],c['name'],b) for i in covered]
     if any(x is None for x in after):continue
     gain=sum(before)-sum(after)
     if gain<=max(100,sum(before)*2*model['holdout_max_abs_relative_error']):continue
     proposals.append(dict(cell=c['name'],old=place[c['name']],new=b,incident_nets=sorted({e['net'] for _,e in incident}),score_before=sum(before),score_after=sum(after),predicted_cost_reduction=gain,minimum_incident_native_slack_ps=min_slack,modeled_incident_hops=len(covered),unmodeled_incident_hops=len(incident)-len(covered)))
 proposals.sort(key=lambda p:(p['minimum_incident_native_slack_ps'],-p['predicted_cost_reduction'],p['cell'],p['new']))
 return proposals,place
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--model',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--run',action='store_true');a=p.parse_args();model=json.loads(a.model.read_text());assert model['holdout_max_abs_relative_error']<=.10 and not model['sta_qualified'];assert set(model['coefficients_by_input_pin_ps'])==set('ABCD')
 for f,h in model['input_sha256'].items():assert sha(f)==h
 cp_path=ROOT/'riscv/build/atomic-120.texo.json';cp=json.loads(cp_path.read_text());g=json.loads((ROOT/'riscv/build/atomic-103-graph.json').read_text());proposals,place=select(model,cp,g);assert proposals,'No move has measured coverage and sufficient predicted gain';a.output.mkdir(exist_ok=False,parents=True);chosen=proposals[0];place[chosen['cell']]=chosen['new'];assert len(place)==len(set(place.values()))
 allowed={r['net'] for r in json.loads((OLD/'atomic-120-warm-routes.json').read_text())};routes=[]
 for r in cp['routes']:
  if r['net'] in allowed and r['net'] not in chosen['incident_nets']:
   rr=dict(r);rr.pop('net_id',None);rr['allow_reroute']=True;routes.append(rr)
 names={x['cell_id']:x['cell'] for x in cp['placement']};pairs={names[x['lut']]:names[x['ff']] for x in cp['packing']['lut_ff_pairs']};assert all(place[l]==place[f].replace('.FF','.K') for l,f in pairs.items())
 for name,data in [('placement.json',place),('routes.json',routes),('pairs.json',pairs),('proposals.json',proposals)]: (a.output/name).write_text(json.dumps(data,separators=(',',':'))+'\n')
 cmd=json.loads((OLD/'atomic-120-start.json').read_text())['command'];i=cmd.index('--setup-optimization-budget-seconds');del cmd[i:i+2];cmd+=['--no-timing-optimization']
 for flag,value in [('--output',a.output/'result.texo.json'),('--initial-placement',a.output/'placement.json'),('--initial-routes',a.output/'routes.json'),('--lut-ff-pairs',a.output/'pairs.json')]:cmd[cmd.index(flag)+1]=str(value.resolve())
 record=dict(chosen=chosen,model_sha256=sha(a.model),checkpoint_sha256=sha(cp_path),commands=[cmd],uncalibrated_placement_predictor_enabled=False,timing_evidence_transferred=False,model_is_sta_evidence=False,proposals=len(proposals),input_sha256={str(a.output/n):sha(a.output/n) for n in ['placement.json','routes.json','pairs.json']});(a.output/'application.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(chosen),flush=True)
 if a.run:
  env=os.environ.copy();env.update(TEXO_ROUTE_LIMIT='512',TEXO_METRICS='1',TEXO_PNR_METRICS='1');env.pop('TEXO_PLACEMENT_SNAPSHOT',None);start=time.time()
  with (a.output/'pnr.log').open('w') as f:r=subprocess.run(cmd,cwd=ROOT,env=env,stdout=f,stderr=subprocess.STDOUT)
  record.update(exit_code=r.returncode,seconds=time.time()-start);(a.output/'application.json').write_text(json.dumps(record,indent=2)+'\n');assert r.returncode==0
