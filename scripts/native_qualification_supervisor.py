#!/usr/bin/env python3
"""Signal-safe lifecycle owner for bounded native qualification, never inference policy."""
import argparse
import datetime
import importlib.util
import math
import os
from pathlib import Path
import re
import select
import signal
import stat
import subprocess
import sys
import time


spec = importlib.util.spec_from_file_location(
    'qualification_evidence', Path(__file__).with_name('c9-preproduction-parity.py'))
evidence = importlib.util.module_from_spec(spec)
spec.loader.exec_module(evidence)
HANDLED_SIGNALS = (signal.SIGTERM, signal.SIGINT, signal.SIGHUP)


def descendants(pid):
    found = set()
    pending = [pid]
    while pending:
        current = pending.pop()
        for proc in Path('/proc').glob('[0-9]*'):
            try:
                fields = (proc / 'stat').read_text().rsplit(')', 1)[1].split()
                child = int(proc.name)
                if int(fields[1]) == current and child not in found:
                    found.add(child)
                    pending.append(child)
            except (OSError, ValueError, IndexError):
                pass
    return found


def own_server(pid, expected_digest):
    """Retain the validated pidfd, argument and executable-pin ownership checks."""
    descriptor = os.pidfd_open(pid)
    try:
        proc = Path('/proc') / str(pid)
        arguments = (proc / 'cmdline').read_bytes().split(b'\0')
        if b'--no-warmup' not in arguments or b'--model' not in arguments:
            os.close(descriptor)
            return None
        identity = (proc / 'stat').read_text().rsplit(')', 1)[1].split()[19]
        actual = evidence.sha(proc / 'exe')
        evidence.require(actual == expected_digest, 'owned running server pin changed')
        return descriptor, {
            'pid': pid, 'start_identity': identity, 'server_sha256': actual,
            'arguments': [arg.decode(errors='replace') for arg in arguments if arg],
            'mapped_libraries': (proc / 'maps').read_text(),
        }
    except BaseException:
        os.close(descriptor)
        raise


def stop_owned(descriptor):
    for sig in (signal.SIGTERM, signal.SIGKILL):
        if select.select([descriptor], [], [], 0)[0]:
            return True
        try:
            signal.pidfd_send_signal(descriptor, sig)
        except ProcessLookupError:
            return True
        if select.select([descriptor], [], [], 2)[0]:
            return True
    return False


def deadline_value(value):
    evidence.require(not isinstance(value, bool) and isinstance(value, (int, float))
                     and math.isfinite(value) and 0 < value <= 1800,
                     'deadline must be a positive number at most 1800 seconds')
    return value


def supervise(command, cwd, environment, output, server_digest, deadline_seconds):
    deadline_value(deadline_seconds)
    evidence.require(hasattr(os, 'pidfd_open') and hasattr(signal, 'pidfd_send_signal'),
                     'owned-process signalling requires Linux pidfds')
    owned = {}
    process = None
    timed_out = False
    error = None
    cleanup_errors = []
    state = {'signal': None, 'late_signal': None, 'cleaning': False}
    previous = {}
    start = time.monotonic()

    def interrupted(sig, frame):
        # Repeated handled signals cannot interrupt cleanup or receipt writes.
        if state['cleaning']:
            if state['late_signal'] is None:
                state['late_signal'] = {'number': sig, 'name': signal.Signals(sig).name}
            return
        if state['signal'] is None:
            state['signal'] = {'number': sig, 'name': signal.Signals(sig).name}
        # Do not raise asynchronously between acquiring a child/pidfd and recording
        # its ownership. The wait loop consumes this origin signal at its next check.

    log_path = output / 'run.log'
    with log_path.open('x') as log:
        log_path.chmod(0o600)
        try:
            try:
                for sig in HANDLED_SIGNALS:
                    previous[sig] = signal.signal(sig, interrupted)
                process = subprocess.Popen(command, cwd=cwd, env=environment,
                                           stdout=log, stderr=subprocess.STDOUT)
                while process.poll() is None and state['signal'] is None:
                    for pid in descendants(process.pid):
                        if pid in owned:
                            continue
                        try:
                            item = own_server(pid, server_digest)
                            if item is not None:
                                owned[pid] = item
                        except (ProcessLookupError, FileNotFoundError):
                            continue
                    remaining = deadline_seconds - (time.monotonic() - start)
                    if remaining <= 0:
                        timed_out = True
                        break
                    time.sleep(min(0.2, max(0.001, remaining)))
            except BaseException as exc:
                error = type(exc).__name__
            finally:
                state['cleaning'] = True
                if process is not None and process.poll() is None:
                    try:
                        process.terminate()
                        try:
                            process.wait(timeout=2)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
                    except BaseException as exc:
                        cleanup_errors.append(type(exc).__name__)
                for descriptor, item in owned.values():
                    try:
                        item['exit_observed'] = bool(select.select([descriptor], [], [], 0)[0])
                        item['cleanup_needed'] = not item['exit_observed']
                        if item['cleanup_needed']:
                            item['exit_observed'] = stop_owned(descriptor)
                        item['reaped'] = not (Path('/proc') / str(item['pid'])).exists()
                    except BaseException as exc:
                        cleanup_errors.append(type(exc).__name__)
                        item['reaped'] = False
                    finally:
                        os.close(descriptor)
                evidence.save(output / 'owned-runtime.json', {
                    'owned': [item for _, item in owned.values()],
                    'all_owned_reaped': bool(owned) and all(item['reaped'] for _, item in owned.values()),
                    'direct_child': None if process is None else {
                        'pid': process.pid, 'exit_code': process.poll(),
                        'reaped': process.returncode is not None,
                    },
                    'cleanup_errors': cleanup_errors,
                })
                status = ('SIGNALLED' if state['signal'] else
                          'WALL_TIMEOUT_NOT_SEMANTIC_FAILURE' if timed_out else
                          'DRIVER_ERROR' if error or cleanup_errors else 'PROCESS_EXIT')
                report = {
                    'exit_code': None if process is None else process.returncode,
                    'timed_out': timed_out, 'wall_seconds': time.monotonic() - start,
                    'error': error, 'signal': state['signal'],
                    'late_signal': state['late_signal'],
                    'cleanup_errors': cleanup_errors, 'status': status,
                }
                evidence.save(output / 'exit.json', report)
        finally:
            for sig, handler in previous.items():
                signal.signal(sig, handler)
    return report


def service_stop_receipt(output):
    """An independent systemd hook records abrupt loss; it never fabricates completion."""
    evidence.save(output / 'service-exit.json', {
        'recorded_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'service_result': os.environ.get('SERVICE_RESULT'),
        'exit_code': os.environ.get('EXIT_CODE'),
        'exit_status': os.environ.get('EXIT_STATUS'),
        'supervisor_exit_receipt_present': (output / 'exit.json').is_file(),
        'supervisor_owned_receipt_present': (output / 'owned-runtime.json').is_file(),
        'qualification_success': False,  # Only the independent fidelity gate can sign off.
    })


def systemd_argument(value):
    value = str(value)
    evidence.require(not any(c in value for c in '\x00\n\r'), 'invalid systemd argument')
    return '"' + value.replace('%', '%%').replace('\\', '\\\\').replace('"', '\\"') + '"'


def launch_detached(command, cwd, output, unit, deadline_seconds, offline_receipt=None):
    deadline_value(deadline_seconds)
    evidence.require(offline_receipt is not None, 'mandatory offline baseline receipt required before native launch')
    evidence.require(Path(cwd).resolve() in (evidence.REPO.resolve(), (evidence.REPO/'src-tauri').resolve()),
                     'native launch must use the tested checkout')
    evidence.require_offline_baseline(offline_receipt)
    evidence.require(re.fullmatch(r'docsum-qualification-[a-z0-9-]{1,100}', unit) is not None,
                     'qualification unit name required')
    evidence.require(not output.is_symlink(), 'output cannot be a symlink')
    out = output.resolve()
    metadata = out.stat()
    evidence.require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.getuid()
                     and stat.S_IMODE(metadata.st_mode) == 0o700
                     and '.codex/worktrees' not in str(out)
                     and any(out.is_relative_to(root.resolve()) for root in evidence.EVIDENCE_ROOTS),
                     'existing private durable output required')
    # Executed after main-process death by the service manager, outside its process lifetime.
    stop = ' '.join(systemd_argument(arg) for arg in [
        Path(sys.executable).resolve(), Path(__file__).resolve(),
        'service-stop', '--output', out])
    args = ['systemd-run', '--user', '--unit=' + unit,
            '--working-directory=' + str(Path(cwd).resolve()),
            '--property=RuntimeMaxSec=' + str(deadline_seconds),
            '--property=TimeoutStopSec=10', '--property=KillMode=control-group',
            '--property=Restart=no', '--property=UMask=0077',
            '--property=ExecStopPost=' + stop, '--', *command]
    evidence.save(out / 'detached-launch.json', {
        'unit': unit, 'command': args, 'deadline_seconds': deadline_seconds,
        'owner_sha256': evidence.sha(Path(__file__).resolve()),
        'offline_baseline_receipt_sha256': evidence.sha(offline_receipt),
    })  # Exclusive create: a failed launch cannot silently become a second attempt.
    result = subprocess.run(args, capture_output=True, text=True)
    evidence.require(result.returncode == 0, 'detached service launch failed: ' + result.stderr)
    return result.stdout + result.stderr


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='mode', required=True)
    stop = commands.add_parser('service-stop')
    stop.add_argument('--output', type=Path, required=True)
    launch = commands.add_parser('launch')
    launch.add_argument('--output', type=Path, required=True)
    launch.add_argument('--cwd', type=Path, required=True)
    launch.add_argument('--unit', required=True)
    launch.add_argument('--deadline', type=float, required=True)
    launch.add_argument('--offline-baseline-receipt', type=Path, required=True)
    launch.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.mode == 'service-stop':
        service_stop_receipt(args.output)
    else:
        command = args.command[1:] if args.command[:1] == ['--'] else args.command
        evidence.require(bool(command), 'driver command required')
        print(launch_detached(command, args.cwd, args.output, args.unit, args.deadline,
                              args.offline_baseline_receipt), end='')


if __name__ == '__main__':
    main()
