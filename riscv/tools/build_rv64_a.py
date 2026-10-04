#!/usr/bin/env python3
"""Build self-checking A diagnostics with an independent integer oracle."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'build/rv64-a'
MASK = (1 << 64) - 1
OPS = ['add', 'swap', 'xor', 'or', 'and', 'min', 'max', 'minu', 'maxu']


def signed(x, bits):
    return (x & ((1 << (bits - 1)) - 1)) - (x & (1 << (bits - 1)))


def expected(op, a, b, bits):
    mask = (1 << bits) - 1
    a, b = a & mask, b & mask
    return {'add': lambda: a+b, 'swap': lambda: b, 'xor': lambda: a^b,
            'or': lambda: a|b, 'and': lambda: a&b,
            'min': lambda: min(signed(a,bits), signed(b,bits)),
            'max': lambda: max(signed(a,bits), signed(b,bits)),
            'minu': lambda: min(a,b), 'maxu': lambda: max(a,b)}[op]() & mask


PRE = '''.include "test_macros.inc"
.section .text.init, "ax", @progbits
.globl _start
_start:
    la t0, fail
    csrw mtvec, t0
    csrr t0, misa
    andi t0, t0, 1
    beqz t0, fail
'''
POST = '''    PASS
.balign 4
fail:
    FAIL 90
.section .data, "aw", @progbits
.balign 8
cell: .dword 0
other: .dword 0
'''


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    clang = shutil.which(os.environ.get('RISCV_CLANG', 'clang'))
    original = Path(clang).parent.parent / 'nix-support/orig-cc'
    if original.is_file():
        clang = str(Path(original.read_text().strip()) / 'bin/clang')
    sysroot = Path(subprocess.check_output(['rustup', 'run', '1.98.1', 'rustc', '--print', 'sysroot'], text=True).strip())
    linker = sysroot / 'lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld'
    sources = {}
    edges = [0, 1, MASK, 0x7fffffff, 0x80000000, 0xffffffff, 1 << 63, (1 << 63)-1, 0x1234567887654321]
    for op in OPS:
        for bits, suffix in [(32,'w'),(64,'d')]:
            # Preserve the adjacent word on AMO.W writes and ignore high rs2.
            rows = [(a,b,signed(a,bits)&MASK,(a & (MASK ^ ((1<<bits)-1))) | expected(op,a,b,bits))
                    for a in edges for b in edges]
            text = PRE + f'    la s0, cases\n    li s1, {len(rows)}\n    la s2, cell\nloop:\n'
            text += '    ld a0, 0(s0)\n    ld a1, 8(s0)\n    ld a2, 16(s0)\n    ld a3, 24(s0)\n'
            for order in ['', '.aq', '.rl', '.aqrl']:
                text += f'''    sd a0, 0(s2)
    amo{op}.{suffix}{order} a4, a1, (s2)
    addi a5, a4, 0
    bne a5, a2, fail
    ld a5, 0(s2)
    bne a5, a3, fail
'''
            text += f'''    sd a0, 0(s2)
    mv a4, a1
    amo{op}.{suffix} a4, a4, (s2)
    bne a4, a2, fail
    sd a0, 0(s2)
    mv a4, s2
    amo{op}.{suffix} a4, a1, (a4)
    bne a4, a2, fail
    sd a0, 0(s2)
    amo{op}.{suffix} zero, a1, (s2)
    ld a5, 0(s2)
    bne a5, a3, fail
    addi s0, s0, 32
    addi s1, s1, -1
    bnez s1, loop
'''
            text += POST + '.section .rodata, "a", @progbits\n.balign 8\ncases:\n'
            text += ''.join('    .dword '+', '.join(hex(v) for v in row)+'\n' for row in rows)
            sources[f'amo{op}-{suffix}'] = text
    # Handwritten integration probes stay reviewable rather than generated.
    for p in sorted((ROOT / 'tests/rv64-a').glob('*.S')):
        sources[p.stem] = p.read_text()
    record = {'commands': [], 'elfs': {}}
    for name, text in sources.items():
        source, obj, elf = [OUT / (name+ext) for ext in ['.S','.o','.elf']]
        source.write_text(text)
        for cmd in [[clang, '--target=riscv64-unknown-elf', '-march=rv64imac_zicsr_zifencei', '-mabi=lp64',
                     '-mno-relax', '-I', ROOT/'tests/rv64i-subset', '-c', source, '-o', obj],
                    [linker, '-flavor', 'gnu', '-m', 'elf64lriscv', '-T', ROOT/'tests/rv64-m-benchmarks/link.ld', '-o', elf, obj]]:
            cmd = list(map(str,cmd)); subprocess.run(cmd, check=True); record['commands'].append(cmd)
        record['elfs'][str(elf.relative_to(ROOT.parent))] = hashlib.sha256(elf.read_bytes()).hexdigest()
    (OUT/'build.json').write_text(json.dumps(record,indent=2)+'\n')
    print(f'Built {len(sources)} A diagnostics in {OUT}')


if __name__ == '__main__':
    main()
