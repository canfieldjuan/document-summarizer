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
import stat
import subprocess
import sys
import re
import urllib.request

REPO = Path(__file__).resolve().parents[1]
MANIFEST_PATH = Path('src-tauri/src/pipeline/summary/comparisons/fixtures/c9-parity-manifest.json')
SUMMARY_MANIFEST_PATH = Path('src-tauri/src/pipeline/summary/comparisons/fixtures/native-summary-parity-manifest.json')
SUMMARY_MANIFEST_SHA = '5ed3e4c777eb6dd6da0f2c8cf03d3b084f11969f04e7f65ae99af11ebe8fd4cc'
OFFLINE_MANIFEST = Path('src-tauri/src/pipeline/summary/comparisons/fixtures/whole-writer-baseline-manifest.json')
OFFLINE_MANIFEST_SHA = 'adf2ec146f17343c967179d8b14f39107b0300729959d17b5d5973dd8389d786'
OFFLINE_TESTS = [
    ('counter', 'pipeline::llama_cpp::framing_tests::offline_native_counter_matches_recorded_counts', True),
    ('dependencies', 'pipeline::summary::coherent::readiness::whole_document_dependency_frozen_source_measurement', True),
    ('capacity', 'pipeline::summary::coherent::readiness::whole_writer_offline_native_capacity_measurement', True),
    ('assembly', 'pipeline::summary::coherent::readiness::whole_writer_assembly_replays_the_frozen_public_candidate_packet', True),
    ('consolidation-boundary', 'pipeline::summary::coherent::readiness::whole_writer_original_consolidation_request_still_exceeds_native_context', True),
    ('c9', 'pipeline::summary::comparisons::tests::parity::archived_c9_verdicts_survive_relation_guard', False),
    ('same60', 'pipeline::summary::relation_guard_precision_inventory::archived_delivery_uses_production_getters_and_validation', True),
]
TEST = 'pipeline::summary::comparisons::tests::parity::original_c9_production_gateway_parity'
NATIVE_TEST = 'pipeline::llama_cpp::framing_tests::original_c9_native_framing_parity'
EVIDENCE_ROOTS = (Path.home() / 'Desktop/codex-evidence/document-summarizer',
                  Path.home() / 'Desktop/doc-classify-corpus/heldout')


def require(value, message):
    if not value:
        raise ValueError(message)


def prepare_evidence_output(path):
    """Prepare only this run's new leaf; existing evidence is never reused."""
    require(not path.exists() and not path.is_symlink(), 'output already exists; no retry')
    out = path.resolve()
    require(not out.is_relative_to(REPO) and '.codex/worktrees' not in str(out),
            'durable output required outside worktrees')
    require(any(out != root.resolve() and out.is_relative_to(root.resolve())
                for root in EVIDENCE_ROOTS), 'output must be beneath a durable evidence namespace')
    parent = out.parent.stat()  # Missing parents are refused, never created.
    require(stat.S_ISDIR(parent.st_mode) and parent.st_uid == os.getuid()
            and stat.S_IMODE(parent.st_mode) == 0o700,
            'output parent must be an existing user-owned directory with mode 0700')
    out.mkdir(mode=0o700)  # Exclusive create also refuses a concurrent existing leaf.
    try:
        out.chmod(0o700)  # Only the newly owned leaf; exact mode even under a restrictive umask.
    except OSError:
        try:
            out.rmdir()  # Empty owned leaf only; never recursive cleanup.
        except OSError:
            pass  # Preserve unexpected contents and propagate the original preparation error.
        raise
    return out


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


def offline_source():
    source = tree()
    # A prose-only follow-up does not invalidate exercised code. Any source,
    # test, input, build or gate change does, including a change to this owner.
    return {'files': {p: h for p, h in source['files'].items()
                      if p.startswith(('src-tauri/', 'scripts/', 'src/', '.github/'))
                      or p in ('package.json', 'package-lock.json', 'vite.config.ts')}}


def offline_manifest():
    require(sha(REPO / OFFLINE_MANIFEST) == OFFLINE_MANIFEST_SHA, 'offline baseline manifest changed')
    return read(REPO / OFFLINE_MANIFEST)


def require_exact_test_pass(log):
    require(len(re.findall(r'^test result: ok\. 1 passed; 0 failed; 0 ignored;', log, re.M)) == 1,
            'offline gate requires exactly one exercised passing test')
    require('test result: FAILED' not in log, 'offline test failed')


def verify_frozen_inputs(frozen, pins):
    require(isinstance(frozen,dict) and set(frozen)==set(pins['inputs']), 'offline input inventory changed')
    for name,spec in frozen.items():
        require(spec['sha256']==pins['inputs'][name] and sha(spec['path'])==spec['sha256'], 'frozen input changed: '+name)
    # The source packet pins the original document bytes, not just its index.
    for source in read(frozen['source_packet']['path'])['inputs']:
        require(sha(source['path'])==source['sha256'], 'frozen document bytes changed')


def verify_summary_capture(packet):
    summary=summary_manifest()
    for name,expected in offline_manifest()['summary_capture_sources'].items():
        require(sha(packet/name)==expected, 'summary baseline source/config changed: '+name)
    require(sha(packet/'execution.json')==summary['execution_sha256'], 'summary execution changed')
    for case in summary['cases']:
        for name,expected in case['files'].items():
            require(sha(packet/name)==expected, 'summary capture changed: '+name)
    return summary


def canonical_digest(value):
    return digest_text(json.dumps(value,sort_keys=True,separators=(',',':')))


def verify_offline_results(directory,pins):
    require(sha(directory/'dependencies.json')==pins['dependency_report_sha256'],
            'dependency ownership differs from the reviewed complete reference list')
    capacity=read(directory/'capacity.json')
    require(capacity.get('status')=='draft_planning_only','capacity is diagnostic/incomplete, not admission proof')
    rows=capacity.get('inputs',[])
    require(len(rows)==len(pins['writer_source_ownership']) and
            {row['result']['alias'] for row in rows}==set(pins['writer_source_ownership']), 'missing or duplicate capacity input')
    keys=('normalized','source_units','anchors','omitted_standalone_quote_segments')
    for row in rows:
        alias=row['result']['alias']
        require(canonical_digest({k:row[k] for k in keys})==pins['writer_source_ownership'][alias],
                'source spans/citations/ownership changed: '+alias)
        require(all(type(row.get(k)) is int and row[k]==0 for k in
                    ('governing_units_with_zero_anchors','contextual_anchor_coverage_gaps')), 'source coverage is incomplete')
        require(row['result']['status']=='drafts_admitted' and row['result']['drafts'], 'no admitted drafts')
        for draft in row['result']['drafts']:
            require(draft['maximum_unit_characters']==1200 and type(draft.get('maximum_units')) is int and
                    0<draft['maximum_units']<=64 and type(draft.get('output_allowance_tokens')) is int and
                    draft['output_allowance_tokens']>0, 'invalid section response allowance')
    assembly=read(directory/'assembly.json')['assembly']
    require(canonical_digest({k:assembly[k] for k in ('claims','evidence')})==pins['canonical_assembly_sha256'],
            'frozen assembly claims or citations changed')
    require(len(assembly['lineage'])==len(assembly['claims'])==40,'frozen assembly lineage is incomplete')
    require(sha(directory/'same60.json')==pins['same60_expected_sha256'],'SAME60 complete verdict lists changed')


def require_offline_baseline(path):
    receipt = read(path)
    require(receipt.get('phase') == 'writer-offline' and receipt.get('passed') is True,
            'mandatory offline baseline has not passed')
    require(type(receipt.get('actual_calls')) is int and receipt['actual_calls'] == 0,
            'offline baseline must have zero model calls')
    require(receipt.get('source') == offline_source(), 'offline baseline is stale for this source')
    require(receipt.get('baseline') == offline_manifest(), 'offline baseline inputs changed')
    verify_frozen_inputs(receipt.get('frozen_inputs'),receipt['baseline'])
    verify_summary_capture(Path(receipt['summary_packet']))
    expected = [name for name, _, _ in OFFLINE_TESTS]
    commands = receipt.get('commands', [])
    require([row.get('name') for row in commands] == expected, 'incomplete offline baseline commands')
    require(all(type(row.get('exit')) is int and row['exit'] == 0 for row in commands), 'offline command failed')
    artifacts = receipt.get('artifacts', {})
    require(all(name + '.log' in artifacts for name in expected), 'missing offline test artifacts')
    require({'summary-baseline.json', 'dependencies.json', 'capacity.json', 'assembly.json', 'same60.json'}.issubset(artifacts),
            'missing baseline result artifacts')
    verify_artifacts(path, receipt)
    verify_offline_results(path.parent,receipt['baseline'])
    for name in expected:
        require_exact_test_pass((path.parent / (name + '.log')).read_text())
    return receipt


def check_offline_push(updates, pointer):
    rows = [line.split() for line in updates.decode().splitlines() if line.strip()]
    require(all(len(row)==4 for row in rows), 'invalid Git push update')
    live = [row for row in rows if row[1] != '0'*40]
    if not live: return
    head = command(['git','rev-parse','HEAD'])
    require(all(row[1]==head for row in live), 'push must use the tested current checkout')
    require(pointer.is_file(), 'missing mandatory offline proof; run --phase writer-offline first')
    pin = read(pointer)
    path = Path(pin['path'])
    require(sha(path)==pin['sha256'], 'offline receipt changed')
    require_offline_baseline(path)


def install_offline_gate():
    common = (REPO / command(['git','rev-parse','--git-common-dir'])).resolve()
    target=common/'hooks/pre-push'
    desired=(REPO/'scripts/preproduction-pre-push.py').read_bytes()
    if target.exists() and target.read_bytes()!=desired and b'baseline policy belongs to c9-preproduction-parity.py' not in target.read_bytes():
        prior=target.with_name('pre-push.before-docsum-baseline')
        require(not prior.exists(), 'existing hook backup requires reconciliation')
        require(os.access(target,os.X_OK), 'existing hook is not executable')
        target.rename(prior)  # Retain and chain; no existing hook is discarded.
    target.write_bytes(desired)
    target.chmod(0o700)
    print('Installed chained native Git baseline gate: '+str(target),flush=True)


def run_offline_baseline(args):
    baseline_pins = offline_manifest()
    frozen = read(args.writer_freeze)
    verify_frozen_inputs(frozen,baseline_pins)
    require(sha(REPO / SUMMARY_MANIFEST_PATH) == baseline_pins['summary_manifest_sha256'], 'summary baseline manifest changed')
    summary = verify_summary_capture(args.packet)
    observed = []
    for case in summary['cases']:
        wire = read(args.packet / f"case-{case['case']}-wire-request.json")
        response = read(args.packet / f"case-{case['case']}-response.raw.json")
        observed.append({'alias':case['alias'], 'max_tokens':wire['max_tokens'],
                         'response_format':wire.get('response_format'), 'usage':response['usage']})
    source = offline_source()
    out = prepare_evidence_output(args.output)
    save(out / 'summary-baseline.json', {'baseline':baseline_pins, 'observed':observed})
    environment = {k:v for k,v in os.environ.items() if not k.startswith(('DOC_SUM_', 'LLAMA_ARG_'))}
    environment.update(DOC_SUM_OFFLINE_COUNT_INVENTORY=frozen['count_inventory']['path'],
        DOC_SUM_OFFLINE_COUNT_INVENTORY_SHA256=frozen['count_inventory']['sha256'],
        DOC_SUM_DEPENDENCIES_INPUT=frozen['source_packet']['path'], DOC_SUM_DEPENDENCIES_OUTPUT=str(out/'dependencies.json'),
        DOC_SUM_WRITER_CAPACITY_OUTPUT=str(out/'capacity.json'),
        DOC_SUM_ASSEMBLY_PUBLIC_FREEZE=frozen['assembly_public']['path'], DOC_SUM_ASSEMBLY_PUBLIC_FREEZE_SHA256=frozen['assembly_public']['sha256'],
        DOC_SUM_ASSEMBLY_REPLAY_OUTPUT=str(out/'assembly.json'),
        DOC_SUM_FROZEN_CONSOLIDATION_MEASUREMENT=frozen['consolidation']['path'], DOC_SUM_FROZEN_CONSOLIDATION_MEASUREMENT_SHA256=frozen['consolidation']['sha256'],
        DOC_SUM_GUARD_PRECISION_ROOT=str(Path(frozen['same60_freeze']['path']).parent), DOC_SUM_GUARD_PRECISION_OUTPUT=str(out/'same60.json'))
    commands = []
    for name, selector, ignored in OFFLINE_TESTS:
        cmd = ['cargo', 'test', '--locked', '--all-features', '--lib', selector, '--', '--exact', '--nocapture']
        if ignored: cmd.append('--ignored')
        with (out/(name+'.log')).open('x') as log:
            result = subprocess.run(cmd, cwd=REPO/'src-tauri', env=environment, stdout=log, stderr=subprocess.STDOUT)
        save(out/(name+'-exit.json'), {'code':result.returncode, 'command':cmd})
        require(result.returncode == 0, 'offline baseline STOP: ' + name)
        require_exact_test_pass((out/(name+'.log')).read_text())
        commands.append({'name':name, 'exit':result.returncode, 'command':cmd})
        if name == 'dependencies':
            require(all(row['counts']['unresolved']==0 and row['blocked_units']==0 for row in read(out/'dependencies.json')['inputs']),
                    'source dependency STOP')
        if name == 'same60':
            require(sha(out/'same60.json') == baseline_pins['same60_expected_sha256'], 'SAME60 complete verdict lists changed')
        print('OFFLINE_BASELINE_PASS ' + name, flush=True)
    require(source == offline_source(), 'source changed during offline baseline')
    verify_offline_results(out,baseline_pins)
    save(out/'receipt.json', {'phase':'writer-offline', 'passed':True, 'actual_calls':0,
        'source':source, 'baseline':baseline_pins, 'commands':commands,
        'frozen_inputs':frozen,'summary_packet':str(args.packet.resolve()),
        'artifacts':{str(p.relative_to(out)):sha(p) for p in out.rglob('*') if p.is_file()}})
    require_offline_baseline(out/'receipt.json')
    common=(REPO / command(['git','rev-parse','--git-common-dir'])).resolve()
    pointer=common/'docsum-offline-baseline.json'
    pointer.write_text(json.dumps({'path':str(out/'receipt.json'),'sha256':sha(out/'receipt.json')}))
    pointer.chmod(0o600)
    print('OFFLINE_BASELINE_RECEIPT ' + str(out/'receipt.json'), flush=True)
    return 0


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


def summary_manifest():
    require(sha(REPO / SUMMARY_MANIFEST_PATH) == SUMMARY_MANIFEST_SHA, 'summary manifest changed')
    return read(REPO / SUMMARY_MANIFEST_PATH)


REPAIR_CONTRACT_PATH = Path('docs/PR-NATIVE-MESSAGE-BOUNDARY-REPAIR.md')
REPAIR_PROPOSAL_SHA = 'd39b7edf442e678db8aefd8e9a5eae1154c53216ea4720697d1710246f1e7e07'
REPAIR_RESERVATION = 'native-message-boundary-repair-generation-reservation.json'


def repair_preflight_hashes(packet):
    root = packet.parent / 'native-message-boundary-repair-preflight'
    result = {}
    for i in [1, 2]:
        for suffix in ['candidate-wire', 'oracle-verified']:
            name = f'case-{i}-{suffix}.json'
            result[name] = sha(root / name)
        oracle = read(root / f'case-{i}-oracle-verified.json')
        require(oracle.get('candidate_match') is True and oracle.get('full_actual_input_tokens_equal') is True
                and oracle.get('metadata_only_sampler_equal') is True and type(oracle.get('generation_calls')) is int and oracle['generation_calls'] == 0
                and oracle.get('context_created') is False, 'offline repair oracle failed')
    return result


def validate_repair_approval(approval, source, packet):
    require(approval.get('operator_message') == 'i accept production repair'
            and approval.get('proposal_sha256') == REPAIR_PROPOSAL_SHA, 'missing repair acceptance')
    require(type(approval.get('maximum_calls')) is int and approval['maximum_calls'] == 2, 'repair budget changed')
    contract = sha(REPO / REPAIR_CONTRACT_PATH)
    require(approval.get('accepted_contract_sha256') == contract, 'accepted repair contract changed')
    require(approval.get('source') == source, 'stale repair source')
    prior = sha(packet.parent / 'native-summary-parity-generation-reservation.json')
    require(approval.get('prior_reservation_sha256') == prior, 'repair history changed')
    preflight = repair_preflight_hashes(packet)
    require(approval.get('preflight_hashes') == preflight, 'repair preflight changed')
    return {'accepted_contract_sha256': contract, 'maximum_calls': 2,
            'source': source, 'prior_reservation_sha256': prior, 'preflight_hashes': preflight}


def reserve_summary_repair(packet, approval_path, source, output):
    value = validate_repair_approval(read(approval_path), source, packet)
    value.update(approval_sha256=sha(approval_path), output=str(output))
    save(packet.parent / REPAIR_RESERVATION, value)
    return value


def summary_gpu_idle():
    queue = http('http://127.0.0.1:8189/queue')
    require(not queue.get('queue_running') and not queue.get('queue_pending'), 'ComfyUI is busy')


def summary_lane(args, source):
    require(args.output is not None and args.gguf is not None and args.server is not None,
            'summary output/model/server are required')
    m = summary_manifest()
    require(sha(args.gguf) == m['model_sha256'] and sha(args.server) == m['server_sha256'],
            'native summary runtime pin changed')
    require(sha(args.packet / 'execution.json') == m['execution_sha256'], 'summary freeze changed')
    repair = getattr(args, 'repair_acceptance', None)
    if repair:
        validate_repair_approval(read(repair), source, args.packet)
        require(not (args.packet.parent / REPAIR_RESERVATION).exists(), 'repair budget already reserved')
        summary_gpu_idle()
    else:
        require(not (args.packet.parent / 'native-summary-parity-generation-reservation.json').exists(), 'summary budget already reserved')
    with args.lock.open('r+') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        out = prepare_evidence_output(args.output)
        save(out / 'freeze.json', {'source':source, 'task':m, 'generation_ceiling':2})
        env = {k:v for k,v in os.environ.items() if not k.startswith(('DOC_SUM_', 'LLAMA_ARG_'))}
        env.update(DOC_SUM_SUMMARY_ORIGINAL=str(args.packet.resolve()),
                   DOC_SUM_SUMMARY_PARITY_OUTPUT=str(out), DOC_SUM_QUALIFICATION_GGUF=str(args.gguf),
                   DOC_SUM_LLAMA_SERVER_PATH=str(args.server))
        if repair:
            reservation = reserve_summary_repair(args.packet, repair, source, out)
            env['DOC_SUM_SUMMARY_REPAIR_CONTRACT_SHA'] = reservation['accepted_contract_sha256']
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
        if repair:
            summary_gpu_idle()
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
    p.add_argument('--phase', choices=['native', 'preflight', 'gateway', 'summary', 'writer-offline', 'check-offline', 'install-offline-gate'], required=True)
    for key in ['packet', 'lock']:
        p.add_argument('--' + key, type=Path)
    for key in ['deployment', 'model', 'settings']:
        p.add_argument('--' + key, type=Path)
    for key in ['output', 'verify', 'native-receipt', 'gguf', 'server', 'repair-acceptance']:
        p.add_argument('--' + key, type=Path)
    p.add_argument('--writer-freeze', type=Path)
    p.add_argument('--offline-baseline-receipt', type=Path)
    args = p.parse_args()
    os.umask(0o077)
    if args.phase == 'install-offline-gate':
        install_offline_gate()
        return 0
    if args.phase == 'check-offline':
        require(args.verify is not None, 'offline receipt required')
        require_offline_baseline(args.verify)
        print('Offline baseline accepted for current source and frozen inputs')
        return 0
    if args.phase == 'writer-offline':
        require(args.packet is not None and args.writer_freeze is not None and args.output is not None, 'summary packet, writer freeze and durable output required')
        return run_offline_baseline(args)
    require(args.packet is not None and args.lock is not None, 'packet and lock required')
    if args.phase in ('summary','gateway') and args.verify is None:
        require(args.offline_baseline_receipt is not None, 'mandatory offline baseline receipt required before model launch')
        require_offline_baseline(args.offline_baseline_receipt)
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
        correction_budget = correction_history(args.packet) if args.phase == 'gateway' else None
        native_evidence = None
        if args.phase == 'gateway':
            require(args.native_receipt is not None, 'mandatory native receipt required')
            native = read(args.native_receipt)
            validate_receipt(native, source, inputs, runtime, 'native')
            verify_artifacts(args.native_receipt, native)
            native_evidence = {'receipt_path': str(args.native_receipt.resolve()), 'sha256': sha(args.native_receipt)}
        out = prepare_evidence_output(args.output)
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
