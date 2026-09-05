#!/usr/bin/env python3
"""Probe fixed RV64 images through XVC, separating hard TAP, PLL and ER1."""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import struct
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
EVIDENCE = ROOT / "toolchain/measurements/2026-09-06-rv64-jtag-zero"
IMAGES = {
    "bad": ("riscv/build/riscv-hardware-compact-4k-124p8-20260906.bit", "f76f09071540e8ffcf637e53f48b061324fcdcb9b87f1fae2ea09afb3af3473e"),
    "good": ("riscv/build/riscv-hardware-compact-4k-124p8-20260906-ce.bit", "ed8b12372736055d799bbb6b515ad7f73bad5bb4785b093e7e5cf4be41017deb"),
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Xvc:
    def __init__(self, period):
        self.prefix_bits = 0
        self.suffix_bits = 0
        self.s = socket.create_connection(("127.0.0.1", 3721), timeout=10)
        self.s.sendall(b"getinfo:")
        info = bytearray()
        while not info.endswith(b"\n"):
            info += self.read(1)
        self.s.sendall(b"settck:" + struct.pack("<I", period))
        self.period = struct.unpack("<I", self.read(4))[0]

    def read(self, count):
        data = bytearray()
        while len(data) != count:
            chunk = self.s.recv(count - len(data))
            if not chunk:
                raise EOFError("XVC closed")
            data += chunk
        return data

    def shift(self, tms, tdi=None):
        if tdi is None:
            tdi = [0] * len(tms)
        assert len(tms) == len(tdi)
        def pack(bits):
            data = bytearray((len(bits) + 7) // 8)
            for i, bit in enumerate(bits):
                data[i // 8] |= int(bit) << (i % 8)
            return data
        self.s.sendall(b"shift:" + struct.pack("<I", len(tms)) + pack(tms) + pack(tdi))
        data = self.read((len(tms) + 7) // 8)
        return [(data[i // 8] >> (i % 8)) & 1 for i in range(len(tms))]

    def reset(self):
        self.shift([1] * 8 + [0] * 16)

    def ir(self, value):
        return self.shift([1, 1, 0, 0] + [0] * 7 + [1, 1, 0],
                          [0] * 4 + [(value >> i) & 1 for i in range(8)] + [0, 0])[4:12]

    def dr(self, value, width):
        return self.shift([1, 0, 0] + [0] * (width - 1) + [1, 1, 0],
                          [0] * 3 + [(value >> i) & 1 for i in range(width)] + [0, 0])[3:3 + width]

    def frame(self, opcode, sequence=0x55, address=0, data=0, mask=0):
        packet = bytes([0xd5, (opcode << 4) | 1, sequence]) + struct.pack("<IQB", address, data, mask)
        return self.dr(int.from_bytes(packet, "little") << self.prefix_bits,
                       128 + self.prefix_bits + self.suffix_bits)

    def status(self, idle=0):
        counts = []
        self.frame(4, address=0)
        if idle:
            self.shift([0] * idle)
        for selector in range(1, 33):
            counts.append(sum(self.frame(4 if selector < 32 else 0, address=selector)))
            if idle:
                self.shift([0] * idle)
        value = sum((n >= 64) << i for i, n in enumerate(counts))
        return dict(value=f"0x{value:08x}", magic=value & 255, version=(value >> 8) & 255,
                    flags=(value >> 16) & 255, sequence=(value >> 24) & 255,
                    ones_per_selector=counts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", choices=IMAGES, default="bad")
    parser.add_argument("--bit", type=Path, help="explicit diagnostic bitstream")
    parser.add_argument("--sha256", help="required hash when --bit is supplied")
    parser.add_argument("--program", action="store_true")
    parser.add_argument("--label", required=True)
    parser.add_argument("--period", type=int, default=166)
    parser.add_argument("--idle", type=int, default=0)
    parser.add_argument("--prefix-bits", type=int, default=0)
    parser.add_argument("--suffix-bits", type=int, default=0)
    args = parser.parse_args()
    bit, expected = IMAGES[args.image]
    if args.bit is not None:
        assert args.sha256, "--bit requires --sha256"
        bit, expected = str(args.bit), args.sha256
        args.image = "diagnostic"
    else:
        assert args.sha256 is None
    assert sha(ROOT / bit) == expected
    path = EVIDENCE / (args.label + ".json")
    assert not path.exists(), "refusing to overwrite evidence"
    record = dict(image=args.image, bitstream=bit, bitstream_sha256=expected,
                  script_sha256=sha(Path(__file__)), period_requested_ns=args.period,
                  idle_tck=args.idle, prefix_bits=args.prefix_bits,
                  suffix_bits=args.suffix_bits, started_epoch=time.time())
    path.write_text(json.dumps(record, indent=2) + "\n")
    if args.program:
        cmd = ["openFPGALoader", "-c", "xvc-client", "--ip", "127.0.0.1", "--port", "3721", "--write-sram", bit]
        log = path.with_suffix(".program.log")
        with log.open("wb") as stream:
            result = subprocess.run(cmd, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
        record.update(program_command=cmd, program_exit=result.returncode, program_log_sha256=sha(log))
        assert result.returncode == 0
    xvc = Xvc(args.period)
    xvc.prefix_bits, xvc.suffix_bits = args.prefix_bits, args.suffix_bits
    xvc.reset()
    xvc.ir(0xe0)
    record["idcode"] = hex(sum(v << i for i, v in enumerate(xvc.dr(0, 32))))
    xvc.ir(0x38)
    record["pll_er2_ones"] = sum(xvc.dr(0, 128))
    xvc.ir(0x32)
    xvc.frame(3, data=2)
    record["status"] = xvc.status(args.idle)
    # Probe transaction sequence independently of CPU execution and memory IO.
    sequences = []
    for seq in [0, 1, 0x55, 0xaa, 0xff]:
        xvc.frame(3, sequence=seq, data=2)
        sequences.append(dict(expected=seq, observed=xvc.status(args.idle)))
    record.update(sequence_probes=sequences, period_actual_ns=xvc.period, finished_epoch=time.time())
    xvc.s.close()
    path.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps({k: record[k] for k in ["image", "idcode", "pll_er2_ones", "period_actual_ns", "status"]}))


if __name__ == "__main__":
    main()
