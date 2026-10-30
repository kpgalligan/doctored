#!/usr/bin/env python3
"""Scoring runner unit checks: the pins, the widened audit and the verdict.

Fabricated files only. Nothing here runs the Rust scorer, reads a reference
photograph, decodes a real workspace or makes a network request.
"""
import json
from pathlib import Path
import tempfile
import unittest

import phase9i_assisted_scoring as runner
from test_phase9i_assisted_workspace import workspace_with

ANON = "0123456789ab"
CASE = "photo-0051115Q_000001_0003-light-blurred"
OTHER = "full-fur-longhair-checker-binary"


def replay(anonymous=ANON, detail=None, exposed=False):
    """The shape `replay_manifest` and the audit read. Its `alpha_write` key is
    the reason the audit may only look at string VALUES."""
    return {
        "case_id": anonymous, "origin": "human", "scoreable": True, "outcome": "submitted",
        "annotator_reports_prior_exposure": exposed,
        "label_plane": {"encoding": "trimap_u8_0_128_255", "sha256": "a" * 64},
        "write_planes": {"alpha_write": "constant_65535", "rgb_write": "constant_65535",
                         "protection": "constant_0", "alpha_write_sha256": "b" * 64,
                         "rgb_write_sha256": "b" * 64, "protection_sha256": "c" * 64},
        "events": [{"index": 0, "kind": "view", "detail": detail,
                    "label_plane_sha256": "d" * 64}],
    }


def scored(cases, passes=True):
    """A `rescore.score_run` result with one summary row per corpus."""
    summaries = {
        corpus: {"cases": len(rows), "pass_rule_cases": len(rows),
                 "v1_passes": len(rows) if passes else 0,
                 "v2_passes": len(rows) if passes else 0, "v2_indeterminate": 0,
                 "refusals": 0, "ambiguous_diagnostic": 0, "glass_research": 0}
        for corpus, rows in cases.items()
    }
    return {"summaries": summaries,
            "cases": [row for rows in cases.values() for row in rows]}


def mapping_of(case_ids, subset_override=False, exposed=()):
    return {"mapping_schema_version": 1, "workspace_id": "w",
            "subset_override": subset_override,
            "cases": [{"case": case, "anonymous_id": f"anon{index:08d}",
                       "corpus": case.split("-")[0],
                       "reference_id": runner.exporter.parse_case(case)[1],
                       "background": runner.exporter.parse_case(case)[2],
                       "start": runner.exporter.parse_case(case)[3],
                       "annotator_exposed": case in set(exposed)}
                      for index, case in enumerate(sorted(case_ids))]}


class PinChecks(unittest.TestCase):
    def test_every_pinned_source_exists(self):
        self.assertIn("core/src/assisted_replay.rs", runner.PINNED_SOURCES)
        self.assertIn("core/tests/common/sha256.rs", runner.PINNED_SOURCES)
        for name in runner.PINNED_SOURCES:
            self.assertTrue((runner.ROOT / name).is_file(), name)
        self.assertTrue(runner.DECLARATION.is_file())
        self.assertTrue(runner.PROTOCOL_DOC.is_file())


class AuditChecks(unittest.TestCase):
    def audited(self, root, document=None, name=f"{ANON}.json", forbidden=()):
        workspace = workspace_with(root)
        if document is not None:
            (workspace / "replays" / name).write_text(json.dumps(document))
        runner.audit_scored_workspace(workspace, runner.exporter.BASE_FORBIDDEN + tuple(forbidden))
        return workspace

    def test_a_replay_for_a_manifest_case_is_permitted(self):
        with tempfile.TemporaryDirectory() as root:
            self.audited(root, replay(), forbidden=(CASE, "0051115Q_000001_0003"))

    def test_the_replay_schema_keys_do_not_trip_the_alpha_token(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = self.audited(root, replay())
            document = json.loads((workspace / "replays" / f"{ANON}.json").read_text())
            self.assertIn("alpha_write", document["write_planes"])
            self.assertIn("alpha", runner.exporter.BASE_FORBIDDEN)

    def test_audit_refuses_a_stray_file_in_the_replays_directory(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            (workspace / "replays" / "notes.txt").write_text("left over")
            with self.assertRaisesRegex(ValueError, "unexpected workspace files"):
                runner.audit_scored_workspace(workspace, runner.exporter.BASE_FORBIDDEN)

    def test_audit_refuses_a_replay_for_an_unknown_anonymous_id(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            (workspace / "replays" / "cafecafecafe.json").write_text(json.dumps(replay()))
            with self.assertRaisesRegex(ValueError, "unexpected workspace files"):
                runner.audit_scored_workspace(workspace, runner.exporter.BASE_FORBIDDEN)

    def test_audit_refuses_a_case_id_inside_a_replay_detail(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            (workspace / "replays" / f"{ANON}.json").write_text(
                json.dumps(replay(detail=f"looked at {CASE}")))
            # Named on its own, so the message is the case id rather than the
            # `photo-` prefix BASE_FORBIDDEN would have caught first.
            with self.assertRaisesRegex(ValueError, f"names {CASE.lower()}"):
                runner.audit_scored_workspace(workspace, (CASE,))
            with self.assertRaisesRegex(ValueError, "names photo-"):
                runner.audit_scored_workspace(
                    workspace, runner.exporter.BASE_FORBIDDEN + (CASE,))

    def test_a_malformed_replay_is_audited_and_recorded_rather_than_raised(self):
        # `main` audits BEFORE it builds the manifest, so a replay that will not
        # parse has to survive the audit for the scorer to refuse its case.
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            (workspace / "replays" / f"{ANON}.json").write_text("{")
            runner.audit_scored_workspace(workspace, runner.exporter.BASE_FORBIDDEN)
            manifest = json.loads((workspace / "manifest.json").read_text())
            entry = runner.replay_manifest(workspace, manifest)[ANON]
            self.assertEqual(sorted(entry), ["malformed", "sha256"])
            self.assertTrue(entry["malformed"])
            self.assertRegex(entry["sha256"], r"^[0-9a-f]{64}$")
            self.assertEqual(runner._refusal_reason(entry), "malformed replay")

    def test_a_malformed_replay_is_still_scanned_for_forbidden_tokens(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            (workspace / "replays" / f"{ANON}.json").write_text('{"detail": "photo-')
            with self.assertRaisesRegex(ValueError, "names photo-"):
                runner.audit_scored_workspace(workspace, runner.exporter.BASE_FORBIDDEN)


class ReplayManifestChecks(unittest.TestCase):
    def test_a_missing_replay_is_reported_as_none(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            manifest = json.loads((workspace / "manifest.json").read_text())
            self.assertEqual(runner.replay_manifest(workspace, manifest), {ANON: None})
            (workspace / "replays" / f"{ANON}.json").write_text(json.dumps(replay()))
            entry = runner.replay_manifest(workspace, manifest)[ANON]
            self.assertEqual(sorted(entry),
                             ["annotator_reports_prior_exposure", "label_plane_sha256",
                              "origin", "outcome", "scoreable", "sha256"])
            self.assertEqual(entry["outcome"], "submitted")
            self.assertEqual(entry["label_plane_sha256"], "a" * 64)
            self.assertRegex(entry["sha256"], r"^[0-9a-f]{64}$")


class VerdictChecks(unittest.TestCase):
    def test_a_dry_run_claims_nothing_however_well_it_scores(self):
        mapping = mapping_of([CASE, OTHER], subset_override=True)
        anonymous = {case["case"]: case["anonymous_id"] for case in mapping["cases"]}
        rows = {"crops": [{"case": CASE, "refused": False}],
                "full": [{"case": OTHER, "refused": True}]}
        replays = {anonymous[CASE]: replay(), anonymous[OTHER]: None}
        decision = runner.verdict(scored(rows), mapping, replays, "v2", True)
        self.assertTrue(decision["dry_run"])
        self.assertFalse(decision["assisted_result_claimed"])
        self.assertFalse(decision["numerical_gate_passed"])
        self.assertFalse(decision["completion_decidable"])
        self.assertEqual(decision["scored_cases"], 2)
        self.assertEqual(decision["refused_cases"], [{"case": OTHER, "reason": "no replay"}])
        self.assertIn("dry run", decision["reason"])

    def test_a_subset_override_cannot_decide_completion(self):
        mapping = mapping_of([CASE, OTHER], subset_override=True)
        rows = {"crops": [{"case": CASE, "refused": False}],
                "full": [{"case": OTHER, "refused": False}]}
        decision = runner.verdict(scored(rows), mapping, {}, "v2", False)
        self.assertTrue(decision["subset_override"])
        self.assertFalse(decision["completion_decidable"])
        self.assertFalse(decision["assisted_result_claimed"])
        self.assertEqual(len(decision["missing_subset_cases"]), 34)

    def test_a_complete_passing_run_claims_the_result_under_v2(self):
        subset = sorted(runner.rescore.ASSISTED_COMPLETION_SUBSET)
        mapping = mapping_of(subset, exposed=(subset[0],))
        rows = {"crops": [{"case": case, "refused": False} for case in subset if
                          case.startswith("photo-")],
                "full": [{"case": case, "refused": False} for case in subset if
                         case.startswith("full-")]}
        decision = runner.verdict(scored(rows), mapping, {}, "v2", False)
        self.assertEqual(decision["missing_subset_cases"], [])
        self.assertTrue(decision["completion_decidable"])
        self.assertTrue(decision["numerical_gate_passed"])
        self.assertTrue(decision["assisted_result_claimed"])
        self.assertEqual(decision["scored_cases"], 36)
        self.assertEqual(decision["exposed_cases"], [subset[0]])
        self.assertIsNone(decision["reason"])

    def test_a_self_reported_exposure_is_listed_even_when_the_mapping_is_silent(self):
        subset = sorted(runner.rescore.ASSISTED_COMPLETION_SUBSET)
        mapping = mapping_of(subset, exposed=(subset[0],))
        anonymous = {case["case"]: case["anonymous_id"] for case in mapping["cases"]}
        rows = {"crops": [{"case": case, "refused": False} for case in subset if
                          case.startswith("photo-")],
                "full": [{"case": case, "refused": False} for case in subset if
                         case.startswith("full-")]}
        replays = {anonymous[subset[1]]: replay(exposed=True),
                   anonymous[subset[2]]: replay(exposed=False)}
        decision = runner.verdict(scored(rows), mapping, replays, "v2", False)
        self.assertEqual(decision["exposed_cases"], sorted(subset[:2]))

    def test_the_scorers_own_refusal_reason_wins_over_the_derived_one(self):
        mapping = mapping_of([CASE, OTHER], subset_override=True)
        anonymous = {case["case"]: case["anonymous_id"] for case in mapping["cases"]}
        rows = {"crops": [{"case": CASE, "refused": True}],
                "full": [{"case": OTHER, "refused": True}]}
        replays = {anonymous[CASE]: {"sha256": "e" * 64, "malformed": True},
                   anonymous[OTHER]: None}
        # The scorer saw the planes; the workspace alone only knows the replay.
        refusals = [{"case": CASE, "reason": "replay refused: unknown gesture kind"}]
        decision = runner.verdict(scored(rows), mapping, replays, "v2", False, refusals)
        self.assertEqual(decision["refused_cases"], [
            {"case": CASE, "reason": "replay refused: unknown gesture kind"},
            {"case": OTHER, "reason": "no replay"},
        ])
        # With no scorer list the derived reason answers the malformed replay.
        derived = runner.verdict(scored(rows), mapping, replays, "v2", False)
        self.assertEqual(derived["refused_cases"][0],
                         {"case": CASE, "reason": "malformed replay"})

    def test_a_complete_failing_run_claims_nothing(self):
        subset = sorted(runner.rescore.ASSISTED_COMPLETION_SUBSET)
        mapping = mapping_of(subset)
        rows = {"crops": [{"case": case, "refused": False} for case in subset if
                          case.startswith("photo-")],
                "full": [{"case": case, "refused": False} for case in subset if
                         case.startswith("full-")]}
        decision = runner.verdict(scored(rows, passes=False), mapping, {}, "v2", False)
        self.assertTrue(decision["completion_decidable"])
        self.assertFalse(decision["numerical_gate_passed"])
        self.assertFalse(decision["assisted_result_claimed"])
        self.assertIn("gate v2", decision["reason"])


if __name__ == "__main__":
    unittest.main()
