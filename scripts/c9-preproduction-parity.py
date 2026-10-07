#!/usr/bin/env python3
"""Receipt-bound local C9 qualification. Rust owns requests, parsing and verdicts.

Run with --phase native, preflight or gateway. All receipts and outputs must live
outside a worktree. Native is zero generation; gateway permits exactly 30 calls for the accepted restoration.
An absent/failed/stale receipt cannot be used as acceptance evidence.
"""
import argparse
import contextlib
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import ssl
import subprocess
import sys
import urllib.request

REPO = Path(__file__).resolve().parents[1]
MANIFEST_PATH = Path('src-tauri/src/pipeline/summary/comparisons/fixtures/c9-parity-manifest.json')
TEST = 'pipeline::summary::comparisons::tests::parity::original_c9_production_gateway_parity'
NATIVE_TEST = 'pipeline::llama_cpp::framing_tests::original_c9_native_framing_parity'


def require(value, message):
    if not value:
        raise ValueError(message)


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def digest_text(value):
    return hashlib.sha256(value.encode()).hexdigest()


def read(path):
    return json.loads(Path(path).read_text())


def save(path, value):
    with Path(path).open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')
    Path(path).chmod(0o600)


def command(args):
    result = subprocess.run(args, cwd=REPO, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f'{args[0]} exited {result.returncode}: {result.stderr}')
    return result.stdout.strip()


def tree():
    require(not command(['git', 'status', '--porcelain', '--untracked-files=all']), 'checkout must be clean')
    return {'head': command(['git', 'rev-parse', 'HEAD']),
            'files': {p: sha(REPO / p) for p in command(['git', 'ls-files']).splitlines() if (REPO / p).is_file()}}


def http(url, data=None, headers=None, context=None):
    request = urllib.request.Request(url, data=None if data is None else json.dumps(data).encode(),
                                     headers={'Content-Type': 'application/json', **(headers or {})})
    with urllib.request.urlopen(request, context=context, timeout=30) as response:
        return json.load(response)


def baseline(packet):
    m = read(REPO / MANIFEST_PATH)
    require(sha(packet / 'baseline-inventory.json') == m['inventory_sha256'], 'baseline inventory changed')
    inv = read(packet / 'baseline-inventory.json')
    for name, expected in inv['files'].items():
        require(sha(packet / 'original-c9' / name) == expected, f'baseline changed: {name}')
    require(sha(packet / 'operator-decisions.json') == m['operator_decisions_sha256'], 'adjudication changed')
    return {'manifest_sha256': sha(REPO / MANIFEST_PATH), 'inventory_sha256': m['inventory_sha256'],
            'operator_decisions_sha256': m['operator_decisions_sha256']}


def audit(args):
    pins = read(REPO / MANIFEST_PATH)['gateway_runtime']
    for path, key in [(args.deployment, 'deployment_receipt_sha256'),
                      (args.model, 'model_receipt_sha256'), (args.settings, 'settings_sha256')]:
        require(sha(path) == pins[key], f'qualified {key} changed')
    deploy = read(args.deployment)
    model = read(args.model)
    settings = read(args.settings)['gateway']
    release = Path('/opt/local-inference-gateway/venv').resolve().parent
    require(str(release) == deploy['installed_release'], 'installed gateway changed')
    packages = list((release / 'venv/lib').glob('python*/site-packages/local_inference_gateway'))
    require(len(packages) == 1, 'ambiguous installed gateway')
    require(all(sha(packages[0] / p) == h for p, h in deploy['source_sha256'].items()), 'gateway module changed')
    state = dict(line.split('=', 1) for line in command([
        'systemctl', 'show', 'local-inference-gateway.service', '-p', 'ActiveState', '-p', 'SubState',
        '-p', 'MainPID', '-p', 'ExecMainStartTimestamp', '-p', 'ExecMainStartTimestampMonotonic']).splitlines())
    require(state == deploy['service'], 'gateway running process changed')
    require(command(['sudo', '-n', 'sha256sum', '/etc/local-inference-gateway/gateway.env']).split()[0]
            == deploy['config_sha256'], 'gateway config changed')
    headers = {'Authorization': 'Bearer ' + Path(settings['tokenFile']).read_text().strip()}
    profile = http(settings['baseUrl'].rstrip('/') + '/v1/tasks/document.summary.step/2/profile',
                   headers=headers, context=ssl.create_default_context(cafile=settings['caFile']))
    require(profile == model['task_profile'], 'task profile changed')
    info = http('http://127.0.0.1:11434/api/show', {'model': model['model']})
    tags = http('http://127.0.0.1:11434/api/tags')['models']
    tag = next(m for m in tags if m['name'] == model['model'])
    require(tag['digest'] == model['manifest_digest'], 'model manifest changed')
    require(digest_text(info['template']) == model['template_sha256'], 'template changed')
    require(digest_text(info['parameters']) == model['parameters_sha256'], 'sampling changed')
    return {'deployment_receipt_sha256': sha(args.deployment), 'model_receipt_sha256': sha(args.model),
            'settings_sha256': sha(args.settings), 'process': state, 'profile': profile,
            'template_sha256': model['template_sha256'], 'parameters_sha256': model['parameters_sha256'],
            'model_manifest': tag['digest']}


def gpu(args):
    model = read(args.model)['model']
    resident = http('http://127.0.0.1:11434/api/ps')['models']
    require(all(m['name'] == model for m in resident), 'another model owns the GPU')
    if args.phase == 'native' and resident:
        command(['ollama', 'stop', model])
        resident = []
    processes = command(['nvidia-smi', '--query-compute-apps=pid,process_name,used_gpu_memory', '--format=csv,noheader'])
    for line in processes.splitlines():
        name = line.split(',')[1].strip()
        require(name == '/home/juan-canfield/Downloads/Godot_v4.7.2-stable_linux.x86_64'
                or (args.phase != 'native' and resident and name == '/usr/local/bin/ollama'), 'another GPU process is active')
    total, used = map(int, command(['nvidia-smi', '--query-gpu=memory.total,memory.used', '--format=csv,noheader,nounits']).split(','))
    require(resident or total - used >= 12000, 'insufficient free GPU memory')
    return processes


def validate_receipt(receipt, source, inputs, runtime, required_phase):
    require(receipt.get('version') == 2, 'receipt version')
    require(receipt.get('phase') == required_phase, 'wrong receipt phase')
    require(receipt.get('passed') is True, 'receipt failed/incomplete')
    require(receipt.get('source') == source, 'stale source receipt')
    require(receipt.get('inputs') == inputs, 'changed input or labels')
    require(receipt.get('runtime') == runtime, 'changed runtime receipt')
    calls = receipt.get('actual_calls')
    require(type(calls) is int and calls == (30 if required_phase == 'gateway' else 0), 'wrong call count')
    artifacts = receipt.get('artifacts', {})
    required = {'freeze.json', 'exit.json', 'run.log'}
    if required_phase == 'gateway':
        required |= {'results.json', 'admission.json', 'runtime.json'}
        required |= {f'{kind}-{i}.json' for kind in ['planned', 'request', 'response', 'provenance'] for i in range(30)}
        required |= {f'case-{i}.json' for i in range(30)}
        require(receipt.get('native_evidence'), 'missing native parity receipt')
    else:
        required |= {'native-framing.json'} | {f'framing-{i}.json' for i in range(30)}
    require(required.issubset(artifacts), 'missing artifact inventory')


def verify_artifacts(path, receipt):
    for name, expected in receipt['artifacts'].items():
        relative = Path(name)
        require(not relative.is_absolute() and '..' not in relative.parts, 'invalid artifact path')
        require(sha(path.parent / relative) == expected, f'changed artifact: {name}')


def verify_native(receipt, source, inputs, runtime):
    evidence = receipt['native_evidence']
    path = Path(evidence['receipt_path'])
    require(sha(path) == evidence['sha256'], 'native receipt changed')
    native = read(path)
    validate_receipt(native, source, inputs, runtime, 'native')
    verify_artifacts(path, native)


def correction_history(packet):
    path = packet / 'parity-investigation-result.json'
    require(sha(path) == '89a2ebae706e55abe9612b1295776ce28ea31064c2e6a44122c73cf53a37ffe9',
            'accepted investigation changed')
    prior = read(path)
    for name, expected in prior['evidence'].items():
        require(sha(packet / name) == expected, 'investigation evidence changed')
    require(prior['total_calls'] == 210 and prior['remaining_calls'] == 360, 'prior budget changed')
    require(sha(packet / 'RESTORATION-AMENDMENT.md') ==
            '6c725b88e9f65e51705059567a0170f6b71dddf4e0cd909c924c3135117bf3c9',
            'accepted representation amendment changed')
    return {'prior_calls': 210, 'planned_calls': 30, 'total_after': 240, 'hard_ceiling': 570}


def reserve_gateway(packet, source, output):
    # An interrupted correction keeps its reservation. Never rerun by changing
    # the output path or overwriting the earlier published-production receipt.
    budget = correction_history(packet)
    save(packet / 'correction-gateway-reservation.json', {
        'source': source, 'output': str(output), 'maximum_calls': 30,
        'phase': 'corrected-production', **budget})


def summary_lane(args, source):
    require(args.output is not None and args.gguf is not None and args.server is not None,
            'summary output/model/server are required')
    m = read(REPO / MANIFEST_PATH)['additional_tasks'][0]
    require(sha(args.gguf) == m['model_sha256'] and sha(args.server) == m['server_sha256'],
            'native summary runtime pin changed')
    require(sha(args.packet / 'execution.json') == m['execution_sha256'], 'summary freeze changed')
    out = args.output.resolve()
    require(not out.is_relative_to(REPO) and '.codex/worktrees' not in str(out), 'durable output required')
    require(not out.exists(), 'summary output already exists; no retry')
    require(not (args.packet.parent / 'native-summary-parity-generation-reservation.json').exists(), 'summary budget already reserved')
    with args.lock.open('r+') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        out.mkdir(parents=True, mode=0o700)
        out.parent.chmod(0o700)
        save(out / 'freeze.json', {'source':source, 'task':m, 'generation_ceiling':2})
        env = {k:v for k,v in os.environ.items() if not k.startswith(('DOC_SUM_', 'LLAMA_ARG_'))}
        env.update(DOC_SUM_SUMMARY_ORIGINAL=str(args.packet.resolve()),
                   DOC_SUM_SUMMARY_PARITY_OUTPUT=str(out), DOC_SUM_QUALIFICATION_GGUF=str(args.gguf),
                   DOC_SUM_LLAMA_SERVER_PATH=str(args.server))
        test = 'pipeline::llama_cpp::framing_tests::original_summary_native_adapter_parity'
        cmd = ['cargo','test','--locked','--all-features','--lib',test,'--','--exact','--ignored','--nocapture','--test-threads=1']
        with (out / 'run.log').open('x') as log:
            proc = subprocess.Popen(cmd,cwd=REPO / 'src-tauri',env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
            for line in proc.stdout:
                log.write(line);log.flush()
                if line.startswith(('SUMMARY_', 'test result:', 'error:', "thread '")):
                    print(line.strip(),flush=True)
            code=proc.wait()
        save(out / 'exit.json', {'code':code})
        if code:
            print(f'Native summary check exited {code}; stopped at {out}',flush=True)
            return code
        result=read(out / 'results.json') if (out / 'results.json').exists() else {'passed':False,'calls':0}
        require(type(result['calls']) is int and 0 <= result['calls'] <= 2, 'summary budget exceeded')
        require(not result['passed'] or (result['calls']==2 and len(result.get('cases',[]))==2 and all(c['passed'] for c in result['cases'])), 'incomplete summary cannot pass')
        require(tree() == source, 'source changed during native summary parity')
        receipt={'version':1,'phase':'native-summary','at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
                 'source':source,'task':m,'passed':result['passed'],'actual_calls':result['calls'],
                 'artifacts':{str(f.relative_to(out)):sha(f) for f in out.rglob('*') if f.is_file()}}
        save(out / 'receipt.json',receipt)
        print(json.dumps({'phase':'native-summary','passed':receipt['passed'],'actual_calls':receipt['actual_calls'],'receipt':str(out / 'receipt.json')}),flush=True)
        return 0


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--phase', choices=['native', 'preflight', 'gateway', 'summary'], required=True)
    for key in ['packet', 'lock']:
        p.add_argument('--' + key, type=Path, required=True)
    for key in ['deployment', 'model', 'settings']:
        p.add_argument('--' + key, type=Path)
    for key in ['output', 'verify', 'native-receipt', 'gguf', 'server']:
        p.add_argument('--' + key, type=Path)
    args = p.parse_args()
    os.umask(0o077)
    source = tree()
    if args.phase == 'summary':
        return summary_lane(args, source)
    require(all(getattr(args,k) is not None for k in ['deployment','model','settings']), 'gateway receipts/settings required')
    inputs = baseline(args.packet)
    with contextlib.ExitStack() as stack:
        # Preflight and receipt inspection cannot generate or change GPU residency.
        if args.phase != 'preflight' and not args.verify:
            lock = stack.enter_context(args.lock.open('r+'))
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        runtime = audit(args)
        if args.verify:
            require(args.phase != 'preflight', 'preflight is not qualification')
            receipt = read(args.verify)
            validate_receipt(receipt, source, inputs, runtime, args.phase)
            verify_artifacts(args.verify, receipt)
            if args.phase == 'gateway':
                verify_native(receipt, source, inputs, runtime)
            print('Receipt accepted for exact source, inputs and runtime')
            return 0
        require(args.output is not None, '--output required')
        out = args.output.resolve()
        require('/.codex/worktrees/' not in str(out) and not out.is_relative_to(REPO), 'evidence must be outside worktree')
        correction_budget = correction_history(args.packet) if args.phase == 'gateway' else None
        native_evidence = None
        if args.phase == 'gateway':
            require(args.native_receipt is not None, 'mandatory native receipt required')
            native = read(args.native_receipt)
            validate_receipt(native, source, inputs, runtime, 'native')
            verify_artifacts(args.native_receipt, native)
            native_evidence = {'receipt_path': str(args.native_receipt.resolve()), 'sha256': sha(args.native_receipt)}
        out.mkdir(mode=0o700)  # Existing evidence is never reused or overwritten.
        if args.phase != 'preflight':
            gpu(args)
        save(out / 'freeze.json', {'source': source, 'inputs': inputs, 'runtime': runtime,
                                  'correction_budget': correction_budget, 'phase': args.phase, 'seed': 7, 'output_tokens': 4096,
                                  'context_tokens': 32768, 'planned_calls': 30 if args.phase == 'gateway' else 0})
        env = {k: v for k, v in os.environ.items() if not k.startswith(('DOC_SUM_', 'LLAMA_ARG_'))}
        env.update(DOC_SUM_C9_PARITY_OUTPUT=str(out), DOC_SUM_C9_ORIGINAL=str(args.packet / 'original-c9'),
                   DOC_SUM_C9_GATEWAY_SETTINGS=str(args.settings),
                   DOC_SUM_C9_DEPLOYMENT=read(args.deployment)['deployment_id'], DOC_SUM_C9_POLICY='2')
        env['DOC_SUM_C9_PROCESS'] = json.dumps(runtime['process'], sort_keys=True)
        if args.phase == 'native':
            require(args.gguf is not None and args.server is not None, 'native model/server paths required')
            env.update(DOC_SUM_QUALIFICATION_GGUF=str(args.gguf), DOC_SUM_LLAMA_SERVER_PATH=str(args.server))
        elif args.phase == 'preflight':
            env['DOC_SUM_C9_BOUNDARIES_ONLY'] = '1'
        test = NATIVE_TEST if args.phase == 'native' else TEST
        cmd = ['cargo', 'test', '--locked', '--lib', test, '--', '--exact', '--ignored', '--nocapture', '--test-threads=1']
        if args.phase == 'gateway':
            reserve_gateway(args.packet, source, out)
        print(json.dumps({'phase': args.phase, 'head': source['head'], 'planned_calls': 30 if args.phase == 'gateway' else 0}), flush=True)
        with (out / 'run.log').open('x') as log:
            proc = subprocess.Popen(cmd, cwd=REPO / 'src-tauri', env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            for line in proc.stdout:
                log.write(line); log.flush()
                if line.startswith(('C9_', 'test result:')):
                    print(line.strip(), flush=True)
            code = proc.wait()
        save(out / 'exit.json', {'code': code})
        if code:
            print(f'Rust check exited {code}; stopped with raw evidence at {out}', flush=True)
            return code
        require(source == tree(), 'source changed during run')
        require(runtime == audit(args), 'runtime changed during run')
        if args.phase == 'native':
            result = read(out / 'native-framing.json')
            passed, calls = result['gate_passed'], result['generation_calls']
        elif args.phase == 'gateway':
            result = read(out / 'results.json')
            passed, calls = result['gateway_parity_passed'], result['actual_calls']
        else:
            result = read(out / 'admission.json')
            passed, calls = result['admitted'], 0
        receipt = {'version': 2, 'at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                   'phase': args.phase, 'passed': passed, 'actual_calls': calls,
                   'source': source, 'inputs': inputs, 'runtime': runtime, 'native_evidence': native_evidence,
                   'artifacts': {str(f.relative_to(out)): sha(f) for f in out.rglob('*') if f.is_file()}}
        save(out / 'receipt.json', receipt)
        print(json.dumps({'phase': args.phase, 'passed': passed, 'actual_calls': calls, 'receipt': str(out / 'receipt.json')}), flush=True)
        # Semantic mismatches are experiment outcomes, not a reason to rerun.
        return 0


if __name__ == '__main__':
    sys.exit(main())
