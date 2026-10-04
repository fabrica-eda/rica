"""Fit measured loop-average LUT+route costs; no STA labels or fabricated samples."""
from pathlib import Path
import argparse,json,hashlib,statistics,re,itertools,math
FEATURES=['lut_hops','x_near_tiles','x_far_tiles','y_near_tiles','y_far_tiles','loop_offset']
def features(p):
 coords=[tuple(map(int,re.match(r'X(\d+)/Y(\d+)/',x).groups())) for x in p['sites']]
 v=[len(coords),0,0,0,0,1]
 for (x,y),(xx,yy) in zip(coords,coords[1:]+coords[:1]):
  dx,dy=abs(x-xx),abs(y-yy);v[1]+=min(dx,5);v[2]+=max(dx-5,0);v[3]+=min(dy,5);v[4]+=max(dy-5,0)
 return v
def solve(a,b):
 n=len(b);m=[list(row)+[v] for row,v in zip(a,b)]
 for k in range(n):
  pivot=max(range(k,n),key=lambda i:abs(m[i][k]));m[k],m[pivot]=m[pivot],m[k]
  if abs(m[k][k])<1e-8:return None
  t=m[k][k];m[k]=[x/t for x in m[k]]
  for i in range(n):
   if i!=k:
    t=m[i][k];m[i]=[x-t*y for x,y in zip(m[i],m[k])]
 return [m[i][-1] for i in range(n)]
def fit(rows):
 # Small exact active-set enumeration for nonnegative least squares.
 best=None
 for mask in range(1,1<<len(FEATURES)):
  ids=[i for i in range(len(FEATURES)) if mask>>i&1]
  a=[[sum(r['features'][i]*r['features'][j] for r in rows) for j in ids] for i in ids]
  b=[sum(r['features'][i]*r['loop_delay_ps'] for r in rows) for i in ids];c=solve(a,b)
  if c is None or min(c)<0:continue
  full=[0.0]*len(FEATURES)
  for i,x in zip(ids,c):full[i]=x
  loss=sum((sum(x*y for x,y in zip(full,r['features']))-r['loop_delay_ps'])**2 for r in rows)
  if best is None or loss<best[0]:best=loss,full
 assert best;return best[1]
def analyze(dirs):
 rows=[];inputs={}
 for directory in dirs:
  d=Path(directory);raw=json.loads((d/'raw-counts.json').read_text());probes=json.loads((d/'probes.json').read_text())
  assert raw['bitstream_sha256']==hashlib.sha256((d/'measure.bit').read_bytes()).hexdigest()
  for name in ['raw-counts.json','probes.json','routed.json','measure.bit']:inputs[str(d/name)]=hashlib.sha256((d/name).read_bytes()).hexdigest()
  for p in probes:
   counts=[r['count'] for r in raw['samples'] if r['probe']==p['id'] and r['sequence']>12];assert len(counts)>=4,(d,p['id'],len(counts))
   assert min(counts)>1000
   relative_range=(max(counts)-min(counts))/statistics.median(counts);assert relative_range<0.01
   delay=raw['gate_reference_cycles']/raw['reference_hz_nominal']/statistics.median(counts)/raw['divider']/2*1e12
   rows.append(dict(experiment=d.name,probe=p['id'],input_pin=p.get('input_pin','A'),holdout=p['holdout'],features=features(p),loop_delay_ps=delay,samples=len(counts),count_min=min(counts),count_max=max(counts),relative_range=relative_range))
 by_pin={pin:fit([r for r in rows if not r['holdout'] and r['input_pin']==pin]) for pin in sorted({r['input_pin'] for r in rows})}
 for r in rows:
  r['predicted_ps']=sum(x*y for x,y in zip(by_pin[r['input_pin']],r['features']));r['relative_error']=(r['predicted_ps']/r['loop_delay_ps']-1)
 return dict(schema=1,kind='measured_loop_average_lut_plus_route_cost',device='LFE5UM5G-85F',package='CABGA381',board='connected ECP5 EVN at 127.0.0.1:3721',reference_hz_nominal=12000000,reference_externally_calibrated=False,warmup_policy='exclude complete first sweep: sequence <= 12, regardless of count',features=FEATURES,coefficients_by_input_pin_ps=by_pin,fit_method='nonnegative least squares, training probes only',rows=rows,input_sha256=inputs,holdout_max_abs_relative_error=max(abs(r['relative_error']) for r in rows if r['holdout']),sta_qualified=False,limitations=['One board, ambient temperature and supply not measured','Loop average of rising/falling propagation, not setup/hold or worst-case PVT','LUT and fixed local routing costs are not separately identifiable','Placement-ranking cost only; no change to STA timing evidence'])
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('directories',nargs='+');p.add_argument('--output',type=Path,required=True);a=p.parse_args();m=analyze(a.directories);a.output.write_text(json.dumps(m,indent=2)+'\n');print('coefficients',m['coefficients_by_input_pin_ps']);print('holdout max error',m['holdout_max_abs_relative_error'])
