"""Read raw oscillator counts; never derives samples from STA predictions."""
import argparse,json,socket,struct,time,hashlib
from pathlib import Path
class Xvc:
 def __init__(self,address):
  host,port=address.rsplit(':',1);self.s=socket.create_connection((host,int(port)),timeout=10)
  self.s.sendall(b'getinfo:');info=b''
  while not info.endswith(b'\n'):info+=self.recv(1)
  self.max_bits=int(info.strip().split(b':')[-1]);self.s.sendall(b'settck:'+struct.pack('<I',166));self.period_ns=struct.unpack('<I',self.recv(4))[0]
 def recv(self,n):
  out=b''
  while len(out)<n:
   b=self.s.recv(n-len(out))
   if not b:raise RuntimeError('XVC disconnected')
   out+=b
  return out
 def shift(self,tms,tdi):
  assert len(tms)==len(tdi)<=self.max_bits
  def pack(bits):return sum(int(b)<<i for i,b in enumerate(bits)).to_bytes((len(bits)+7)//8,'little')
  self.s.sendall(b'shift:'+struct.pack('<I',len(tms))+pack(tms)+pack(tdi));v=int.from_bytes(self.recv((len(tms)+7)//8),'little');return [(v>>i)&1 for i in range(len(tms))]
 def select(self):
  tms=[1]*5+[0]+[1,1,0,0]+[0]*7+[1]+[1,0];tdi=[0]*10+[(0x32>>i)&1 for i in range(8)]+[0,0];self.shift(tms,tdi)
 def frame(self):
  tms=[1,0,0]+[0]*159+[1]+[1,0];bits=self.shift(tms,[0]*len(tms))[3:163];return sum(b<<i for i,b in enumerate(bits))
 def close(self):self.s.close()
def decode(v):
 for offset in range(5):
  f=(v>>offset)&((1<<128)-1)
  if f>>96==0x524f4d31:
   count=f&0xffffffff;probe=(f>>32)&0xffffffff;seq=(f>>64)&0xffffffff
   if probe<12 and count:return dict(sequence=seq,probe=probe,count=count,bit_offset=offset)
 return None
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--xvc',default='127.0.0.1:3721');p.add_argument('--seconds',type=float,default=30);p.add_argument('--output',type=Path,required=True);p.add_argument('--bitstream',type=Path,required=True);a=p.parse_args()
 x=Xvc(a.xvc);x.select();samples=[];invalid=[];seen=set();start=time.monotonic();record=dict(schema=1,xvc=a.xvc,tck_period_ns=x.period_ns,reference_hz_nominal=12000000,reference_externally_calibrated=False,gate_reference_cycles=1<<20,divider=16,bitstream_sha256=hashlib.sha256(a.bitstream.read_bytes()).hexdigest(),samples=samples,invalid_frames=invalid)
 try:
  while time.monotonic()-start<a.seconds:
   raw1=x.frame();raw2=x.frame();d=decode(raw1);e=decode(raw2)
   if d and d==e and d['sequence'] not in seen:
    seen.add(d['sequence']);d.update(elapsed_s=time.monotonic()-start,raw_frames=[f'{raw1:040x}',f'{raw2:040x}']);samples.append(d);print(json.dumps(d),flush=True)
   elif d is None or e is None:
    if len(invalid)<20:invalid.append([f'{raw1:040x}',f'{raw2:040x}'])
   a.output.write_text(json.dumps(record,indent=2)+'\n');time.sleep(.035)
 finally:x.close()
 assert len(samples)>=12, f'Only {len(samples)} samples; invalid frames: {invalid[:2]}'
