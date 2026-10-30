#!/usr/bin/env python3
"""No-ML guard tests: successful child exits still respect posthoc bounds."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
import phase6_vitmatte_spike as spike


class FinishedWorker:
    def __init__(self, output, metadata):
        self.returncode = 0
        self.pid = 1
        for name in ["alpha.png", "raw-alpha.png"]:
            (output / name).write_bytes(b"dummy fixture output")
        (output / "case.json").write_text(json.dumps(metadata))

    def poll(self):
        return self.returncode

    def wait(self, timeout=None):
        return self.returncode


class GuardTests(unittest.TestCase):
    def run_worker(self, *, rss=1000, elapsed=1, width=20, height=10):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        output = root / "output"
        metadata = {"worker_peak_rss_bytes": rss, "width": width, "height": height}
        with patch.object(spike.subprocess, "Popen", side_effect=lambda *args, **kwargs: FinishedWorker(output, metadata)), \
             patch.object(spike.time, "monotonic", side_effect=[0, elapsed]):
            report = spike.guarded_case(root / "model", root / "input", output, (20, 10))
        return report, output

    def test_short_rss_spike_is_rejected_even_when_worker_finishes_before_poll(self):
        report, output = self.run_worker(rss=spike.RSS_LIMIT + 1)
        self.assertFalse(report["completed"])
        self.assertIn("RSS", report["refusal"])
        self.assertFalse((output / "alpha.png").exists())
        self.assertFalse((output / "raw-alpha.png").exists())

    def test_wall_timeout_after_success_and_wrong_dimensions_remove_results(self):
        for options in [{"elapsed": spike.CASE_SECONDS + 0.1}, {"width": 21}]:
            report, output = self.run_worker(**options)
            self.assertFalse(report["completed"])
            self.assertFalse((output / "alpha.png").exists())

    def test_successful_case_retains_hashes_and_exact_limits(self):
        report, output = self.run_worker(rss=spike.RSS_LIMIT, elapsed=spike.CASE_SECONDS)
        self.assertTrue(report["completed"])
        self.assertEqual(report["outputs"]["alpha.png"], spike.digest(output / "alpha.png"))

    def test_repository_output_refuses_before_loading_any_model(self):
        repository = Path(spike.__file__).resolve().parents[1]
        for worker in [[], ["--worker"]]:
            arguments = [spike.__file__, "--model", "/tmp/absent-model", "--inputs", "/tmp/absent-inputs",
                         "--outputs", str(repository / "must-not-exist-output"), *worker]
            with patch.object(spike.sys, "argv", arguments), self.assertRaisesRegex(ValueError, "outside the repository"):
                spike.main()
        self.assertFalse((repository / "must-not-exist-output").exists())

    def test_native_attention_preflight_distinguishes_small_and_large_inputs(self):
        self.assertLess(spike.working_bytes(320, 256), spike.RSS_LIMIT)
        self.assertLess(spike.working_bytes(451, 300), spike.RSS_LIMIT)
        self.assertGreater(spike.working_bytes(1280, 1280), spike.RSS_LIMIT)


if __name__ == "__main__":
    unittest.main()
