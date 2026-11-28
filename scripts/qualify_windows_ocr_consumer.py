"""Run the real DocSum handoff under the pinned OCR checkout's fixture-account launcher."""

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


CONSUMER_TEST = "connect::ocr_consumer::tests::windows_real_foreground_provider_is_consumed_and_recovered"


def test_foreground_consumer(tmp_path):
    from document_ocr.windows_binding import BoundState
    from document_ocr.windows_paths import Route
    from document_ocr.windows_registration import Publisher
    from document_ocr.windows_tasks import current_sid

    assert current_sid() == os.environ["DOCUMENT_OCR_FIXTURE_SID"]
    route = Route.current()
    assert (
        not route.application.exists() and not (route.profile / "LocalConnect").exists()
    )

    # The shared launcher sets the working directory to the pinned OCR checkout.
    provider_tests = Path.cwd() / "tests"
    sys.path.insert(0, str(provider_tests))
    from serving_baseline import load_qualified_serving_baseline
    from test_windows_cli import command

    scan = provider_tests / "fixtures/cord-v2/validation-000.pdf"
    cases = load_qualified_serving_baseline()
    expected = cases[hashlib.sha256(scan.read_bytes()).hexdigest()]
    baseline = tmp_path / "qualified.json"
    baseline.write_text(json.dumps({"cases": cases}), encoding="utf-8")
    consumer = (Path.cwd() / "consumer-test.exe").resolve(strict=True)
    argv, env = command("serve")
    print("PROVIDER_COMMAND", subprocess.list2cmdline(argv), flush=True)
    log = tmp_path / "provider.log"
    with log.open("wb") as stream:
        child = subprocess.Popen(
            argv,
            env=env,
            creationflags=subprocess.CREATE_NEW_CONSOLE,
            stdout=stream,
            stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                assert child.poll() is None, log.read_text(errors="replace")
                binding = BoundState(route).read() if route.control.exists() else None
                if binding is not None:
                    owner = Publisher(
                        binding.instance_id, route.publication_environment()
                    )
                    paths = [owner.path(version) for version in (2, 3)]
                    if all(path.exists() for path in paths):
                        assert all(
                            json.loads(path.read_bytes())["pid"] == child.pid
                            for path in paths
                        )
                        break
                time.sleep(0.1)
            else:
                raise AssertionError(
                    "foreground registrations did not appear: "
                    + log.read_text(errors="replace")
                )

            invocation = [
                str(consumer),
                "--ignored",
                "--exact",
                CONSUMER_TEST,
                "--nocapture",
            ]
            print("CONSUMER_COMMAND", subprocess.list2cmdline(invocation), flush=True)
            subprocess.run(
                invocation,
                check=True,
                timeout=360,
                env=dict(
                    os.environ,
                    DOCSUM_NATIVE_OCR_SCAN=str(scan),
                    DOCSUM_NATIVE_OCR_BASELINE=str(baseline),
                    DOCSUM_NATIVE_OCR_OUTPUT=str(tmp_path),
                ),
            )
            for suffix in ("pdf", "txt"):
                output = (tmp_path / f"ocr.{suffix}").read_bytes()
                field = "pdf" if suffix == "pdf" else "text"
                digest = hashlib.sha256(output).hexdigest()
                assert (len(output), digest) == (
                    expected[field + "_bytes"],
                    expected[field + "_sha256"],
                )
                print("CONSUMER_OUTPUT", f"ocr.{suffix}", len(output), digest)

            subprocess.run(
                [
                    sys._base_executable,
                    str(provider_tests / "fixtures/windows_console_stop.py"),
                    str(child.pid),
                    str(int(signal.CTRL_C_EVENT)),
                ],
                creationflags=subprocess.DETACHED_PROCESS,
                check=True,
                timeout=10,
            )
            assert child.wait(timeout=10) == 0
            assert all(not path.exists() for path in paths)
        finally:
            if child.poll() is None:
                child.terminate()
            child.wait(timeout=10)
