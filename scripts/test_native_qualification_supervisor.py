"""Public, zero-model process-lifecycle regressions for qualification supervision."""
import importlib.util
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock


OWNER = Path(os.environ.get(
    'NATIVE_SUPERVISOR_TEST_OWNER',
    Path(__file__).with_name('native_qualification_supervisor.py')))
SUPERVISOR = '''
import importlib.util, json, os, sys
from pathlib import Path
spec = importlib.util.spec_from_file_location('supervisor_owner', sys.argv[1])
owner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(owner)
out = Path(sys.argv[2])
child = [sys.executable, '-c', sys.argv[3], str(out)]
result = owner.supervise(child, out, dict(os.environ), out,
                         'NO_MODEL_OR_SERVER', float(sys.argv[4]))
sys.exit(0 if result['exit_code'] == 0 and result.get('signal') is None else 1)
'''
CHILD = '''
import json, os, sys, time
from pathlib import Path
out = Path(sys.argv[1])
(out/'child.json').write_text(json.dumps({'pid':os.getpid()}))
while not (out/'release').exists(): time.sleep(.02)
'''


def load_owner():
    spec = importlib.util.spec_from_file_location('supervisor_tests', OWNER)
    owner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(owner)
    return owner


def wait_file(path, timeout=5):
    deadline = time.monotonic() + timeout
    while not path.exists():
        if time.monotonic() >= deadline:
            raise AssertionError(f'child startup did not create {path.name}')
        time.sleep(.02)


class SignalReceiptsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.out = Path(self.temp.name)
        self.process = None
        self.child_fd = None
        self.parent_fd = None

    def tearDown(self):
        # Only pidfd-pinned processes created by this test may be stopped.
        for descriptor in [self.parent_fd, self.child_fd]:
            if descriptor is not None:
                if not select.select([descriptor], [], [], 0)[0]:
                    signal.pidfd_send_signal(descriptor, signal.SIGKILL)
                    select.select([descriptor], [], [], 5)
                os.close(descriptor)
        if self.process is not None:
            self.process.wait(timeout=5)
        self.temp.cleanup()

    def start(self, deadline=10, child=CHILD, owner=OWNER):
        self.process = subprocess.Popen(
            [sys.executable, '-c', SUPERVISOR, str(owner), str(self.out),
             child, str(deadline)], stdout=subprocess.DEVNULL)
        self.parent_fd = os.pidfd_open(self.process.pid)
        wait_file(self.out / 'child.json')
        self.child_fd = os.pidfd_open(json.loads(
            (self.out / 'child.json').read_text())['pid'])

    def assert_receipts(self):
        self.assertTrue((self.out / 'exit.json').exists(), 'exit.json missing')
        self.assertTrue((self.out / 'owned-runtime.json').exists(),
                        'owned-runtime.json missing')
        self.assertTrue(select.select([self.child_fd], [], [], 0)[0],
                        'direct child remains alive after supervisor exits')
        return json.loads((self.out / 'exit.json').read_text())

    def signal_case(self, sig):
        self.start()
        signal.pidfd_send_signal(self.parent_fd, sig)
        self.process.wait(timeout=5)
        report = self.assert_receipts()
        self.assertEqual(report['signal'], {'number': sig, 'name': signal.Signals(sig).name})
        self.assertEqual(report['status'], 'SIGNALLED')
        self.assertFalse(report['timed_out'])
        self.assertNotEqual(self.process.returncode, 0)

    def test_sigterm_writes_final_receipts(self):
        self.signal_case(signal.SIGTERM)

    def test_sighup_writes_final_receipts(self):
        self.signal_case(signal.SIGHUP)

    def test_sigint_writes_final_receipts(self):
        self.signal_case(signal.SIGINT)

    def test_signalled_child_exit_zero_is_not_success(self):
        child = CHILD.replace("(out/'child.json').write_text", """
import signal
signal.signal(signal.SIGTERM, lambda sig, frame: sys.exit(0))
(out/'child.json').write_text""")
        self.start(child=child)
        signal.pidfd_send_signal(self.parent_fd, signal.SIGTERM)
        self.assertNotEqual(self.process.wait(timeout=5), 0)
        report = self.assert_receipts()
        self.assertEqual(report['exit_code'], 0)
        self.assertEqual(report['status'], 'SIGNALLED')
        self.assertEqual(report['signal']['name'], 'SIGTERM')

    def test_ordinary_completion(self):
        self.start()
        (self.out / 'release').touch()
        self.assertEqual(self.process.wait(timeout=5), 0)
        report = self.assert_receipts()
        self.assertEqual(report['status'], 'PROCESS_EXIT')
        self.assertEqual(report['exit_code'], 0)
        self.assertFalse(report['timed_out'])
        self.assertIsNone(report['signal'])

    def test_child_failure_is_not_success(self):
        self.start(child=CHILD + '\nraise SystemExit(7)\n')
        (self.out / 'release').touch()
        self.assertNotEqual(self.process.wait(timeout=5), 0)
        report = self.assert_receipts()
        self.assertEqual(report['exit_code'], 7)
        self.assertEqual(report['status'], 'PROCESS_EXIT')

    def test_wall_timeout_writes_cleanup(self):
        self.start(deadline=.2)
        self.assertNotEqual(self.process.wait(timeout=5), 0)
        report = self.assert_receipts()
        self.assertTrue(report['timed_out'])
        self.assertEqual(report['status'], 'WALL_TIMEOUT_NOT_SEMANTIC_FAILURE')

    def test_repeated_signals_do_not_interrupt_cleanup(self):
        child = CHILD.replace("(out/'child.json').write_text", """
import signal
def stopping(sig, frame):
    (out/'child-stopping').touch()
signal.signal(signal.SIGTERM, stopping)
(out/'child.json').write_text""")
        self.start(child=child)
        signal.pidfd_send_signal(self.parent_fd, signal.SIGTERM)
        wait_file(self.out / 'child-stopping')
        for sig in [signal.SIGHUP, signal.SIGINT, signal.SIGTERM]:
            signal.pidfd_send_signal(self.parent_fd, sig)
            time.sleep(.03)
        self.assertNotEqual(self.process.wait(timeout=5), 0)
        report = self.assert_receipts()
        self.assertEqual(report['signal']['name'], 'SIGTERM')
        self.assertEqual(report['status'], 'SIGNALLED')
        self.assertEqual(report['cleanup_errors'], [])
        self.assertEqual(report['late_signal'], {'number': signal.SIGHUP, 'name': 'SIGHUP'})


class SupervisorBoundariesTests(unittest.TestCase):
    def setUp(self):
        self.owner = load_owner()

    def test_deadline_boundaries_and_falsy_values(self):
        for bad in [0, -1, False, True, '', None, float('nan'), float('inf'), 1801]:
            with self.subTest(value=bad), self.assertRaises(ValueError):
                self.owner.deadline_value(bad)
        for good in [.001, 1799, 1800]:
            with self.subTest(value=good):
                self.assertEqual(self.owner.deadline_value(good), good)

    def test_launch_error_still_writes_receipts_and_restores_handlers(self):
        with tempfile.TemporaryDirectory() as tmp:
            before = {sig: signal.getsignal(sig) for sig in self.owner.HANDLED_SIGNALS}
            out = Path(tmp)
            report = self.owner.supervise([str(out/'absent-public-fixture')], out,
                                          dict(os.environ), out, 'no-server', 1)
            self.assertEqual(report['status'], 'DRIVER_ERROR')
            self.assertEqual(report['error'], 'FileNotFoundError')
            self.assertIsNone(report['exit_code'])
            self.assertTrue((out/'owned-runtime.json').exists())
            self.assertTrue((out/'exit.json').exists())
            self.assertEqual(before, {sig: signal.getsignal(sig) for sig in before})

    def test_late_signal_is_recorded_without_changing_completion(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            original_save = self.owner.evidence.save

            def signal_during_cleanup(path, value):
                if path.name == 'owned-runtime.json':
                    signal.raise_signal(signal.SIGHUP)
                return original_save(path, value)

            with mock.patch.object(self.owner.evidence, 'save', side_effect=signal_during_cleanup):
                report = self.owner.supervise([sys.executable, '-c', 'pass'], out,
                                              dict(os.environ), out, 'no-server', 1)
            self.assertEqual(report['late_signal'], {'number': signal.SIGHUP, 'name': 'SIGHUP'})
            self.assertEqual(json.loads((out/'exit.json').read_text())['late_signal'],
                             report['late_signal'])
            self.assertEqual(report['status'], 'PROCESS_EXIT')
            self.assertEqual(report['exit_code'], 0)
            self.assertIsNone(report['signal'])
            self.assertFalse(report['timed_out'])
            self.assertEqual(report['cleanup_errors'], [])

    def test_stop_receipt_distinguishes_uncatchable_loss(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            with mock.patch.dict(os.environ, {'SERVICE_RESULT': 'signal',
                                              'EXIT_CODE': 'killed', 'EXIT_STATUS': 'KILL'}):
                self.owner.service_stop_receipt(out)
            report = json.loads((out/'service-exit.json').read_text())
            self.assertEqual(report['exit_status'], 'KILL')
            self.assertFalse(report['supervisor_exit_receipt_present'])
            self.assertFalse(report['supervisor_owned_receipt_present'])
            self.assertFalse(report['qualification_success'])

    def test_launch_projection_is_bounded_and_exclusive(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            with mock.patch.object(self.owner.evidence, 'EVIDENCE_ROOTS', (out.parent,)), \
                    mock.patch.object(self.owner.evidence, 'require_offline_baseline'), \
                    mock.patch.object(self.owner.subprocess, 'run', return_value=
                                      subprocess.CompletedProcess([], 0, '', '')) as launch:
                proof=out/'offline.json';proof.write_text('{}')
                self.owner.launch_detached(['public-driver'], self.owner.evidence.REPO, out,
                                           'docsum-qualification-fixture', 1800, proof)
                args = launch.call_args.args[0]
                for expected in ['--property=RuntimeMaxSec=1800', '--property=Restart=no',
                                 '--property=KillMode=control-group', '--property=TimeoutStopSec=10']:
                    self.assertIn(expected, args)
                self.assertTrue(any(arg.startswith('--property=ExecStopPost=') for arg in args))
                with self.assertRaises(FileExistsError):
                    self.owner.launch_detached(['public-driver'], self.owner.evidence.REPO, out,
                                               'docsum-qualification-fixture', 1800, proof)
                self.assertEqual(launch.call_count, 1)

    def test_rejected_launch_does_not_run_or_reserve(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            with mock.patch.object(self.owner.evidence, 'EVIDENCE_ROOTS', (out.parent,)), \
                    mock.patch.object(self.owner.evidence, 'require_offline_baseline'), \
                    mock.patch.object(self.owner.subprocess, 'run') as launch:
                proof=out/'offline.json';proof.write_text('{}')
                for unit, deadline in [('foreign-service', 1800),
                                       ('docsum-qualification-good', 0),
                                       ('docsum-qualification-good', 1801)]:
                    with self.subTest(unit=unit, deadline=deadline), self.assertRaises(ValueError):
                        self.owner.launch_detached(['public-driver'], self.owner.evidence.REPO, out, unit, deadline, proof)
                out.chmod(0o755)
                with self.assertRaises(ValueError):
                    self.owner.launch_detached(['public-driver'], self.owner.evidence.REPO, out,
                                               'docsum-qualification-good', 1800, proof)
                self.assertFalse((out/'detached-launch.json').exists())
                launch.assert_not_called()

    def test_missing_or_failed_offline_baseline_never_launches(self):
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp)
            with mock.patch.object(self.owner.subprocess,'run') as launch:
                with self.assertRaisesRegex(ValueError,'mandatory offline baseline'):
                    self.owner.launch_detached(['public-driver'],self.owner.evidence.REPO,out,'docsum-qualification-fixture',1800)
                with mock.patch.object(self.owner.evidence,'require_offline_baseline',side_effect=ValueError('stale source')):
                    with self.assertRaisesRegex(ValueError,'stale source'):
                        self.owner.launch_detached(['public-driver'],self.owner.evidence.REPO,out,'docsum-qualification-fixture',1800,out/'receipt')
                launch.assert_not_called()
                self.assertFalse((out/'detached-launch.json').exists())


if __name__ == '__main__':
    unittest.main()
