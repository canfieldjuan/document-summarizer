import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('parity', Path(__file__).with_name('c9-preproduction-parity.py'))
parity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parity)


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


if __name__ == '__main__':
    unittest.main()
