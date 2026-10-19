import copy
import contextlib
import importlib.util
import os
from pathlib import Path
import stat
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('parity', Path(__file__).with_name('c9-preproduction-parity.py'))
parity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parity)


class ReachedLaunch(Exception):
    """Stop before any Rust subprocess or model call."""


class OfflineBaselineGateTests(unittest.TestCase):
    def test_native_push_gate_refuses_missing_and_stale_proof(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);pointer=root/'pointer.json';head='a'*40
            update=f'refs/heads/codex/test {head} refs/heads/codex/test '+('0'*40)+'\n'
            with mock.patch.object(parity,'command',return_value=head),mock.patch.object(parity,'require_offline_baseline') as proof:
                with self.assertRaisesRegex(ValueError,'missing mandatory offline proof'):
                    parity.check_offline_push(update.encode(),pointer)
                import json
                receipt=root/'receipt.json';receipt.write_text('{}')
                pointer.write_text(json.dumps({'path':str(receipt),'sha256':parity.sha(receipt)}))
                parity.check_offline_push(update.encode(),pointer);proof.assert_called_once_with(receipt)
                receipt.write_text('changed')
                with self.assertRaisesRegex(ValueError,'receipt changed'):
                    parity.check_offline_push(update.encode(),pointer)
                with self.assertRaisesRegex(ValueError,'tested current checkout'):
                    parity.check_offline_push(update.replace(head,'b'*40).encode(),pointer)
                with self.assertRaisesRegex(ValueError,'invalid Git push'):
                    parity.check_offline_push(b'partial update\n',pointer)

    def test_hook_install_is_idempotent_and_preserves_prior_hook(self):
        with tempfile.TemporaryDirectory() as tmp:
            common=Path(tmp);hooks=common/'hooks';hooks.mkdir()
            hook=hooks/'pre-push';original=b'#!/bin/sh\nexit 9\n';hook.write_bytes(original);hook.chmod(0o700)
            with mock.patch.object(parity,'command',return_value=str(common)):
                parity.install_offline_gate();parity.install_offline_gate()
            self.assertEqual((hooks/'pre-push.before-docsum-baseline').read_bytes(),original)
            self.assertEqual(hook.read_bytes(),(parity.REPO/'scripts/preproduction-pre-push.py').read_bytes())

    def test_native_hook_honors_prior_hook_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            parity.subprocess.run(['git','init','--quiet',str(root)],check=True)
            prior=root/'.git/hooks/pre-push.before-docsum-baseline'
            prior.write_text('#!/bin/sh\nexit 9\n');prior.chmod(0o700)
            hook=parity.REPO/'scripts/preproduction-pre-push.py'
            result=parity.subprocess.run([parity.sys.executable,str(hook)],cwd=root,input=b'',capture_output=True)
            self.assertEqual(result.returncode,9,result.stderr)

    def test_native_hook_inspects_pushed_commit_not_current_checkout(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            def git(*args):
                return parity.subprocess.check_output(['git',*args],cwd=root,text=True,stderr=parity.subprocess.STDOUT).strip()
            git('init','--quiet');git('config','user.name','Baseline fixture');git('config','user.email','fixture@example.invalid')
            (root/'README').write_text('public fixture')
            git('add','README');git('commit','--quiet','-m','before writer');old=git('rev-parse','HEAD')
            marker=root/'src-tauri/src/pipeline/summary/coherent/whole_document.rs'
            marker.parent.mkdir(parents=True);marker.write_text('// public writer fixture')
            git('add','src-tauri');git('commit','--quiet','-m','writer');writer=git('rev-parse','HEAD')
            git('switch','--detach',old)
            hook=parity.REPO/'scripts/preproduction-pre-push.py'
            def push(head):
                update=f'refs/heads/codex/fixture {head} refs/heads/codex/fixture '+('0'*40)+'\n'
                return parity.subprocess.run([parity.sys.executable,str(hook)],cwd=root,input=update.encode(),capture_output=True)
            self.assertEqual(push(old).returncode,0,'unrelated branches do not adopt this writer gate')
            result=push(writer)
            self.assertNotEqual(result.returncode,0,'another checkout must not bypass the writer baseline')
            self.assertIn(b'tested checkout',result.stderr)

    def test_no_matching_test_is_not_a_pass(self):
        for text in ['test result: ok. 0 passed; 0 failed; 0 ignored;',
                     'test result: ok. 0 passed; 0 failed; 1 ignored;',
                     'test result: FAILED. 1 passed; 1 failed; 0 ignored;']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                parity.require_exact_test_pass(text)
        parity.require_exact_test_pass('test result: ok. 1 passed; 0 failed; 0 ignored;')

    def test_receipt_refuses_missing_stale_inputs_and_changed_artifacts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            receipt=root/'receipt.json'
            source={'files':{'source.rs':'current'}}
            with mock.patch.object(parity, 'offline_source', return_value=source), \
                    mock.patch.object(parity, 'offline_manifest', return_value={'version':1}), \
                    mock.patch.object(parity,'verify_frozen_inputs'), \
                    mock.patch.object(parity,'verify_offline_results'), \
                    mock.patch.object(parity,'verify_summary_capture'):
                with self.assertRaises(FileNotFoundError): parity.require_offline_baseline(receipt)
                for name,_,_ in parity.OFFLINE_TESTS:
                    (root/(name+'.log')).write_text('test result: ok. 1 passed; 0 failed; 0 ignored;')
                for name in ('summary-baseline.json','dependencies.json','capacity.json','assembly.json','same60.json'):
                    (root/name).write_text('{}')
                value={'phase':'writer-offline','passed':True,'actual_calls':0,
                       'frozen_inputs':{},'summary_packet':str(root),
                       'source':source,'baseline':{'version':1},
                       'artifacts':{p.name:parity.sha(p) for p in root.iterdir()},
                       'commands':[{'name':name,'exit':0} for name,_,_ in parity.OFFLINE_TESTS]}
                import json
                receipt.write_text(json.dumps(value))
                parity.require_offline_baseline(receipt)
                for key,bad in [('passed',False),('passed',1),('actual_calls',False),
                                ('actual_calls',1),('source',{'files':{}}),('baseline',{}),
                                ('commands',[]),('artifacts',{})]:
                    changed=copy.deepcopy(value);changed[key]=bad;receipt.write_text(json.dumps(changed))
                    with self.subTest(key=key,bad=bad),self.assertRaises(ValueError):parity.require_offline_baseline(receipt)
                receipt.write_text(json.dumps(value));(root/'counter.log').write_text('changed proof')
                with self.assertRaises(ValueError):parity.require_offline_baseline(receipt)

    def test_changed_frozen_document_refuses_even_with_unchanged_index(self):
        with tempfile.TemporaryDirectory() as tmp:
            import json
            root=Path(tmp);document=root/'document.txt';document.write_text('public original text')
            packet=root/'packet.json';packet.write_text(json.dumps({'inputs':[{'path':str(document),'sha256':parity.sha(document)}]}))
            frozen={'source_packet':{'path':str(packet),'sha256':parity.sha(packet)}}
            pins={'inputs':{'source_packet':parity.sha(packet)}}
            parity.verify_frozen_inputs(frozen,pins)
            document.write_text('changed document')
            with self.assertRaisesRegex(ValueError,'document bytes changed'):
                parity.verify_frozen_inputs(frozen,pins)

    def test_complete_results_require_admission_ownership_and_full_assembly(self):
        import json
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            def write(name,value): (root/name).write_text(json.dumps(value))
            write('dependencies.json',{});write('same60.json',{})
            ownership={'normalized':'public source','source_units':['unit'],'anchors':['anchor'],'omitted_standalone_quote_segments':0}
            row={**ownership,'governing_units_with_zero_anchors':0,'contextual_anchor_coverage_gaps':0,
                 'result':{'alias':'control','status':'drafts_admitted','drafts':[{'maximum_unit_characters':1200,'maximum_units':2,'output_allowance_tokens':4096}]}}
            capacity={'status':'draft_planning_only','inputs':[row]};write('capacity.json',capacity)
            assembly={'claims':list(range(40)),'evidence':['public'],'lineage':list(range(40))};write('assembly.json',{'assembly':assembly})
            pins={'dependency_report_sha256':parity.sha(root/'dependencies.json'),'same60_expected_sha256':parity.sha(root/'same60.json'),
                  'writer_source_ownership':{'control':parity.canonical_digest(ownership)},
                  'canonical_assembly_sha256':parity.canonical_digest({k:assembly[k] for k in ('claims','evidence')})}
            parity.verify_offline_results(root,pins)
            bad=[]
            changed=copy.deepcopy(capacity);changed['status']='diagnostic_only_not_admission';bad.append(changed)
            changed=copy.deepcopy(capacity);changed['inputs']=[];bad.append(changed)
            changed=copy.deepcopy(capacity);changed['inputs'][0]['normalized']='different source';bad.append(changed)
            for key,value in [('maximum_unit_characters',39),('maximum_units',0),('maximum_units',65),('maximum_units',True),('output_allowance_tokens',False)]:
                changed=copy.deepcopy(capacity);changed['inputs'][0]['result']['drafts'][0][key]=value;bad.append(changed)
            for value in [1,False]:
                changed=copy.deepcopy(capacity);changed['inputs'][0]['contextual_anchor_coverage_gaps']=value;bad.append(changed)
            for changed in bad:
                write('capacity.json',changed)
                with self.assertRaises(ValueError):parity.verify_offline_results(root,pins)
            write('capacity.json',capacity)
            assembly['claims'].pop();write('assembly.json',{'assembly':assembly})
            with self.assertRaises(ValueError):parity.verify_offline_results(root,pins)


class OutputPermissionsTests(unittest.TestCase):
    def summary_args(self, root, out):
        packet = root / 'packet'
        packet.mkdir(exist_ok=True)
        lock = root / 'lock'
        lock.touch()
        return SimpleNamespace(output=out, gguf=root / 'model', server=root / 'server',
                               packet=packet, lock=lock, repair_acceptance=None)

    def invoke_summary(self, root, out, repair=False):
        args = self.summary_args(root, out)
        if repair:
            args.repair_acceptance = root / 'acceptance'
        manifest = {'model_sha256': 'pin', 'server_sha256': 'pin', 'execution_sha256': 'pin'}
        with mock.patch.object(parity, 'EVIDENCE_ROOTS', (root,), create=True), \
                mock.patch.object(parity, 'summary_manifest', return_value=manifest), \
                mock.patch.object(parity, 'sha', return_value='pin'), \
                mock.patch.object(parity, 'read', return_value={}), \
                mock.patch.object(parity, 'validate_repair_approval'), \
                mock.patch.object(parity, 'summary_gpu_idle'), \
                mock.patch.object(parity.subprocess, 'Popen', side_effect=ReachedLaunch) as launch:
            try:
                parity.summary_lane(args, {'head': 'public'})
            except ReachedLaunch:
                pass
            return launch.call_count

    def test_existing_parent_mode_is_unchanged(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            parent = root / 'shared'
            parent.mkdir()
            parent.chmod(0o2775)
            before = stat.S_IMODE(parent.stat().st_mode)
            try:
                self.invoke_summary(root, parent / 'output')
            except ValueError as error:
                # The accepted repair rejects a shared parent before creating a run.
                self.assertIn('parent', str(error))
                self.assertFalse((parent / 'output').exists())
            self.assertEqual(stat.S_IMODE(parent.stat().st_mode), before,
                             'existing parent permissions changed')

    def test_refused_chmod_leaves_no_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            parent = root / 'private'
            parent.mkdir(mode=0o700)
            out = parent / 'output'
            with mock.patch.object(Path, 'chmod', side_effect=PermissionError('public denied chmod')):
                with self.assertRaises(PermissionError):
                    self.invoke_summary(root, out)
            self.assertFalse(out.exists(), 'failed chmod left an output blocking retry')
            self.assertFalse((root / 'native-summary-parity-generation-reservation.json').exists())
            self.assertFalse((root / parity.REPAIR_RESERVATION).exists())

    def test_private_parent_and_restrictive_umask(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            root.chmod(0o700)
            old_umask = os.umask(0o777)
            try:
                with mock.patch.object(parity, 'EVIDENCE_ROOTS', (root,)):
                    out = parity.prepare_evidence_output(root / 'run')
                self.assertEqual(stat.S_IMODE(out.stat().st_mode), 0o700)
                self.assertEqual(stat.S_IMODE(root.stat().st_mode), 0o700)
            finally:
                os.umask(old_umask)

    def test_shared_parents_are_refused_without_mutation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for mode in [0o2775, 0o1777, 0o750]:
                with self.subTest(mode=oct(mode)):
                    root.chmod(mode)
                    with mock.patch.object(parity, 'EVIDENCE_ROOTS', (root,)), \
                            self.assertRaisesRegex(ValueError, 'parent'):
                        parity.prepare_evidence_output(root / 'run')
                    self.assertFalse((root / 'run').exists())
                    self.assertEqual(stat.S_IMODE(root.stat().st_mode), mode)

    def test_missing_and_foreign_owned_parents_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with mock.patch.object(parity, 'EVIDENCE_ROOTS', (root,)):
                with self.assertRaises(FileNotFoundError):
                    parity.prepare_evidence_output(root / 'missing' / 'run')
                self.assertFalse((root / 'missing').exists())
                foreign_uid = root.stat().st_uid + 1
                with mock.patch.object(parity.os, 'getuid', return_value=foreign_uid), \
                        self.assertRaisesRegex(ValueError, 'parent'):
                    parity.prepare_evidence_output(root / 'run')
                self.assertFalse((root / 'run').exists())

    def test_namespace_and_worktree_admission(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            allowed = root / 'durable'
            allowed.mkdir(mode=0o700)
            worktrees = allowed / '.codex' / 'worktrees'
            worktrees.mkdir(parents=True)
            paths = [root / 'outside', root / 'durable-sibling' / 'run',
                     worktrees / 'run', parity.REPO / 'refused-parity-output']
            with mock.patch.object(parity, 'EVIDENCE_ROOTS', (allowed,)):
                for path in paths:
                    with self.subTest(path=path), self.assertRaises(ValueError):
                        parity.prepare_evidence_output(path)
                    self.assertFalse(path.exists())

    def test_existing_file_directory_and_symlinks_are_never_reused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            file = root / 'file'
            file.write_text('retained')
            directory = root / 'directory'
            directory.mkdir(mode=0o750)
            (directory / 'freeze.json').write_text('retained freeze')
            link = root / 'link'
            link.symlink_to(file)
            broken = root / 'broken'
            broken.symlink_to(root / 'absent')
            with mock.patch.object(parity, 'EVIDENCE_ROOTS', (root,)):
                for path in [file, directory, link, broken]:
                    with self.subTest(path=path), self.assertRaisesRegex(ValueError, 'no retry'):
                        parity.prepare_evidence_output(path)
            self.assertEqual(file.read_text(), 'retained')
            self.assertEqual((directory / 'freeze.json').read_text(), 'retained freeze')
            self.assertEqual(stat.S_IMODE(directory.stat().st_mode), 0o750)
            self.assertTrue(link.is_symlink())
            self.assertTrue(broken.is_symlink())

    def test_creation_collision_is_never_chmodded_or_cleaned(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out = root / 'run'
            mkdir = Path.mkdir

            def collision(path, *args, **kwargs):
                mkdir(path, mode=0o750)
                (path / 'other-owner').write_text('retained')
                raise FileExistsError('concurrent output')

            with mock.patch.object(parity, 'EVIDENCE_ROOTS', (root,)), \
                    mock.patch.object(Path, 'mkdir', collision), \
                    mock.patch.object(Path, 'chmod') as chmod, \
                    mock.patch.object(Path, 'rmdir') as rmdir:
                with self.assertRaises(FileExistsError):
                    parity.prepare_evidence_output(out)
                chmod.assert_not_called()
                rmdir.assert_not_called()
            self.assertEqual((out / 'other-owner').read_text(), 'retained')

    def test_permission_failure_allows_retry_only_before_freeze(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out = root / 'run'
            for repair in [False, True]:
                with self.subTest(repair=repair), \
                        mock.patch.object(parity, 'save') as save, \
                        mock.patch.object(parity, 'reserve_summary_repair') as reserve, \
                        mock.patch.object(Path, 'chmod', side_effect=PermissionError('denied leaf')):
                    with self.assertRaises(PermissionError):
                        self.invoke_summary(root, out, repair=repair)
                    save.assert_not_called()
                    reserve.assert_not_called()
                self.assertFalse(out.exists())
            self.assertEqual(self.invoke_summary(root, out), 1)
            freeze = (out / 'freeze.json').read_bytes()
            with self.assertRaisesRegex(ValueError, 'no retry'):
                self.invoke_summary(root, out)
            self.assertEqual((out / 'freeze.json').read_bytes(), freeze)

    def test_failed_preparation_preserves_unexpected_contents_and_original_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out = root / 'run'

            def denied(path, mode):
                (path / 'unexpected').write_text('retained')
                raise PermissionError('original denied leaf')

            with mock.patch.object(parity, 'EVIDENCE_ROOTS', (root,)), \
                    mock.patch.object(Path, 'chmod', denied), \
                    self.assertRaisesRegex(PermissionError, 'original denied leaf'):
                parity.prepare_evidence_output(out)
            self.assertEqual((out / 'unexpected').read_text(), 'retained')

    def run_main(self, root, out, phase, denied=False):
        lock = root / 'lock'
        lock.touch()
        argv = ['parity', '--phase', phase, '--packet', str(root / 'packet'),
                '--lock', str(lock), '--output', str(out), '--deployment', str(root / 'deployment'),
                '--model', str(root / 'model'), '--settings', str(root / 'settings'),
                '--gguf', str(root / 'gguf'), '--server', str(root / 'server'),
                '--native-receipt', str(root / 'native-receipt'),
                '--offline-baseline-receipt',str(root/'offline-receipt')]
        with contextlib.ExitStack() as stack:
            def patch(owner, name, **kwargs):
                return stack.enter_context(mock.patch.object(owner, name, **kwargs))
            patch(parity, 'EVIDENCE_ROOTS', new=(root,))
            patch(parity.sys, 'argv', new=argv)
            patch(parity.os, 'umask')
            patch(parity, 'tree', return_value={'head': 'public'})
            patch(parity, 'baseline', return_value={})
            patch(parity, 'audit', return_value={'process': {}})
            patch(parity, 'correction_history', return_value={})
            patch(parity, 'read', return_value={'deployment_id': 'public'})
            patch(parity, 'sha', return_value='pin')
            patch(parity, 'validate_receipt')
            patch(parity, 'verify_artifacts')
            patch(parity, 'require_offline_baseline')
            owner = patch(parity, 'prepare_evidence_output', wraps=parity.prepare_evidence_output)
            gpu = patch(parity, 'gpu')
            reserve = patch(parity, 'reserve_gateway')
            save = patch(parity, 'save', wraps=parity.save)
            launch = patch(parity.subprocess, 'Popen', side_effect=ReachedLaunch)
            if denied:
                patch(Path, 'chmod', side_effect=PermissionError('denied leaf'))
                with self.assertRaises(PermissionError):
                    parity.main()
                gpu.assert_not_called()
                reserve.assert_not_called()
                save.assert_not_called()
                launch.assert_not_called()
                self.assertFalse(out.exists())
            else:
                with self.assertRaises(ReachedLaunch):
                    parity.main()
                self.assertEqual(stat.S_IMODE(out.stat().st_mode), 0o700)
                self.assertTrue((out / 'freeze.json').is_file())
                self.assertEqual(reserve.call_count, int(phase == 'gateway'))
                if phase == 'gateway':
                    self.assertEqual(reserve.call_args.args[-1], out.resolve())
                self.assertEqual(launch.call_count, 1)
            owner.assert_called_once_with(out)

    def test_all_phases_use_shared_preparation_before_freeze_budget_and_launch(self):
        for phase in ['native', 'preflight', 'gateway']:
            for denied in [False, True]:
                with self.subTest(phase=phase, denied=denied), tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp)
                    self.run_main(root, root / 'run', phase, denied)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with mock.patch.object(parity, 'prepare_evidence_output', wraps=parity.prepare_evidence_output) as owner:
                self.assertEqual(self.invoke_summary(root, root / 'run'), 1)
                owner.assert_called_once_with(root / 'run')


class ReceiptTests(unittest.TestCase):
    def receipt(self, phase='gateway'):
        artifacts = {'freeze.json': 'hash', 'exit.json': 'hash', 'run.log': 'hash'}
        if phase == 'gateway':
            artifacts.update({k: 'hash' for k in ['results.json', 'admission.json', 'runtime.json']})
            artifacts.update({f'{kind}-{i}.json': 'hash' for kind in ['planned', 'request', 'response', 'provenance'] for i in range(30)})
            artifacts.update({f'case-{i}.json': 'hash' for i in range(30)})
        else:
            artifacts['native-framing.json'] = 'hash'
            artifacts.update({f'framing-{i}.json': 'hash' for i in range(30)})
        return {'version': 2, 'phase': phase, 'passed': True, 'source': {'head': 'current', 'files': {'owner': 'hash'}},
                'inputs': {'manifest': 'frozen'}, 'runtime': {'deployment': 'frozen'},
                'actual_calls': 30 if phase == 'gateway' else 0, 'artifacts': artifacts,
                'native_evidence': {'receipt_path': '/durable/native/receipt.json', 'sha256': 'hash'}}

    def check(self, receipt, phase='gateway'):
        parity.validate_receipt(receipt, {'head': 'current', 'files': {'owner': 'hash'}},
                                {'manifest': 'frozen'}, {'deployment': 'frozen'}, phase)

    def test_separate_summary_manifest_is_pinned(self):
        self.assertEqual(parity.summary_manifest()['task'], 'native-summary-parity')
        self.assertNotIn('additional_tasks', parity.read(parity.REPO / parity.MANIFEST_PATH))
        with mock.patch.object(parity, 'sha', return_value='changed'):
            with self.assertRaisesRegex(ValueError, 'summary manifest changed'):
                parity.summary_manifest()

    def test_complete_gateway_and_native(self):
        self.check(self.receipt())
        self.check(self.receipt('native'), 'native')

    def test_stale_missing_failed_and_wrong_runtime(self):
        good = self.receipt()
        for field, wrong in [('source', {'head': 'old'}), ('inputs', {}), ('runtime', {'deployment': 'old'}),
                             ('phase', 'preflight'), ('passed', False), ('passed', 1), ('artifacts', {}),
                             ('native_evidence', None)]:
            with self.subTest(field=field, value=wrong):
                bad = copy.deepcopy(good); bad[field] = wrong
                with self.assertRaises(ValueError):
                    self.check(bad)
        for field in good:
            bad = copy.deepcopy(good); del bad[field]
            with self.subTest(missing=field), self.assertRaises(ValueError):
                self.check(bad)

    def test_count_and_partial_artifact_boundaries(self):
        for count in [0, '', False, -1, 1, 29, 31, 120, 570, None]:
            bad = self.receipt(); bad['actual_calls'] = count
            with self.subTest(count=count), self.assertRaises(ValueError):
                self.check(bad)
        for artifact in ['response-29.json', 'provenance-0.json', 'case-29.json', 'planned-15.json']:
            bad = self.receipt(); del bad['artifacts'][artifact]
            with self.subTest(artifact=artifact), self.assertRaises(ValueError):
                self.check(bad)

    def test_changed_correction_history_rejected_before_reservation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'parity-investigation-result.json').write_text('{}')
            with self.assertRaises(ValueError):
                parity.reserve_gateway(root, {'head': 'current'}, root / 'unused')
            self.assertFalse((root / 'correction-gateway-reservation.json').exists())

    def test_tampered_raw_output_and_artifact_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); raw = root / 'response.json'; raw.write_text('original')
            receipt = {'artifacts': {'response.json': parity.sha(raw)}}
            parity.verify_artifacts(root / 'receipt.json', receipt)
            raw.write_text('tampered')
            with self.assertRaises(ValueError):
                parity.verify_artifacts(root / 'receipt.json', receipt)
            for path in ['../outside', '/absolute']:
                with self.subTest(path=path), self.assertRaises(ValueError):
                    parity.verify_artifacts(root / 'receipt.json', {'artifacts': {path: 'hash'}})

    def test_uncertain_generation_cannot_be_retried_in_a_new_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with mock.patch.object(parity, 'correction_history', return_value={'prior_calls': 210, 'planned_calls': 30}):
                parity.reserve_gateway(root, {'head': 'current'}, root / 'first')
                with self.assertRaises(FileExistsError):
                    parity.reserve_gateway(root, {'head': 'current'}, root / 'second')


    def test_repair_approval_binds_source_contract_history_and_budget(self):
        source = {'head': 'current', 'files': {'owner': 'hash'}}
        packet = Path('/durable/packet')
        good = {'operator_message': 'i accept production repair', 'proposal_sha256': parity.REPAIR_PROPOSAL_SHA,
                'maximum_calls': 2, 'accepted_contract_sha256': 'hash', 'source': source,
                'prior_reservation_sha256': 'hash', 'preflight_hashes': {'oracle': 'frozen'}}
        with mock.patch.object(parity, 'sha', return_value='hash'), mock.patch.object(parity, 'repair_preflight_hashes', return_value={'oracle': 'frozen'}):
            parity.validate_repair_approval(good, source, packet)
            for key, wrong in [('operator_message', ''), ('proposal_sha256', 'changed'),
                               ('maximum_calls', 0), ('maximum_calls', False), ('maximum_calls', ''),
                               ('maximum_calls', 2.0), ('maximum_calls', 3),
                               ('accepted_contract_sha256', 'changed'), ('source', {'head':'old'}),
                               ('prior_reservation_sha256', 'changed'), ('preflight_hashes', {})]:
                bad=copy.deepcopy(good);bad[key]=wrong
                with self.subTest(key=key,wrong=wrong), self.assertRaises(ValueError):
                    parity.validate_repair_approval(bad,source,packet)
            for key in good:
                bad=copy.deepcopy(good);del bad[key]
                with self.subTest(missing=key),self.assertRaises(ValueError):
                    parity.validate_repair_approval(bad,source,packet)

    def test_repair_cannot_reset_budget_by_renaming_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            approval=root/'approval.json';approval.write_text('{}')
            value={'accepted_contract_sha256':'hash','maximum_calls':2,'source':{'head':'current'}}
            with mock.patch.object(parity,'validate_repair_approval',return_value=value):
                parity.reserve_summary_repair(root/'packet',approval,{'head':'current'},root/'first')
                with self.assertRaises(FileExistsError):
                    parity.reserve_summary_repair(root/'packet',approval,{'head':'current'},root/'second')


if __name__ == '__main__':
    unittest.main()
