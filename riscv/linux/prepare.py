#!/usr/bin/env python3
"""Build the simulation's SBI firmware, static /init, initramfs and FDT.

Requires a RISC-V capable Clang and LLD. Override RISCV_CLANG and RISCV_LLD
when the default host compiler is wrapped for a different target.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys

HERE = Path(__file__).resolve().parent
OUT = Path(sys.argv[1] if len(sys.argv) > 1 else "work/linux-boot")
OUT.mkdir(parents=True, exist_ok=True)
TIMER_DIVISOR = int(os.environ.get("LINUX_SIM_TIMER_DIVISOR", "100"))
if TIMER_DIVISOR <= 0:
    raise ValueError("LINUX_SIM_TIMER_DIVISOR must be positive")
clang = os.environ.get("RISCV_CLANG", "clang")
lld = os.environ.get("RISCV_LLD", shutil.which("ld.lld") or "ld.lld")
flags = [clang, "--target=riscv64-unknown-elf", "-march=rv64imac_zicsr_zifencei",
         "-mabi=lp64", "-mcmodel=medany", "-O2", "-ffreestanding",
         "-fno-stack-protector", "-fno-pic", "-nostdlib", f"-fuse-ld={lld}", "-Wl,--no-relax"]
subprocess.run(flags + [f"-Wl,-T,{HERE / 'firmware.ld'}", str(HERE / "start.S"),
                       str(HERE / "firmware.c"), "-o", str(OUT / "firmware.elf")], check=True)
subprocess.run(flags + ["-Wl,--image-base=0x10000", str(HERE / "init.S"),
                       "-o", str(OUT / "init")], check=True)

# newc cpio: no external cpio/dtc tooling needed.
archive = bytearray()
def entry(name, mode, data=b"", rdevmajor=0, rdevminor=0):
    fields = [1, mode, 0, 0, 1, 0, len(data), 0, 0, rdevmajor, rdevminor, len(name)+1, 0]
    archive.extend(b"070701" + b"".join(f"{v:08x}".encode() for v in fields))
    archive.extend(name.encode()+b"\0")
    archive.extend(b"\0"*(-len(archive)%4))
    archive.extend(data)
    archive.extend(b"\0"*(-len(archive)%4))
entry("dev", 0o40755)
entry("dev/console", 0o20600, rdevmajor=5, rdevminor=1)
entry("init", 0o100755, (OUT / "init").read_bytes())
entry("TRAILER!!!", 0)
(OUT / "initramfs.cpio").write_bytes(archive)

strings = bytearray()
names = {}
structure = bytearray()
def u32(*values): return struct.pack(">"+"I"*len(values), *values)
def begin(name):
    structure.extend(u32(1)+name.encode()+b"\0")
    structure.extend(b"\0"*(-len(structure)%4))
def end(): structure.extend(u32(2))
def prop(name, value):
    if name not in names:
        names[name] = len(strings)
        strings.extend(name.encode()+b"\0")
    if isinstance(value, str): value = value.encode()+b"\0"
    structure.extend(u32(3,len(value),names[name])+value)
    structure.extend(b"\0"*(-len(structure)%4))
begin("")
prop("#address-cells",u32(2)); prop("#size-cells",u32(2))
prop("compatible","rica,simulation"); prop("model","Rica RTL Linux simulation")
begin("chosen")
prop("bootargs","earlycon=sbi console=hvc0 keep_bootcon loglevel=8 nokaslr rdinit=/init lpj=10000")
prop("linux,initrd-start",u32(0,0x86000000))
prop("linux,initrd-end",u32(0,0x86000000+len(archive)))
end()
begin("memory@80000000"); prop("device_type","memory"); prop("reg",u32(0,0x80000000,0,0x08000000)); end()
begin("cpus"); prop("#address-cells",u32(1)); prop("#size-cells",u32(0)); prop("timebase-frequency",u32(1000000))
begin("cpu@0")
prop("device_type","cpu"); prop("reg",u32(0)); prop("status","okay")
prop("compatible","riscv"); prop("riscv,isa","rv64imac_zicsr_zifencei"); prop("mmu-type","riscv,sv39")
prop("riscv,isa-base","rv64i")
prop("riscv,isa-extensions",b"i\0m\0a\0c\0zicsr\0zifencei\0svade\0")
prop("clock-frequency",u32(1000000 * TIMER_DIVISOR))
begin("interrupt-controller")
prop("#interrupt-cells",u32(1)); prop("interrupt-controller",b""); prop("compatible","riscv,cpu-intc"); prop("phandle",u32(1))
end(); end(); end()
end(); structure.extend(u32(9))
reserve=struct.pack(">QQQQ",0x80000000,0x200000,0,0)
offset=40+len(reserve)
header=u32(0xd00dfeed,offset+len(structure)+len(strings),offset,offset+len(structure),40,17,16,0,len(strings),len(structure))
(OUT / "platform.dtb").write_bytes(header+reserve+structure+strings)
print(f"Prepared firmware, DTB and {len(archive)}-byte initramfs in {OUT}")

manifest = {
    "clang": subprocess.check_output([clang, "--version"], text=True).splitlines()[0],
    "firmware_isa": "rv64imac_zicsr_zifencei", "abi": "lp64",
    "ram_base": "0x80000000", "ram_end": "0x88000000",
    "timer_hz": 1000000, "cpu_hz": 1000000 * TIMER_DIVISOR,
    "sha256": {name: hashlib.sha256((OUT / name).read_bytes()).hexdigest()
               for name in ["firmware.elf", "init", "initramfs.cpio", "platform.dtb"]},
    "source_sha256": {name: hashlib.sha256((HERE / name).read_bytes()).hexdigest()
                      for name in ["firmware.c", "start.S", "firmware.ld", "init.S", "prepare.py"]},
}
(OUT / "manifest.json").write_text(json.dumps(manifest, indent=2)+"\n")
