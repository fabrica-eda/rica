import json
import socket
import struct
import time
from read_debug import pack, recv, read_selectors


def frame(s, op, address=0, data=0, mask=0):
    packet = bytes([0xd5, (op<<4)|1, 0x55])+struct.pack('<IQB',address,data,mask)
    tms = [1,0,0]+[0]*127+[1,1,0]
    tdi = [0]*3+[(packet[i//8]>>(i%8))&1 for i in range(128)]+[0,0]
    s.sendall(b'shift:'+struct.pack('<I',len(tms))+pack(tms)+pack(tdi))
    recv(s,(len(tms)+7)//8)


with socket.create_connection(('127.0.0.1',3721),timeout=10) as s:
    s.sendall(b'getinfo:')
    while recv(s,1) != b'\n':
        pass
    s.sendall(b'settck:'+struct.pack('<I',1000))
    recv(s,4)
    for offset in [0,4,8,12,16,20,24,28]:
        for imm in [32,64,128,256,512,90]:
            words = [0x13]*(offset//4)+[(imm<<20)|0xf93,0x6f]
            if len(words)%2:
                words.append(0x13)
            frame(s,3,data=2)
            for index in range(0,len(words),2):
                frame(s,1,address=index*4,data=words[index]|(words[index+1]<<32),mask=255)
            frame(s,2,address=0x100)
            frame(s,3,data=1)
            time.sleep(0.005)
            result=read_selectors(s,list(range(96,160)))
            print(json.dumps(dict(offset=offset,immediate=imm,x31=result)),flush=True)
