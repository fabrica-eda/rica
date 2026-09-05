"""Read the live ER1 mux without TAP reset or changing CPU run state."""
import json
import socket
import struct


def recv(s, n):
    data = bytearray()
    while len(data) < n:
        part = s.recv(n-len(data))
        if not part:
            raise EOFError
        data.extend(part)
    return data


def pack(bits):
    value = bytearray((len(bits)+7)//8)
    for i,b in enumerate(bits):
        value[i//8] |= int(b) << (i%8)
    return value


def read_selectors(s, selectors):
    tms, tdi, positions = [], [], []
    def push(m, d=0):
        tms.append(m)
        tdi.append(d)
    for seq, selector in enumerate([*selectors, None]):
        data = bytes([0xd5, 0x41 if selector is not None else 1, seq%256])
        data += struct.pack('<I', selector or 0) + bytes(9)
        for b in (1,0,0):
            push(b)
        start = len(tms)
        for i in range(128):
            push(i==127, (data[i//8]>>(i%8)) & 1)
        push(1)
        push(0)
        if seq:
            positions.append(start)
    count = len(tms)
    s.sendall(b'shift:'+struct.pack('<I',count)+pack(tms)+pack(tdi))
    result = recv(s,(count+7)//8)
    counts = [sum((result[i//8]>>(i%8))&1 for i in range(start,start+128)) for start in positions]
    return dict(value=sum((n>=64)<<i for i,n in enumerate(counts)),
                nonuniform=[dict(bit=i, ones=n) for i,n in enumerate(counts) if n not in (0,128)])


if __name__ == "__main__":
    with socket.create_connection(('127.0.0.1',3721), timeout=10) as s:
        s.sendall(b'getinfo:')
        info = bytearray()
        while not info.endswith(b'\n'):
            info += recv(s,1)
        s.sendall(b'settck:'+struct.pack('<I',166))
        period = struct.unpack('<I',recv(s,4))[0]
        result = dict(tck_period_ns=period)
        result['status'] = read_selectors(s, list(range(32)))
        for name,bank in [('tohost',1),('x31',3)]:
            result[name] = read_selectors(s,list(range(bank*32,(bank+2)*32)))
        for counter,name in enumerate(['mcycle','minstret','dependency','memory','redirect','frontend','conditional','direction_wrong','btb_miss','ras_fallback','repair']):
            result[name] = read_selectors(s, [(counter<<8)|(5<<5)|b for b in range(32)]+[(counter<<8)|(6<<5)|b for b in range(32)])
        print(json.dumps(result,indent=2))
