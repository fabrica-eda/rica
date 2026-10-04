"""Qualify complete measured-STA CPU output, then program/test that exact bit."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import time
from phase_sweep import ROOT,sha


def require(condition,message):
    if not condition:raise RuntimeError(message)


def metrics(text):
    return [(m[1],int(m[2]),int(m[3])) for m in re.finditer(r'^METRIC\s+(\S+)\s+mcycle=(\d+) minstret=(\d+)',text,re.M)]


def is_transport_failure(text):
    return not re.search(r'^FAIL\b',text,re.M) and any(marker in text for marker in [
        'UnexpectedEof', 'setClkFreq: fail to configure frequency',
        'JTAG init failed with: connection failure', 'IDCODE: 0xffffff00',
        'Client orderly shut down the connection.',
    ])


def qualify(directory,legacy_batch_size=238,transport_retries=0):
    require(1<=legacy_batch_size<=238 and 0<=transport_retries<=3,'invalid batching/retry bounds')
    directory=directory.resolve();out=directory/'qualification';out.mkdir(exist_ok=False)
    cp_path=directory/'result.texo.json';cp=json.loads(cp_path.read_text());app=json.loads((directory/'application.json').read_text())
    t=cp['timing'];require(t['meets_timing_closure'] and t['met_timing'],'native timing failed')
    guard=app['setup_hold_guard_ps']
    require(t['setup_uncertainty_ps']==t['hold_uncertainty_ps']==guard,'timing reserves changed')
    mt=cp['target']['measured_timing']
    require(mt['sha256']==app['timing_library_sha256']==sha(mt['path']),'different STA library')
    require(not mt['legacy_table_fallback'] and mt['required_setup_hold_guard_ps']==guard,'STA fallback or missing reserve')
    require(mt.get('lut_input_timing')=='routed_physical_pin','LUT input permutation is not timed')
    require(app['sta_table_replaced'],'STA table not replaced')
    m=cp['target']['measured_placement'];require(m and not m['guessed_placement_predictor_enabled'],'guessed predictor still enabled')
    require(m['model_sha256']==app['model_sha256'],'different measured model')
    require(m['sta_model_replaced'],'STA replacement not active')
    for path,expected in app['input_sha256'].items():require(sha(path)==expected,f'changed implementation input: {path}')
    old=ROOT/'toolchain/measurements/2026-09-21-rv64-a/atomic-120.texo.qualification'
    baseline=json.loads((old/'qualification.json').read_text())
    proofs=[p for p in old.parent.glob('*cycle-comparison.json') if sha(p)==baseline['model_proof_sha256']]
    require(len(proofs)==1,'missing exact baseline simulation proof')
    proof=json.loads(proofs[0].read_text())
    frozen=Path(app['command'][2]);require(frozen.resolve()==Path(proof['frozen_source']).resolve(),'different RTL source set')
    for source,expected in proof['source_sha256'].items():require(sha(frozen/'src'/Path(source).name)==expected,'RTL differs from simulated proof')
    atomic=sorted((ROOT/'riscv/build/rv64-a').glob('*.elf'))
    legacy=sorted((ROOT/'riscv/build/atomic-baseline-elfs').rglob('*.elf'))
    kernel=ROOT/'riscv/build/rust-kernel';kernels=[kernel/f'kernel-{name}.elf' for name in ['tasks','access-faults','syscalls','atomics']]
    console=kernel/'kernel-console.elf';require(len(atomic)==24 and len(legacy)==238,'missing test images')
    for elf in atomic+legacy+kernels+[console]:require(sha(elf)==baseline['elf_sha256'][str(elf)],f'ELF changed: {elf}')
    binary=Path(app['command'][0]);arch=ROOT/'toolchain/worktrees/dsp/deps/ecp5-dsp-architecture-v6.txdb'
    record=dict(checkpoint_sha256=sha(cp_path),application_sha256=sha(directory/'application.json'),
        previous_qualification_sha256=sha(old/'qualification.json'),model_sha256=m['model_sha256'],
        timing_library_sha256=mt['sha256'],sta_table_replaced=True,setup_hold_guard_ps=guard,
        setup_slack_ps=t['worst_slack_ps'],hold_slack_ps=t['worst_hold_slack_ps'],runs=[],qualified=False,failed_transport_attempts=[],assembled_logs={},
        orchestrator_sha256=sha(Path(__file__)),legacy_batch_size=legacy_batch_size,transport_retries=transport_retries,
        elf_sha256={str(f):sha(f) for f in atomic+legacy+kernels+[console]})
    def save(): (out/'qualification.json').write_text(json.dumps(record,indent=2)+'\n')
    def run(name,cmd,hardware=False):
        cmd=list(map(str,cmd))
        for attempt in range(1+(transport_retries if hardware else 0)):
            entry=dict(name=name,command=cmd,started=time.time(),attempt=attempt+1)
            record['runs'].append(entry);save()
            logfile=out/(name+'.log')
            with logfile.open('w') as f:r=subprocess.run(cmd,cwd=ROOT,stdout=f,stderr=subprocess.STDOUT)
            text=logfile.read_text()
            entry.update(exit_code=r.returncode,seconds=time.time()-entry['started'],log_sha256=sha(logfile));save()
            if r.returncode==0:return text
            if not hardware or not is_transport_failure(text) or attempt>=transport_retries:
                require(False,f'{name} failed; see {logfile}')
            failed_name=f'{name}-transport-failed-{attempt+1}.log'
            logfile.rename(out/failed_name)
            record['runs'].pop();entry['log_file']=failed_name
            record['failed_transport_attempts'].append(entry);save()
            # Restart the whole batch with a fresh XVC session, never replay
            # an ambiguous JTAG shift. Functional failures are not retried.
            time.sleep(2)
    save()
    run('integrated-jtck',['python3',ROOT/'riscv/tools/check_integrated_jtck_sta.py',cp_path,'--architecture',arch,'--output',out/'jtck.json','--supplemental-binary',ROOT/'target/texo-sta-replacement/release/examples/measured_jtag_sta'])
    bit=out/'rv64-a-measured-sta-124p8.bit'
    run('checked-bitgen',[binary,'bitgen',cp_path,'--architecture',arch,'--database','/usr/share/trellis/database',
        '--base-config','/home/user/.cache/texo/targets/LFE5UM5G-85F-texo-LFE5UM5G-85F-schema7-cache5-x86_64-unknown-linux-gnu/base.config',
        '--ecppack','/usr/bin/ecppack','--bit',bit])
    require(sha(cp_path)==record['checkpoint_sha256'],'checkpoint changed during bitgen')
    record['bitstream_sha256']=sha(bit);runner=ROOT/'target/release/hardware-riscv-tests';record['runner_sha256']=sha(runner)
    require(record['runner_sha256']==baseline['runner_sha256'],'hardware runner changed')
    common=[runner,'--xvc','127.0.0.1:3721','--bitstream',bit];save()
    programmed=False
    try:
        programmed=True
        run('a-hardware',[*common,*atomic],hardware=True)
        parts=[]
        for start in range(0,len(legacy),legacy_batch_size):
            name=f'legacy-hardware-batch-{start//legacy_batch_size:02d}'
            run(name,[*common,'--no-program',*legacy[start:start+legacy_batch_size]],hardware=True)
            parts.append(name+'.log')
        combined=out/'legacy-hardware.log'
        combined.write_bytes(b''.join((out/name).read_bytes() for name in parts))
        record['assembled_logs'][combined.name]=dict(parts=parts,sha256=sha(combined));save()
        run('kernel-hardware',[*common,'--no-program',*kernels],hardware=True)
        run('console-hardware',[*common,'--no-program','--kernel-console',console],hardware=True)
        comparisons=[]
        for name,count in [('a-hardware',24),('legacy-hardware',238),('kernel-hardware',4)]:
            actual=metrics((out/(name+'.log')).read_text());expected=metrics((old/(name+'.log')).read_text())
            require(len(actual)==len(expected)==count,f'{name} result count')
            for (n,cycles,instructions),(en,ec,ei) in zip(actual,expected):
                require(n==en and instructions==ei and abs(cycles-ec)<=1,f'cycle/instruction regression: {n} {cycles, instructions} vs {ec,ei}')
                comparisons.append(dict(test=n,mcycle_delta=cycles-ec,minstret_delta=instructions-ei))
        def transcript(p):return json.loads(next(x.removeprefix('CONSOLE_TRANSCRIPT ') for x in p.read_text().splitlines() if x.startswith('CONSOLE_TRANSCRIPT ')))['text']
        actual=transcript(out/'console-hardware.log');require(actual==transcript(old/'console-hardware.log'),'console transcript changed')
        record.update(hardware_tests=267,console_bytes=len(actual.encode()),counter_comparisons=comparisons)
        run('final-atomic-os',[*common,'--no-program',kernel/'kernel-atomics.elf'],hardware=True)
        require(sha(bit)==record['bitstream_sha256'],'bitstream changed during tests')
        record['qualified']=True;save()
    finally:
        if programmed and not record['qualified']:
            run('restore-qualified-cpu',[runner,'--xvc','127.0.0.1:3721','--bitstream',ROOT/'riscv/build/riscv-hardware-compact-atomic-measured-124p8-20260922.bit',kernel/'kernel-atomics.elf'],hardware=True)
    print(json.dumps({k:v for k,v in record.items() if k not in ['runs','elf_sha256','counter_comparisons']},indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('directory',type=Path);p.add_argument('--legacy-batch-size',type=int,default=238);p.add_argument('--transport-retries',type=int,default=0);a=p.parse_args();qualify(a.directory,a.legacy_batch_size,a.transport_retries)
