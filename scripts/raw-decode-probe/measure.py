#!/usr/bin/env python3
"""Measure the phys_footprint of RAW decode stages and fit the decode charge.

Builds `main.swift` with the app sources (-O, whole module, release core)
through `scripts/probe_build.py`, which caches the linked probe under
`build/probe-cache`; writes synthetic DNG fixtures into --output, then runs
every case in its own process (MallocLargeCache=0, three runs plus a baseline
twin, a 32 GiB working limit) and prints the table, the residency gate, the
STOP checks and the fit. `--real LABEL=PATH` (repeatable) adds a real camera
RAW's O, W, K and V cases. The Presence variants OP (open) and VP
(preview, 16-bit) of the NEF and every `--real` fixture are check-only: not
fitted, and a STOP unless the committed decode charge is ≥ ⌈1.25 × max E⌉
(run them with `--only`). The v1-lensfun variants OL (open, 16/8) and VL
(import preview, 16-bit) develop with the Lensfun stage on, on the same
fixtures, under the same check, and STOP as well when the stage did not run
(the decoder carries its own lens profile, or no Lensfun profile matched).
Run `cd core && cargo build --release` first. Nothing it writes belongs in the
repository: point --output outside it.

`--phase2` runs only phase 2's restore, local and publish cases and
fits `RawLocalWork`'s restore and local charges; `--menu <limit>` runs the
`menu` cases (Layer ▸ Redevelop RAW… stage by stage) at that working limit.

`--fit-from JSON…` refits from saved `measure.json` files and starts no
process; `--recheck-phase2 JSON` reprints a saved phase-2 run's committed
coverage and ceilings under the committed constants; `--admit <limit>` opens
and prepares the NEF and every `--real` fixture once at that working limit.

`--float` runs the 32-bit float (linear) development's cases at depth
32 on the NEF, D24 and every `--real` fixture and gates each on the app's own
float charge: O32, OG32, W32, K32, V32 and VG32 (`RawSourceWork.scratch(float:
true)`, V with the session's 24 B/px of inspection copies; OG and VG develop
grain), GR32 (`applyingRawGrain` alone, the core's selected-edit plan),
VF32F and VF32D (a viewport-only HDR frame, the preview session's 48 MiB plus
`extendedFramePerPixel` per output pixel of each f32 side) and L0, L1, L4 and
FX with grain (`RawLocalWork.measuredScratch(float: true)` plus those 24
B/px). It fits nothing: a miss is a STOP.

`--predict <limit> --file <fixture>` instead mirrors `RasterResources.admit`
for one RAW step with the committed `RawSourceWork.scratch` constants (and
`RawLocalWork`'s with `--local` or `--restore`) and prints the step's charge
and a verdict (README.md).
"""

import argparse
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import plistlib
import re
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
MIB = 1024 * 1024
HARNESS_LIMIT = 34_359_738_368  # 32 GiB: no admission refuses inside the harness
EIGHT_GIB = 8_589_934_592
RESIDENCY_MARGIN = 128 * MIB
A_MIN_PX = 2_000_000
PER_ENCODED_BYTE = 3
# The charge this refit replaces, for the comparison columns.
PREVIOUS = {"scratchFixed": 9 * MIB, "scratchPerPixel": 173, "scratchPerEncodedByte": 3}

sys.path.insert(0, str(ROOT / "scripts"))
from phase3_fixtures import synthetic_dng  # noqa: E402

_isolation_spec = importlib.util.spec_from_file_location(
    "verification_isolation", ROOT / "scripts/verification_isolation.py")
_isolation = importlib.util.module_from_spec(_isolation_spec)
_isolation_spec.loader.exec_module(_isolation)
# The shared cached probe build, loaded by path.
_build_spec = importlib.util.spec_from_file_location("probe_build", ROOT / "scripts/probe_build.py")
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)

SIZES = {"D0": (512, 384), "D3": (2000, 1500), "D12": (4000, 3000), "D24": (6000, 4000),
         "D26": (6240, 4160), "D45": (8256, 5504)}
SMALL = ["D0", "D3", "D12", "D24", "D26"]


def cases(camera, real):
    """(label, mode, fixture, depth, fitted)."""
    nef = ["NEF"] if camera else []
    rows = []
    for depth in (16, 8):
        for name in SMALL + ["D45"] + nef:
            rows.append((f"O{depth}-{name}", "open", name, depth, True))
    for depth in (16, 8):
        for name in SMALL + nef:
            rows.append((f"W{depth}-{name}", "warm", name, depth, True))
    for depth in (16, 8):
        for name in SMALL + nef:
            rows.append((f"K{depth}-{name}", "cold", name, depth, True))
    rows.append(("K16-D45", "cold", "D45", 16, True))
    for name in ["D3", "D12", "D24"] + nef:
        rows.append((f"V16-{name}", "preview", name, 16, True))
    for depth in (16, 8):
        rows.append((f"P{depth}-D24", "prep", "D24", depth, False))
    for name in ["D24"] + nef:
        rows.append((f"RES16-{name}", "resident", name, 16, False))
    rows.append(("RES8-D24", "resident", "D24", 8, False))
    for name in real:
        for depth in (16, 8):
            rows.append((f"O{depth}-{name}", "open", name, depth, True))
        for depth in (16, 8):
            rows.append((f"W{depth}-{name}", "warm", name, depth, True))
        for depth in (16, 8):
            rows.append((f"K{depth}-{name}", "cold", name, depth, True))
        rows.append((f"V16-{name}", "preview", name, 16, True))
    if "D850" in real:
        rows.append(("RES16-D850", "resident", "D850", 16, False))
    # Check-only Presence variants (`PRESENCE_VARIANTS`), never fitted.
    for name in nef + list(real):
        for depth in (16, 8):
            rows.append((f"OP{depth}-{name}", "open", name, depth, False))
        rows.append((f"VP16-{name}", "preview", name, 16, False))
    # v1-lensfun: check-only Lensfun variants (`LENSFUN_VARIANTS`), never fitted.
    for name in nef + list(real):
        for depth in (16, 8):
            rows.append((f"OL{depth}-{name}", "open", name, depth, False))
        rows.append((f"VL16-{name}", "preview", name, 16, False))
    return rows


# The label prefixes whose case passes a shape to its mode (`main.swift`): the open and the
# 16-bit import preview with Develop Presence. Check-only: each STOPs unless the committed decode
# charge is ≥ ⌈1.25 × max E⌉.
PRESENCE_VARIANTS = ("OP", "VP")
# v1-lensfun: the open and the 16-bit import preview with the Lensfun stage on (Profile
# Corrections on a file whose decoder carries no lens profile). Check-only, as Presence's.
LENSFUN_VARIANTS = ("OL", "VL")


def variant(label):
    """The shape argument of a Presence or Lensfun variant's label (OP, VP, OL, VL), or None."""
    prefix = label[:2]
    known = PRESENCE_VARIANTS + LENSFUN_VARIANTS
    return prefix if prefix in known and label[2:4] in ("16", "8-") else None


def real_fixtures(values):
    """`--real LABEL=PATH` pairs, in order."""
    found = {}
    for value in values or []:
        label, sep, path = value.partition("=")
        if not sep or not re.fullmatch(r"[A-Z0-9]+", label) or label in SIZES or label == "NEF":
            raise SystemExit(f"--real {value}: LABEL must match [A-Z0-9]+ and not be a synthetic name or NEF")
        if label in found:
            raise SystemExit(f"--real {label} is given twice")
        if not Path(path).is_file():
            raise SystemExit(f"--real {label}: {path} is not a file")
        found[label] = Path(path)
    return found


REAL_FULL = ("D850", "A7R4")
# The local tone path's shapes, each (tone locals, whether a tone reads a neighbourhood):
# LT1 one pointwise tone radial, LT16 sixteen on a 4×4 grid, LTS one radial with Highlights,
# Shadows, Clarity and Texture (the guide and the Presence plane). They develop no variant.
# LTD is LTS plus Sharpness 60 and Noise Reduction 60 (the detail stage over the
# Presence plane): check-only, never fitted (`fit_tone` reads LT1, LT16 and LTS), so its committed
# charge must cover ⌈1.25 × max E⌉ in `phase2_coverage`.
TONE_SHAPES = {"LT1": (1, False), "LT16": (16, False), "LTS": (1, True), "LTD": (1, True)}
TONE_FIXTURES = ("D24", "Z6")
# The shapes whose one local's mask Vision generates (subject, people). LS on Z6 is the
# case that allocates, so it must produce a mask.
GENERATED_SHAPES = ("LS", "LP")
MUST_GENERATE = ("LS",)


def phase2_cases(camera, real):
    """Phase 2: (label, mode, fixture, depth, fitted, shape, cold, locals, effects).
    Each `--real` label adds RS, L0, L1, L4 and FX at 16 and 8 bits; D850 and A7R4 also
    add the cold and one-local shapes (RSK, L0K, L1K, L1B, L1C). Z6 (toys on a table) also adds
    the generated-mask shapes LS and LP: one local whose mask Vision makes, k 1."""
    nef = ["NEF"] if camera else []
    rows = []
    for name in real:
        for depth in (16, 8):
            if name in TONE_FIXTURES:
                rows += tone_rows(name, depth)
            if name == "Z6":
                rows.append((f"LS-{depth}-{name}", "local", name, depth, True, "LS", False, 1, False))
                rows.append((f"LP-{depth}-{name}", "local", name, depth, True, "LP", False, 1, False))
            rows.append((f"RS{depth}-{name}", "restore", name, depth, True, "none", False, 0, False))
            rows.append((f"L0-{depth}-{name}", "local", name, depth, True, "L0", False, 0, False))
            rows.append((f"L1-{depth}-{name}", "local", name, depth, True, "L1", False, 1, False))
            rows.append((f"L4-{depth}-{name}", "local", name, depth, True, "L4", False, 4, False))
            rows.append((f"FX-{depth}-{name}", "local", name, depth, True, "FX", False, 1, True))
            # FX plus Develop Presence, check-only (`presence_local_check`).
            rows.append((f"PX-{depth}-{name}", "local", name, depth, False, "PX", False, 1, True))
            if name in REAL_FULL:
                rows.append((f"RSK{depth}-{name}", "restore", name, depth, True, "none", True, 0, False))
                rows.append((f"L0K-{depth}-{name}", "local", name, depth, True, "L0", True, 0, False))
                rows.append((f"L1K-{depth}-{name}", "local", name, depth, True, "L1", True, 1, False))
                rows.append((f"L1B-{depth}-{name}", "local", name, depth, True, "L1B", False, 1, False))
                rows.append((f"L1C-{depth}-{name}", "local", name, depth, True, "L1C", False, 1, False))
    return synthetic_phase2_cases(nef) + rows


def tone_rows(name, depth):
    """The tone shapes on one fixture at one depth (k is the camera-local count the Swift charge
    takes for their creates: min(4, creates))."""
    return [(f"{shape}-{depth}-{name}", "local", name, depth, True, shape, False, min(4, count), False)
            for shape, (count, _) in TONE_SHAPES.items()]


def synthetic_phase2_cases(nef):
    rows = []
    for depth in (16, 8):
        if "D24" in TONE_FIXTURES:
            rows += tone_rows("D24", depth)
    for depth in (16, 8):
        for name in SMALL + nef:
            rows.append((f"RS{depth}-{name}", "restore", name, depth, True, "none", False, 0, False))
        for name in ["D3", "D24", "D26"]:
            rows.append((f"RS4-{depth}-{name}", "restore", name, depth, True, "L4", False, 4, False))
        for name in ["D0", "D24"] + nef:
            rows.append((f"RSK{depth}-{name}", "restore", name, depth, True, "none", True, 0, False))
    for depth in (16, 8):
        for name in SMALL + nef:
            rows.append((f"L0-{depth}-{name}", "local", name, depth, True, "L0", False, 0, False))
            rows.append((f"L1-{depth}-{name}", "local", name, depth, True, "L1", False, 1, False))
        for name in ["D0", "D24"] + nef:
            rows.append((f"L0K-{depth}-{name}", "local", name, depth, True, "L0", True, 0, False))
            rows.append((f"L1K-{depth}-{name}", "local", name, depth, True, "L1", True, 1, False))
        for name in ["D24", "D26"]:
            rows.append((f"L1B-{depth}-{name}", "local", name, depth, True, "L1B", False, 1, False))
            rows.append((f"L1C-{depth}-{name}", "local", name, depth, True, "L1C", False, 1, False))
        for name in ["D3", "D24", "D26"] + nef:
            rows.append((f"L4-{depth}-{name}", "local", name, depth, True, "L4", False, 4, False))
            rows.append((f"FX-{depth}-{name}", "local", name, depth, True, "FX", False, 1, True))
        for name in ["D24", "D26"]:
            rows.append((f"PB0-{depth}-{name}", "publish", name, depth, False, "L0", False, 0, False))
            rows.append((f"PB1-{depth}-{name}", "publish", name, depth, False, "L1", False, 1, False))
    return rows


def menu_cases(camera, real):
    """(label, fixture, depth, sequence) for `--menu <limit>`: D24, D26 and every `--real` label."""
    rows = []
    for depth in (16, 8):
        for name in ["D24", "D26"]:
            for sequence in ("M0", "M1", "M11", "MC"):
                rows.append((f"{sequence}-{depth}-{name}", name, depth, sequence))
    for name in real:
        for depth in (16, 8):
            for sequence in ("M0", "M1", "M11", "MC"):
                rows.append((f"{sequence}-{depth}-{name}", name, depth, sequence))
    return rows


def environment(output, limit=HARNESS_LIMIT):
    return dict(os.environ, MallocLargeCache="0", DR_BACKGROUND="1", DR_AUTOMATION_ALERTS="record",
                ANTHROPIC_API_KEY="dummy", CLANG_MODULE_CACHE_PATH=str(output / "clang-cache"),
                DR_WORKING_LIMIT_BYTES=str(limit),
                DR_ACTIONS_DIR=str(output / "isolation/actions"),
                DR_RECOVERY_DIR=str(output / "isolation/recovery"),
                **_isolation.preset_environment(output / "isolation"))


def build(output, env):
    library = ROOT / "core/target/release"
    if not (library / "libdoctored_core.a").is_file():
        raise SystemExit("Missing the release core; run `cd core && cargo build --release` first.")
    with (ROOT / "app/Info.plist").open("rb") as source:
        info = plistlib.load(source)
    info["CFBundleIdentifier"] = "com.kgalligan.doctored.raw-decode-probe"
    info_copy = output / "Info.plist"
    with info_copy.open("wb") as destination:
        plistlib.dump(info, destination)
    sources = probe_build.app_sources()
    probes = probe_build.probe_sources("raw-decode-probe")
    binary = output / "raw-decode-probe"
    (output / "build.log").write_text("")
    probe_build.build(app_sources=sources, sources=probes,
                      bridging_header=ROOT / "app/Bridging/Doctored-Bridging-Header.h",
                      library=library / "libdoctored_core.a", info_plist=info_copy, optimization="-O",
                      output=binary, log=output / "build.log", env=env, timeout=1800)
    return binary


def fixtures(output, camera, real):
    folder = output / "fx"
    folder.mkdir(parents=True, exist_ok=True)
    files = {}
    for name, (w, h) in SIZES.items():
        path = folder / f"{name}.dng"
        if not path.exists():
            synthetic_dng(path, w, h)
        files[name] = path
    if camera:
        files["NEF"] = Path(camera)
    files.update(real)
    receipt = {}
    for name, path in files.items():
        data = path.read_bytes()
        receipt[name] = {"path": str(path), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
    return files, receipt


def run(binary, arguments, env, log):
    """One process under /usr/bin/time -l; the harness's JSON plus time's peak."""
    completed = subprocess.run(["/usr/bin/time", "-l", str(binary), *map(str, arguments),
                                "-ApplePersistenceIgnoreState", "YES", "-ColorWorkingSpace", "srgb"],
                               cwd=ROOT, env=env, capture_output=True, text=True, timeout=1800)
    with log.open("a") as stream:
        stream.write(f"$ {' '.join(map(str, arguments))}\n{completed.stdout}{completed.stderr}\n")
    reply = None
    for line in completed.stdout.splitlines():
        if line.startswith("{"):
            reply = json.loads(line)
    if reply is None:
        reply = {"error": f"no JSON (exit {completed.returncode})"}
    peak = re.search(r"(\d+)\s+peak memory footprint", completed.stderr)
    if peak:
        reply["time_peak"] = int(peak.group(1))
    if "[AutomationAlerts]" in completed.stderr:
        reply["alerts"] = [line for line in completed.stderr.splitlines() if "[AutomationAlerts]" in line]
    return reply


# --- the charge -------------------------------------------------------------

def committed_constants():
    """`RawSourceWork`'s scratch constants, read from the Swift source."""
    text = (ROOT / "app/Sources/RawSourceWork.swift").read_text()
    values = {}
    for name in ("scratchFixed", "scratchPerPixel", "scratchPerEncodedByte"):
        found = re.search(rf"static let {name}: UInt64 = ([0-9_ *]+)\n", text)
        if not found:
            return None
        product = 1
        for factor in found.group(1).split("*"):
            product *= int(factor.strip().replace("_", ""))
        values[name] = product
    return values


LOCAL_NAMES = ("restoreFixed", "restorePerPixel", "localFixed", "localPerPixel", "localPerLocalPixel",
               "localPerEffectsPixel", "localPerToneLocalPixel", "localToneStagePixel")


def local_constants():
    """`RawLocalWork`'s restore and local constants, read from the Swift source."""
    text = (ROOT / "app/Sources/RawLocalWork.swift").read_text()
    values = {}
    for name in LOCAL_NAMES:
        found = re.search(rf"static let {name}: UInt64 = ([0-9_ *]+)\n", text)
        if not found:
            return None
        product = 1
        for factor in found.group(1).split("*"):
            product *= int(factor.strip().replace("_", ""))
        values[name] = product
    return values


def restore_charge(local, px, enc):
    """`RawLocalWork.restoreScratch`."""
    return local["restoreFixed"] + local["restorePerPixel"] * px + PER_ENCODED_BYTE * enc


def measured_charge(decode, local, px, enc, k, e, kt=0, ts=False):
    """`RawLocalWork.measuredScratch`: max(decode charge, the local formula). k camera locals,
    kt every local (the tone term), ts a tone that reads a neighbourhood."""
    rate = (local["localPerPixel"] + k * local["localPerLocalPixel"] + kt * local["localPerToneLocalPixel"]
            + (local["localToneStagePixel"] if ts else 0) + (local["localPerEffectsPixel"] if e else 0))
    return max(charge(decode, px, enc), local["localFixed"] + rate * px + PER_ENCODED_BYTE * enc)


def old_local_charge(decode, px, enc):
    """`RawLocalWork.scratch` (decoder migration's envelope, unchanged)."""
    return charge(decode, px, enc) + 56 * px + 4 * MIB


def charge(constants, px, enc, local=False):
    s = constants["scratchFixed"] + constants["scratchPerPixel"] * px + constants["scratchPerEncodedByte"] * enc
    return s + 56 * px + 4 * MIB if local else s


def previous_charge(px, enc, local=False):
    """The charge this refit replaces (`PREVIOUS`); the local form is its `RawLocalWork.scratch`."""
    s = charge(PREVIOUS, px, enc)
    return s + 56 * px + 4 * MIB if local else s


def need_row(r):
    return math.ceil(1.25 * r["max_e"])


def a_floor(rows):
    """The largest-px fixture's whole cost per pixel: ⌈max (need − 3·enc) / p_ref⌉ at px == p_ref."""
    p_ref = max(r["px"] for r in rows)
    return max(math.ceil((need_row(r) - PER_ENCODED_BYTE * r["enc"]) / p_ref) for r in rows if r["px"] == p_ref)


def fit(rows):
    """need = ⌈1.25 × max E⌉, c = 3. For each integer a in [a_floor, max(a_floor, a_hi)], F(a) is the
    smallest MiB multiple covering every need − a·px − 3·enc; the a minimising F(a) + a·p_ref wins
    (the larger a on a tie). a_hi is the slope by the rule of the charge this refit replaces, the
    worst per-pixel whole cost at ≥ 2 MP."""
    p_ref = max(r["px"] for r in rows)
    floor_a = a_floor(rows)
    a_hi = max((math.ceil((need_row(r) - PER_ENCODED_BYTE * r["enc"]) / r["px"]) for r in rows if r["px"] >= A_MIN_PX),
               default=floor_a)

    def rest(a):
        return {r["label"]: need_row(r) - a * r["px"] - PER_ENCODED_BYTE * r["enc"] for r in rows}

    def fixed(a):
        return math.ceil(max(0, max(rest(a).values())) / MIB) * MIB

    best = None
    for a in range(floor_a, max(floor_a, a_hi) + 1):
        objective = fixed(a) + a * p_ref
        if best is None or objective <= best[0]:
            best = (objective, a)
    objective, a = best
    f = fixed(a)
    residuals = rest(a)
    top = max(residuals.values())
    binding_f = sorted(label for label, v in residuals.items() if v == top)
    constants = {"scratchFixed": f, "scratchPerPixel": a, "scratchPerEncodedByte": PER_ENCODED_BYTE}
    per_case = {}
    for r in rows:
        c = charge(constants, r["px"], r["enc"])
        need = need_row(r)
        per_case[r["label"]] = {"px": r["px"], "enc": r["enc"], "max_e": r["max_e"], "need": need,
                                "charge": c, "margin": c - need, "ratio": round(c / r["max_e"], 3),
                                "previous_charge": previous_charge(r["px"], r["enc"])}
    return {"F": f, "F_MiB": f // MIB, "a": a, "a_floor": floor_a, "a_hi": a_hi, "c": PER_ENCODED_BYTE,
            "p_ref": p_ref, "binding_F": binding_f, "objective": objective, "cases": per_case}


def ceiling_px(constants, limit, enc_per_px, retained_per_px=0.0):
    """Largest px whose charge (plus the document's own retained bytes) stays below `limit`."""
    rate = constants["scratchPerPixel"] + constants["scratchPerEncodedByte"] * enc_per_px + retained_per_px
    return int((limit - constants["scratchFixed"]) / rate)


def open_ceiling_px(constants, limit, enc_per_px):
    """Largest px whose open is admitted with the captured snapshot (encoded + 4096 B) counted:
    `RawSourceSnapshot` keeps it admitted across the open's second admit and `develop`'s."""
    return ceiling_px(constants, limit - 4096, enc_per_px, retained_per_px=enc_per_px)


def fixture_bytes_per_px(receipt, rows):
    """Encoded bytes per px of the NEF and every real fixture, px from its O16 case."""
    found = {}
    for name, entry in receipt.items():
        if name in SIZES:
            continue
        case = rows.get(f"O16-{name}")
        px = case["px"] if case and case.get("px") else (6_016_000 if name == "NEF" else None)
        if px:
            found[name] = (entry["bytes"] / px, px)
    return found


def print_fit(result):
    print(f"\nFIT F = {result['F']:,} ({result['F_MiB']} MiB), a = {result['a']} B/px "
          f"(a_floor {result['a_floor']}, a_hi {result['a_hi']}, p_ref {result['p_ref']:,}), "
          f"c = {result['c']} B/encoded byte; F set by {result['binding_F']}; "
          f"objective F + a·p_ref = {result['objective']:,}")
    print("CASE        need            charge          margin         ratio  previous charge")
    for label, row in result["cases"].items():
        print(f"{label:10s} {row['need']:>15,} {row['charge']:>15,} {row['margin']:>14,}  "
              f"{row['ratio']:>6.3f}  {row['previous_charge']:>15,}")


def committed_coverage(committed, fitted, results):
    """Print the committed charge ÷ max E; return the labels charged below need."""
    coverage = {c["label"]: charge(committed, c["px"], c["enc"]) / c["max_e"] for c in fitted}
    below = [c["label"] for c in fitted if charge(committed, c["px"], c["enc"]) < need_row(c)]
    results["committed_coverage"] = {"constants": committed, "ratios": coverage}
    low = {k: round(v, 3) for k, v in coverage.items() if v < 1.25}
    print(f"COMMITTED charge ÷ max E: min {min(coverage.values()):.3f}"
          + (f"; below 1.25: {json.dumps(low)}" if low else "; every fitted case ≥ 1.25"))
    return coverage, below


def presence_check(results):
    """The committed decode charge ÷ max E of every Presence variant case (OP, VP); a STOP
    below ⌈1.25 × max E⌉ or for a variant that did not run."""
    committed = committed_constants()
    checked = {}
    for label, case in results["cases"].items():
        if variant(label) not in PRESENCE_VARIANTS:
            continue
        if case.get("refused") or not case.get("max_e"):
            results["stops"].append(f"Presence variant {label} did not run: {case.get('refused')}")
            continue
        value = charge(committed, case["px"], case["enc"])
        need = need_row(case)
        checked[label] = {"need": need, "charge": value, "margin": value - need,
                          "ratio": round(value / case["max_e"], 3)}
        if value < need:
            results["stops"].append(f"the committed decode charge covers Presence variant {label} below 1.25×: "
                                    f"{value:,} < {need:,}")
    if checked:
        results["presence_check"] = checked
        print("\nPRESENCE CASE         need            committed       margin         ratio")
        for label, row in checked.items():
            print(f"{label:20s} {row['need']:>15,} {row['charge']:>15,} {row['margin']:>14,}  {row['ratio']:6.3f}")


def lensfun_check(results):
    """v1-lensfun: the committed decode charge ÷ max E of every Lensfun variant case (OL, VL); a
    STOP below ⌈1.25 × max E⌉, for a variant that did not run, or for one whose development did not
    run the Lensfun stage (every run reports `lensfun`)."""
    committed = committed_constants()
    checked = {}
    for label, case in results["cases"].items():
        if variant(label) not in LENSFUN_VARIANTS:
            continue
        if case.get("refused") or not case.get("max_e"):
            results["stops"].append(f"Lensfun variant {label} did not run: {case.get('refused')}")
            continue
        if not all(r.get("lensfun") is True for r in case["runs"]):
            results["stops"].append(f"Lensfun variant {label} developed without the Lensfun stage: "
                                    f"{[r.get('lensfun') for r in case['runs']]}")
            continue
        value = charge(committed, case["px"], case["enc"])
        need = need_row(case)
        checked[label] = {"need": need, "charge": value, "margin": value - need,
                          "ratio": round(value / case["max_e"], 3)}
        if value < need:
            results["stops"].append(f"the committed decode charge covers Lensfun variant {label} below 1.25×: "
                                    f"{value:,} < {need:,}")
    if checked:
        results["lensfun_check"] = checked
        print("\nLENSFUN (v1) CASE     need            committed       margin         ratio")
        for label, row in checked.items():
            print(f"{label:20s} {row['need']:>15,} {row['charge']:>15,} {row['margin']:>14,}  {row['ratio']:6.3f}")


# --- --predict ----------------------------------------------------------------

def dimensions(path):
    out = subprocess.run(["sips", "-g", "pixelWidth", "-g", "pixelHeight", str(path)],
                         capture_output=True, text=True, check=True).stdout
    width = int(re.search(r"pixelWidth: (\d+)", out).group(1))
    height = int(re.search(r"pixelHeight: (\d+)", out).group(1))
    return width, height


def predict(arguments):
    """Mirror `RasterResources.admit` (no `own`) for one RAW decode step."""
    constants = committed_constants()
    if constants is None:
        raise SystemExit("RawSourceWork.swift has no scratchFixed/scratchPerPixel/scratchPerEncodedByte.")
    local = local_constants()
    if (arguments.local or arguments.restore) and local is None:
        raise SystemExit("RawLocalWork.swift has no restore/local constants.")
    path = Path(arguments.file)
    width, height = (arguments.width, arguments.height) if arguments.width else dimensions(path)
    px = width * height
    enc = path.stat().st_size
    limit = arguments.predict
    if arguments.restore:
        s = restore_charge(local, px, enc)
    elif arguments.local:
        if not 0 <= arguments.locals <= 4:
            raise SystemExit("--locals counts camera locals: at most four.")
        s = measured_charge(constants, local, px, enc, arguments.locals, arguments.effects)
    else:
        s = charge(constants, px, enc)
    reply = {"file": path.name, "width": width, "height": height, "encoded": enc, "local": arguments.local,
             "restore": arguments.restore, "locals": arguments.locals, "effects": arguments.effects,
             "limit": limit, "charge": s}
    if arguments.charge_only:
        previous = previous_charge(px, enc, arguments.local)
        reply.update({"previous_charge": previous, "above_previous": s > previous})
        if arguments.local or arguments.restore:
            reply["old_local_charge"] = old_local_charge(constants, px, enc)
        print(json.dumps(reply))
        return 0
    r, c, t = arguments.retained, arguments.cache, arguments.transient
    if s >= limit:
        verdict = "refuse"
        reply["figure"] = s
    elif s + t < limit - r:
        verdict = "admit"
    elif s + t >= limit - (r - c):
        verdict = "refuse"
    else:
        verdict = "indeterminate"
    reply.update({"retained": r, "cache": c, "transient": t, "verdict": verdict})
    print(json.dumps(reply))
    return 0


# --- the measurement ----------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="Scratch folder outside the repository.")
    parser.add_argument("--camera", type=Path, help="The genuine camera NEF (scripts/raw-probe/camera.py).")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--only", nargs="*", help="Case labels to run (e.g. O16-D0 RES16-D24).")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--predict", type=int, metavar="LIMIT", help="Predict one step's admission at LIMIT bytes.")
    parser.add_argument("--file", help="--predict: the RAW fixture.")
    parser.add_argument("--width", type=int, help="--predict: width (default: sips).")
    parser.add_argument("--height", type=int, help="--predict: height (default: sips).")
    parser.add_argument("--local", action="store_true", help="--predict: RawLocalWork.measuredScratch.")
    parser.add_argument("--locals", type=int, default=0, help="--predict --local: the locals K (0…4).")
    parser.add_argument("--effects", action="store_true",
                        help="--predict --local: grain, vignette or authored lens geometry.")
    parser.add_argument("--restore", action="store_true", help="--predict: RawLocalWork.restoreScratch.")
    parser.add_argument("--phase2", action="store_true", help="Run only phase 2's cases and fits.")
    parser.add_argument("--menu", type=int, metavar="LIMIT", help="Run the menu cases at LIMIT bytes.")
    parser.add_argument("--retained", type=int, default=0, help="--predict: unique_retained_bytes before the call.")
    parser.add_argument("--cache", type=int, default=0, help="--predict: cache_bytes before the call.")
    parser.add_argument("--transient", type=int, default=0, help="--predict: transient_estimate_bytes.")
    parser.add_argument("--charge-only", action="store_true", help="--predict: the new and previous charges only.")
    parser.add_argument("--real", action="append", metavar="LABEL=PATH",
                        help="A real camera RAW to measure (repeatable; LABEL matches [A-Z0-9]+).")
    parser.add_argument("--fit-from", nargs="+", type=Path, metavar="JSON",
                        help="Refit from saved measure.json files; starts no process.")
    parser.add_argument("--recheck-phase2", type=Path, metavar="JSON",
                        help="Recheck a saved measure-phase2.json under the committed constants.")
    parser.add_argument("--float", action="store_true", dest="float_cases",
                        help="Run the 32-bit float (linear) development's cases.")
    parser.add_argument("--admit", type=int, metavar="LIMIT",
                        help="Open and prepare the NEF and every --real fixture once at LIMIT bytes.")
    arguments = parser.parse_args()
    if arguments.predict is not None:
        if not arguments.file:
            parser.error("--predict needs --file")
        return predict(arguments)
    if arguments.fit_from:
        return fit_from(arguments.fit_from)
    if arguments.recheck_phase2:
        return recheck_phase2(arguments.recheck_phase2)
    real = real_fixtures(arguments.real)
    if not arguments.output:
        parser.error("--output is required")
    if platform.system() != "Darwin":
        parser.error("This probe requires macOS.")
    output = arguments.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = environment(output)
    binary = output / "raw-decode-probe"
    if not arguments.skip_build or not binary.exists():
        binary = build(output, env)
    files, receipt = fixtures(output, arguments.camera, real)
    if arguments.admit is not None:
        return run_admit(arguments, output, binary, files, receipt)
    if arguments.menu is not None:
        return run_menu(arguments, output, binary, files, receipt)
    if arguments.phase2:
        return run_phase2(arguments, output, env, binary, files, receipt)
    if arguments.float_cases:
        return run_float(arguments, output, env, binary, files, receipt)
    log = output / "runs.log"
    results = {"fixtures": receipt, "cases": {}, "stops": [], "harness_limit": HARNESS_LIMIT}
    print("FIXTURES " + json.dumps(receipt), flush=True)

    calibration = run(binary, ["calibrate"], env, log)
    results["calibration"] = calibration
    print("CALIBRATE " + json.dumps(calibration), flush=True)
    if not (calibration.get("drop_ok") and calibration.get("e256_ok")):
        results["stops"].append("calibration missed")

    plan = [c for c in cases(arguments.camera, real) if not arguments.only or c[0] in arguments.only]
    for label, mode, name, depth, fitted in plan:
        target = files[name]
        if mode == "cold":
            saved = output / "rz" / f"{name}-{depth}.rz"
            saved.parent.mkdir(exist_ok=True)
            if not saved.exists():
                reply = run(binary, ["save", files[name], depth, saved], env, log)
                print(f"SAVE {label} {json.dumps(reply)}", flush=True)
            target = saved
        shape = [variant(label)] if variant(label) else []
        runs = [run(binary, [mode, target, depth, *shape], env, log) for _ in range(arguments.runs)]
        twin = None if mode == "resident" else run(binary, ["baseline", mode, target, depth, *shape], env, log)
        for index, reply in enumerate(runs, 1):
            print(f"RUN {label} {index} {json.dumps(reply)}", flush=True)
            if reply.get("alerts"):
                results["stops"].append(f"{label} run {index} showed an [AutomationAlerts] line")
        if twin is not None:
            print(f"BASE {label} {json.dumps(twin)}", flush=True)
            if twin.get("e", 0) > 16 * MIB:
                results["stops"].append(f"baseline twin {label} read E {twin.get('e')}")
        results["cases"][label] = {"mode": mode, "fixture": name, "depth": depth, "fitted": fitted,
                                   "runs": runs, "baseline": twin}

    print("\nCASE        px          enc         E runs (pre)                                   max E        time_peak-pre")
    residency = {}
    for label, case in results["cases"].items():
        first = case["runs"][0]
        if case["mode"] == "resident":
            good = [r for r in case["runs"] if "residual_redevelop" in r]
            if len(good) != len(case["runs"]):
                bad = [r.get("setup_refused") or r.get("error") for r in case["runs"] if r not in good]
                results["stops"].append(f"{label} did not complete: {bad[0]}")
            worst_open = max((r["residual_open"] for r in good), default=None)
            worst_redevelop = max((r["residual_redevelop"] for r in good), default=None)
            residency[label] = {"residual_open": [r["residual_open"] for r in good],
                                "residual_redevelop": [r["residual_redevelop"] for r in good],
                                "residual_open_immediate": [r.get("residual_open_immediate") for r in good],
                                "residual_redevelop_immediate": [r.get("residual_redevelop_immediate")
                                                                 for r in good],
                                "p0": [r["p0"] for r in good],
                                "open_retained": [r["open_retained"] for r in good],
                                "redevelop_retained": [r["redevelop_retained"] for r in good],
                                "max_open": worst_open, "max_redevelop": worst_redevelop}
            print(f"{label:10s} {first.get('px', 0):>11,}  settled residual_open "
                  + ", ".join(f"{r['residual_open']:,}" for r in good) + "  residual_redevelop "
                  + ", ".join(f"{r['residual_redevelop']:,}" for r in good)
                  + "  (immediate open " + ", ".join(f"{r.get('residual_open_immediate', 0):,}" for r in good)
                  + "; redevelop " + ", ".join(f"{r.get('residual_redevelop_immediate', 0):,}" for r in good)
                  + ")")
            continue
        good = [r for r in case["runs"] if "e" in r and "refused" not in r and "setup_refused" not in r]
        refused = [r.get("refused") or r.get("setup_refused") or r.get("error") for r in case["runs"]
                   if r not in good]
        px = first.get("px", 0)
        enc = first.get("encoded", first.get("file_bytes", 0))
        es = ", ".join(f"{r['e']:,} ({r['pre']:,})" for r in good)
        tp = ", ".join(f"{r['time_peak'] - r['pre']:,}" for r in good if "time_peak" in r)
        max_e = max((r["e"] for r in good), default=0)
        print(f"{label:10s} {px:>11,} {enc:>11,}  {es}  max {max_e:,}  [{tp}]  twin {case['baseline'].get('e')}"
              + (f"  REFUSED: {refused[0]}" if refused else ""))
        case.update({"px": px, "enc": enc, "max_e": max_e, "refused": refused[0] if refused else None,
                     "label": label})
        if refused and case["fitted"]:
            results["stops"].append(f"fitted case {label} was refused: {refused[0]}")
        if case["mode"] == "prep":
            for r in good:
                print(f"           prep stage 2: scratch_bytes {r.get('stage2_scratch_bytes')}, "
                      f"unique_retained_bytes {r.get('stage2_unique_retained_bytes')}")
        if case["mode"] == "open":
            for r in good:
                if "retained" in r:
                    case.setdefault("retained", []).append(r["retained"])

    presence_check(results)
    lensfun_check(results)
    results["residency"] = residency
    if residency:
        worst = max(max(v["max_open"] or 0, v["max_redevelop"] or 0) for v in residency.values())
        gate = all(v["max_open"] is not None and v["max_open"] <= RESIDENCY_MARGIN
                   and v["max_redevelop"] is not None and v["max_redevelop"] <= RESIDENCY_MARGIN
                   for v in residency.values())
        results["residency_gate"] = {"pass": gate, "worst": worst, "margin": RESIDENCY_MARGIN}
        print(f"\nRESIDENCY GATE {'pass' if gate else 'MISS'}: worst residual {worst:,} "
              f"(margin {RESIDENCY_MARGIN:,})")
        if not gate:
            results["stops"].append(f"residency gate missed: worst residual {worst:,} > {RESIDENCY_MARGIN:,}")

    fitted = [c for c in results["cases"].values()
              if c["fitted"] and not c.get("refused") and c.get("max_e", 0) > 0]
    if fitted and any(c["px"] >= A_MIN_PX for c in fitted):
        result = fit(fitted)
        results["fit"] = result
        print_fit(result)
        constants = {"scratchFixed": result["F"], "scratchPerPixel": result["a"],
                     "scratchPerEncodedByte": result["c"]}
        retained = {}
        for depth in (16, 8):
            opened = results["cases"].get(f"O{depth}-D24")
            if opened and opened.get("retained"):
                retained[depth] = max(opened["retained"]) / opened["px"]
        ceilings = {}
        formats = [("uncompressed DNG (2 B/px)", 2.0)]
        formats += [(name, per_px) for name, (per_px, _) in fixture_bytes_per_px(receipt, results["cases"]).items()]
        for limit_name, limit in (("8GiB", EIGHT_GIB),):
            for fmt, enc_per_px in formats:
                entry = {"open_px": open_ceiling_px(constants, limit, enc_per_px)}
                for depth, per_px in retained.items():
                    entry[f"redevelop{depth}_px"] = ceiling_px(constants, limit, enc_per_px, per_px)
                ceilings[f"{limit_name} {fmt}"] = entry
        results["ceilings"] = {"retained_per_px": retained, "rows": ceilings}
        print("\nCEILINGS (open: the largest px admitted into an empty process with its snapshot counted; "
              "redevelop adds O-D24's retained/px) " + json.dumps(results["ceilings"]))
        committed = committed_constants()
        if committed and committed != constants:
            print(f"NOTE: RawSourceWork.swift's constants {committed} differ from this fit.")
        if committed:
            coverage, _ = committed_coverage(committed, fitted, results)
            under = [k for k, v in coverage.items() if v < 1.0]
            if under:
                results["stops"].append(f"the committed charge under-admits {under}")
    print("\nSTOPS " + json.dumps(results["stops"]))
    (output / "measure.json").write_text(json.dumps(results, indent=1, default=str))
    return 1 if results["stops"] else 0


# --- phase 2 --------------------------------------------------------------------

ADMIT_PREFIX = "This job needs "
# The core's refusal of a RAW whose file and local masks pass its combined aggregate (1 GiB;
# 512 MiB before).
AGGREGATE_SENTENCE = "retained RAW and source resources exceed the 1 GiB aggregate limit"


def saved_rz(binary, files, output, name, depth, env, log, label):
    saved = output / "rz" / f"{name}-{depth}.rz"
    saved.parent.mkdir(exist_ok=True)
    if not saved.exists():
        reply = run(binary, ["save", files[name], depth, saved], env, log)
        print(f"SAVE {label} {json.dumps(reply)}", flush=True)
    return saved


def need_of(case):
    return math.ceil(1.25 * case["max_e"])


def fit_restore(rows):
    """restorePerPixel from cases ≥ 2 MP, then restoreFixed over every case."""
    big = [r for r in rows if r["px"] >= A_MIN_PX]
    a = max(0, max(math.ceil((need_of(r) - PER_ENCODED_BYTE * r["enc"]) / r["px"]) for r in big))
    floor = max(0, max(need_of(r) - a * r["px"] - PER_ENCODED_BYTE * r["enc"] for r in rows))
    binding_a = max(((math.ceil((need_of(r) - PER_ENCODED_BYTE * r["enc"]) / r["px"]), r["label"]) for r in big))[1]
    binding_f = max(((need_of(r) - a * r["px"] - PER_ENCODED_BYTE * r["enc"]), r["label"]) for r in rows)[1]
    return {"restoreFixed": math.ceil(floor / MIB) * MIB, "restorePerPixel": a,
            "binding_restorePerPixel": binding_a, "binding_restoreFixed": binding_f}


def fit_local(rows):
    """The local fit, rest = need − 3·enc: (a, F) = `fit()` over the k = 0 rows (L0, L0K);
    b per local from the k > 0 rows that are not FX and c from FX (both ≥ 2 MP, over F + a·px);
    localFixed = max(F, the least MiB multiple covering every row)."""
    def rest_of(r):
        return need_of(r) - PER_ENCODED_BYTE * r["enc"]
    rows = [r for r in rows if r["shape"] not in TONE_SHAPES]
    zero = [r for r in rows if r["k"] == 0 and not r["e"]]
    base = fit(zero)
    a, fixed = base["a"], base["F"]
    top = [r for r in zero if r["px"] == base["p_ref"]]
    binding_a = max((math.ceil(rest_of(r) / r["px"]), r["label"]) for r in top)[1]
    print(f"LOCAL k=0 fit: F {fixed:,} ({base['F_MiB']} MiB), a {a} (a_floor {base['a_floor']}, "
          f"a_hi {base['a_hi']}, p_ref {base['p_ref']:,}); F set by {base['binding_F']}")
    locals_ = [r for r in rows if r["k"] > 0 and not r["e"] and r["px"] >= A_MIN_PX]
    per_local = [(math.ceil((rest_of(r) - fixed - a * r["px"]) / (r["k"] * r["px"])), r["label"])
                 for r in locals_]
    b = max(0, max(v for v, _ in per_local)) if per_local else 0
    binding_b = max(per_local)[1] if per_local else None
    effects = [r for r in rows if r["e"] and r["px"] >= A_MIN_PX]
    per_fx = [(math.ceil((rest_of(r) - fixed - (a + r["k"] * b) * r["px"]) / r["px"]), r["label"])
              for r in effects]
    c = max(0, max(v for v, _ in per_fx)) if per_fx else 0
    binding_c = max(per_fx)[1] if per_fx else None
    rest = [(rest_of(r) - (a + r["k"] * b + (c if r["e"] else 0)) * r["px"], r["label"]) for r in rows]
    floor = max(0, max(v for v, _ in rest))
    return {"localFixed": max(fixed, math.ceil(floor / MIB) * MIB), "localPerPixel": a,
            "localPerLocalPixel": b, "localPerEffectsPixel": c, "binding_localPerPixel": binding_a,
            "binding_localPerLocalPixel": binding_b, "binding_localPerEffectsPixel": binding_c,
            "binding_localFixed": max(rest)[1]}


def fit_tone(rows, local):
    """Over the committed localFixed, localPerPixel and localPerLocalPixel (rest = need −
    3·enc): localPerToneLocalPixel t the least integer covering LT1 and LT16 at (a + k·b + kt·t)·px,
    then localToneStagePixel s the least covering LTS at (a + k·b + kt·t + s)·px."""
    def over(r, rate):
        return need_of(r) - PER_ENCODED_BYTE * r["enc"] - local["localFixed"] - rate * r["px"]
    pointwise = [r for r in rows if r["shape"] in ("LT1", "LT16")]
    per_tone = [(math.ceil(over(r, local["localPerPixel"] + r["k"] * local["localPerLocalPixel"])
                           / (r["kt"] * r["px"])), r["label"]) for r in pointwise]
    t = max(0, max(v for v, _ in per_tone)) if per_tone else 0
    staged = [r for r in rows if r["shape"] == "LTS"]
    per_stage = [(math.ceil(over(r, local["localPerPixel"] + r["k"] * local["localPerLocalPixel"]
                                 + r["kt"] * t) / r["px"]), r["label"]) for r in staged]
    s = max(0, max(v for v, _ in per_stage)) if per_stage else 0
    return {"localPerToneLocalPixel": t, "localToneStagePixel": s,
            "binding_localPerToneLocalPixel": max(per_tone)[1] if per_tone else None,
            "binding_localToneStagePixel": max(per_stage)[1] if per_stage else None}


def local_ceilings(decode, local, retained_per_px, enc_per_px, limit, k):
    """Largest px whose preview charge (+ the session's 64 MiB) fits beside the opened document."""
    room = limit - 64 * MIB
    by_decode = (room - decode["scratchFixed"]) / (decode["scratchPerPixel"] + 3 * enc_per_px + retained_per_px)
    rate = local["localPerPixel"] + k * local["localPerLocalPixel"] + 3 * enc_per_px + retained_per_px
    by_local = (room - local["localFixed"]) / rate
    return int(min(by_decode, by_local))


def restore_ceiling(local, retained_per_px, enc_per_px, limit):
    rate = local["restorePerPixel"] + 3 * enc_per_px + retained_per_px
    return int((limit - local["restoreFixed"]) / rate) if rate else None


def fixture_pixels(cases):
    """px of every non-synthetic fixture a saved phase-2 case measured (`fixture`, `px`)."""
    found = {}
    for case in cases.values():
        name = case.get("fixture")
        if name and name not in SIZES and name != "NEF" and case.get("px"):
            found[name] = case["px"]
    return found


def ceilings_table(decode, local, retained, receipt, pixels=None):
    """`pixels` (name → px) adds each real camera file's own bytes per pixel."""
    formats = [("DNG 2 B/px", 2.0),
               ("NEF", receipt["NEF"]["bytes"] / 6_016_000 if "NEF" in receipt else None)]
    for name, px in (pixels or {}).items():
        if name in receipt:
            formats.append((name, receipt[name]["bytes"] / px))
    rows = {}
    for limit_name, limit in (("8GiB", EIGHT_GIB),):
        for fmt, enc_per_px in formats:
            if enc_per_px is None:
                continue
            for depth, per_px in retained.items():
                entry = {f"preview_k{k}_px": local_ceilings(decode, local, per_px, enc_per_px, limit, k)
                         for k in (0, 1, 4)}
                entry["restore_px"] = restore_ceiling(local, per_px, enc_per_px, limit)
                rows[f"{limit_name} {fmt} {depth}-bit"] = entry
    return rows


def menu_retained_per_px(output):
    """Retained bytes per px beside the opened document: M0-D24's reading before its preview."""
    found = {}
    path = output / f"menu-{EIGHT_GIB}.json"
    if path.exists():
        menu = json.loads(path.read_text())
        for depth in (16, 8):
            reply = menu.get("cases", {}).get(f"M0-{depth}-D24")
            stage = next((s for s in (reply or {}).get("stages", []) if s.get("stage") == "preview 1"), None)
            if stage and stage.get("unique_retained_bytes"):
                found[depth] = stage["unique_retained_bytes"] / reply["px"]
    return found


def phase2_coverage(fitted, decode, local, results):
    """The committed restore/local charge ÷ max E per fitted phase-2 case, its STOP, and the L4
    migration-envelope note, under the `decode` and `local` constants."""
    print("\nCASE           need            committed       margin         ratio  old RawLocalWork.scratch")
    coverage = {}
    for c in fitted:
        if c["mode"] == "restore":
            value = restore_charge(local, c["px"], c["enc"])
        else:
            value = measured_charge(decode, local, c["px"], c["enc"], c["k"], c["e"], c.get("kt", 0),
                                    c.get("ts", False))
        need = need_of(c)
        coverage[c["label"]] = {"need": need, "charge": value, "margin": value - need,
                                "ratio": round(value / c["max_e"], 3)}
        old = old_local_charge(decode, c["px"], c["enc"]) if c["mode"] == "local" else None
        print(f"{c['label']:14s} {need:>15,} {value:>15,} {value - need:>14,}  {value / c['max_e']:6.3f}"
              + (f"  {old:>15,}" if old else ""))
        if value < need:
            results["stops"].append(f"the committed charge covers {c['label']} below 1.25×: "
                                    f"{value:,} < {need:,}")
    results["committed_coverage"] = {"constants": local, "cases": coverage}
    l4 = [c for c in fitted if c["shape"] == "L4" and c["mode"] == "local"]
    for c in l4:
        envelope = old_local_charge(decode, c["px"], c["enc"]) + 8 * c["px"]
        if envelope < need_of(c):
            note = (f"migration envelope (scratch + 8 B/px) {envelope:,} < ⌈1.25 × E⌉ {need_of(c):,} "
                    f"at {c['label']}")
            results.setdefault("migration_follow_up", []).append(note)
            print("FOLLOW-UP " + note)


def recheck_phase2(path):
    """`--recheck-phase2 JSON`: a saved phase-2 run's coverage and ceilings under the committed constants."""
    saved = json.loads(Path(path).read_text())
    decode, local = committed_constants(), local_constants()
    if decode is None or local is None:
        raise SystemExit("RawSourceWork.swift or RawLocalWork.swift lacks its constants.")
    print(f"RECHECK {path} with decode {json.dumps(decode)} and local {json.dumps(local)}")
    fitted = [c for c in saved["cases"].values() if c["fitted"] and not c["refused"] and c["max_e"] > 0]
    results = {"stops": []}
    phase2_coverage(fitted, decode, local, results)
    low = min(row["ratio"] for row in results["committed_coverage"]["cases"].values())
    print(f"COMMITTED charge ÷ max E: min {low:.3f} over {len(fitted)} fitted cases")
    retained = {int(k): v for k, v in saved["ceilings"]["retained_per_px"].items()}
    rows = ceilings_table(decode, local, retained, saved["fixtures"], fixture_pixels(saved["cases"]))
    print(f"\nCEILINGS (retained per px from {saved['ceilings'].get('source')}) "
          + json.dumps({"retained_per_px": retained, "rows": rows}))
    print("\nSTOPS " + json.dumps(results["stops"]))
    return 1 if results["stops"] else 0


def fit_from(paths):
    """`--fit-from JSON…`: the fit over the per-case max E of every saved `measure.json`. Starts no process."""
    rows, receipt, cases_by_label = {}, {}, {}
    for path in paths:
        saved = json.loads(Path(path).read_text())
        receipt.update(saved.get("fixtures", {}))
        for label, case in saved["cases"].items():
            if not case.get("fitted") or case.get("refused") or not case.get("max_e"):
                continue
            cases_by_label.setdefault(label, case)
            if label not in rows or case["max_e"] > rows[label]["max_e"]:
                rows[label] = {"label": label, "px": case["px"], "enc": case["enc"], "max_e": case["max_e"]}
    fitted = list(rows.values())
    print(f"FIT-FROM {[str(p) for p in paths]}: {len(fitted)} fitted cases")
    result = fit(fitted)
    print_fit(result)
    constants = {"scratchFixed": result["F"], "scratchPerPixel": result["a"],
                 "scratchPerEncodedByte": result["c"]}
    stops = []
    committed = committed_constants()
    if committed is None:
        stops.append("RawSourceWork.swift has no scratch constants")
    else:
        if committed != constants:
            stops.append(f"the committed constants {committed} differ from the fit {constants}")
        _, below = committed_coverage(committed, fitted, {})
        if below:
            stops.append(f"the committed charge covers {below} below ⌈1.25 × max E⌉")
    shown = committed or constants
    print(f"\nOPEN CEILINGS at 8 GiB (charge + snapshot), constants {json.dumps(shown)}")
    for name, (per_px, px) in fixture_bytes_per_px(receipt, cases_by_label).items():
        enc = receipt[name]["bytes"]
        print(f"{name:6s} {per_px:6.3f} B/px  open ceiling {open_ceiling_px(shown, EIGHT_GIB, per_px):>12,} px  "
              f"(this file: {px:,} px, charge {charge(shown, px, enc):,}, previous {previous_charge(px, enc):,})")
    print("\nSTOPS " + json.dumps(stops))
    return 1 if stops else 0


def float_cases(camera, real):
    """The float cases: (label, mode, fixture, shape, cold) at depth 32. FX is grain, vignette and authored
    lens (grain applies at the float encoding). The float cases add OG32 (the open with grain), VG32
    (the import preview whose after develops grain), GR32 (`applyingRawGrain` alone on an open
    float development, gated on the core's selected-edit plan) and VF32F / VF32D (one
    viewport-only HDR frame of two f32 sides with Clipping, at Fit and at a 1024 x 1024 100 %
    region, gated on the preview session's own charge)."""
    rows = []
    for name in (["NEF"] if camera else []) + ["D24"] + list(real):
        rows.append((f"O32-{name}", "open", name, None, False))
        rows.append((f"OG32-{name}", "open", name, "OG", False))
        rows.append((f"W32-{name}", "warm", name, None, False))
        rows.append((f"K32-{name}", "cold", name, None, True))
        rows.append((f"V32-{name}", "preview", name, None, False))
        rows.append((f"VG32-{name}", "preview", name, "VG", False))
        rows.append((f"GR32-{name}", "grain", name, None, False))
        rows.append((f"VF32F-{name}", "frame", name, "F", False))
        rows.append((f"VF32D-{name}", "frame", name, "D", False))
        for shape in ("L0", "L1", "L4", "FX"):
            rows.append((f"{shape}-32-{name}", "local", name, shape, False))
    return rows


def run_float(arguments, output, env, binary, files, receipt):
    """The float development's charges against measured peaks. The gate is the charge the app
    itself computes (`gate_charge`, emitted by the probe from `RawSourceWork.scratch` and
    `RawLocalWork.measuredScratch` with `float: true`): >= ceil(1.25 x max E) in every case."""
    log = output / "runs-float.log"
    results = {"fixtures": receipt, "cases": {}, "stops": [], "harness_limit": HARNESS_LIMIT}
    print("FIXTURES " + json.dumps(receipt), flush=True)
    calibration = run(binary, ["calibrate"], env, log)
    results["calibration"] = calibration
    print("CALIBRATE " + json.dumps(calibration), flush=True)
    if not (calibration.get("drop_ok") and calibration.get("e256_ok")):
        results["stops"].append("calibration missed")
    real = [name for name in files if name not in SIZES and name != "NEF"]
    plan = [c for c in float_cases(arguments.camera, real) if not arguments.only or c[0] in arguments.only]
    for label, mode, name, shape, cold in plan:
        target = saved_rz(binary, files, output, name, 32, env, log, label) if cold else files[name]
        argv = [mode, target, 32] + ([shape] if shape else [])
        runs = [run(binary, argv, env, log) for _ in range(arguments.runs)]
        twin = run(binary, ["baseline", *argv], env, log)
        for index, reply in enumerate(runs, 1):
            print(f"RUN {label} {index} {json.dumps(reply)}", flush=True)
            if reply.get("alerts"):
                results["stops"].append(f"{label} run {index} showed an [AutomationAlerts] line")
        print(f"BASE {label} {json.dumps(twin)}", flush=True)
        if twin.get("alerts"):
            results["stops"].append(f"{label} twin showed an [AutomationAlerts] line")
        if twin.get("e", 0) > 16 * MIB:
            results["stops"].append(f"baseline twin {label} read E {twin.get('e')}")
        good = [r for r in runs if "e" in r and "refused" not in r and "setup_refused" not in r]
        refused = [r.get("refused") or r.get("setup_refused") or r.get("error") for r in runs if r not in good]
        first = runs[0]
        max_e = max((r["e"] for r in good), default=0)
        gate = first.get("gate_charge", 0)
        need = math.ceil(1.25 * max_e)
        case = {"label": label, "mode": mode, "fixture": name, "depth": 32, "shape": shape, "cold": cold,
                "runs": runs, "baseline": twin, "px": first.get("px", 0),
                "enc": first.get("encoded", first.get("file_bytes", 0)), "max_e": max_e, "need": need,
                "gate_charge": gate, "ratio": round(gate / max_e, 3) if max_e else None,
                "refused": refused[0] if refused else None}
        results["cases"][label] = case
        if refused:
            results["stops"].append(f"{label} was refused: {refused[0]}")
        elif len(good) != arguments.runs:
            results["stops"].append(f"{label}: only {len(good)} of {arguments.runs} runs measured")
        elif mode in ("warm", "cold", "grain") and not all(r.get("master_is_float") for r in good):
            results["stops"].append(f"{label}: the source is not a float development")
        elif mode == "grain" and not all(r.get("grained") for r in good):
            results["stops"].append(f"{label}: the grain changed nothing")
        elif mode == "frame" and not all(r.get("extended") for r in good):
            results["stops"].append(f"{label}: the frame did not draw both sides extended")
        elif gate < need:
            results["stops"].append(f"{label}: charge {gate:,} < ceil(1.25 x max E) {need:,}")

    print("\nCASE              px          enc  E runs (pre)                                  max E"
          "          charge   charge/E")
    for label, case in results["cases"].items():
        good = [r for r in case["runs"] if "e" in r and "refused" not in r and "setup_refused" not in r]
        es = ", ".join(f"{r['e']:,} ({r['pre']:,})" for r in good)
        print(f"{label:14s} {case['px']:>11,} {case['enc']:>11,}  {es}  max {case['max_e']:,}  "
              f"charge {case['gate_charge']:,}  {case['ratio']}  twin {case['baseline'].get('e')}"
              + (f"  REFUSED: {case['refused']}" if case["refused"] else ""), flush=True)
    (output / "measure-float.json").write_text(json.dumps(results, indent=2) + "\n")
    if results["stops"]:
        for stop in results["stops"]:
            print(f"STOP: {stop}", flush=True)
        return 1
    print("FLOAT CHARGES: every case >= 1.25 x max E, no STOP", flush=True)
    return 0


def run_admit(arguments, output, binary, files, receipt):
    """`--admit LIMIT`: `open` and `prep` once each, 16 and 8 bits, for the NEF and every real fixture."""
    limit = arguments.admit
    env = environment(output, limit)
    log = output / f"runs-admit-{limit}.log"
    committed = committed_constants()
    if committed is None:
        raise SystemExit("RawSourceWork.swift has no scratch constants.")
    results = {"limit": limit, "constants": committed, "cases": {}, "stops": []}
    names = [n for n in files if n not in SIZES]
    print(f"ADMIT at DR_WORKING_LIMIT_BYTES={limit:,} with {json.dumps(committed)}")
    print("CASE         outcome  charge           open admission   px")
    for name in names:
        for mode, letter in (("open", "O"), ("prep", "P")):
            for depth in (16, 8):
                label = f"{letter}{depth}-{name}"
                reply = run(binary, [mode, files[name], depth], env, log)
                px = reply.get("px", 0)
                enc = reply.get("encoded", reply.get("file_bytes", receipt[name]["bytes"]))
                value = charge(committed, px, enc) if px else 0
                figure = value + enc + 4096 if px else 0
                sentence = reply.get("refused") or reply.get("setup_refused") or reply.get("error")
                results["cases"][label] = {"reply": reply, "px": px, "enc": enc, "charge": value,
                                           "open_admission": figure, "ok": not sentence, "sentence": sentence}
                print(f"{label:12s} {'ok' if not sentence else 'REFUSED':8s} {value:>15,}  {figure:>15,}  "
                      f"{px:>11,}" + (f"  {sentence}" if sentence else ""), flush=True)
                if reply.get("alerts"):
                    results["stops"].append(f"{label} showed an [AutomationAlerts] line")
                if sentence and not sentence.startswith(ADMIT_PREFIX):
                    results["stops"].append(f"{label} failed outside RasterResources.admit: {sentence}")
                if (sentence and mode == "open" and name != "NEF" and px and px <= 46_000_000
                        and figure < limit):
                    results["stops"].append(f"{label} was refused although its open admission figure "
                                            f"{figure:,} < {limit:,}: {sentence}")
    print("\nSTOPS " + json.dumps(results["stops"]))
    (output / f"admit-{limit}.json").write_text(json.dumps(results, indent=1, default=str))
    return 1 if results["stops"] else 0


def run_phase2(arguments, output, env, binary, files, receipt):
    log = output / "runs-phase2.log"
    results = {"fixtures": receipt, "cases": {}, "stops": [], "harness_limit": HARNESS_LIMIT}
    print("FIXTURES " + json.dumps(receipt), flush=True)
    calibration = run(binary, ["calibrate"], env, log)
    results["calibration"] = calibration
    print("CALIBRATE " + json.dumps(calibration), flush=True)
    if not (calibration.get("drop_ok") and calibration.get("e256_ok")):
        results["stops"].append("calibration missed")
    real = [name for name in files if name not in SIZES and name != "NEF"]
    plan = [c for c in phase2_cases(arguments.camera, real) if not arguments.only or c[0] in arguments.only]
    for label, mode, name, depth, fitted, shape, cold, k, e in plan:
        target = saved_rz(binary, files, output, name, depth, env, log, label) if cold else files[name]
        argv = [mode, target, depth, shape]
        runs = [run(binary, argv, env, log) for _ in range(arguments.runs)]
        twin = run(binary, ["baseline", *argv], env, log)
        for index, reply in enumerate(runs, 1):
            print(f"RUN {label} {index} {json.dumps(reply)}", flush=True)
            if reply.get("alerts"):
                results["stops"].append(f"{label} run {index} showed an [AutomationAlerts] line")
        print(f"BASE {label} {json.dumps(twin)}", flush=True)
        if twin.get("alerts"):
            results["stops"].append(f"{label} twin showed an [AutomationAlerts] line")
        if twin.get("e", 0) > 16 * MIB:
            results["stops"].append(f"baseline twin {label} read E {twin.get('e')}")
        if shape in GENERATED_SHAPES:
            # Vision finding nothing (`no_result`) is the stage's result and keeps its E;
            # the session must then have dropped the generating create on its first run (0
            # operations left), and a run that made a mask must still hold it (1).
            for index, reply in enumerate(runs, 1):
                if "e" not in reply or "refused" in reply or "setup_refused" in reply:
                    continue
                expected = 0 if "no_result" in reply else 1
                if reply.get("operations_after") != expected:
                    results["stops"].append(
                        f"{label} run {index}: operations_after {reply.get('operations_after')} "
                        f"where {expected} was expected ({'no_result' if expected == 0 else 'a mask'})")
                if "no_result" in reply and shape in MUST_GENERATE and name == "Z6":
                    results["stops"].append(f"{label} run {index} made no mask: {reply['no_result']}")
        good = [r for r in runs if "e" in r and "refused" not in r and "setup_refused" not in r]
        refused = [r.get("refused") or r.get("setup_refused") or r.get("error") for r in runs if r not in good]
        first = runs[0]
        # The charge's shape as `RawLocalWork.chargeShape` gave it for this case's operations:
        # every local counts in kt (the tone term), and ts marks a tone that reads a plane.
        kt = runs[0].get("tone_locals", k) if mode == "local" else 0
        ts = bool(runs[0].get("tone_stage", False)) if mode == "local" else False
        case = {"label": label, "mode": mode, "fixture": name, "depth": depth, "fitted": fitted, "shape": shape,
                "cold": cold, "k": k, "e": e, "kt": kt, "ts": ts, "runs": runs, "baseline": twin,
                "px": first.get("px", 0), "enc": first.get("encoded", 0),
                "max_e": max((r["e"] for r in good), default=0), "refused": refused[0] if refused else None}
        if shape in GENERATED_SHAPES:
            case["no_result"] = [r.get("no_result") for r in runs]
            case["operations_after"] = [r.get("operations_after") for r in runs]
        results["cases"][label] = case
        if refused and fitted:
            # The core keeps a RAW's file and its local masks (2 B/px each) within one
            # 1 GiB aggregate (512 MiB before), whatever the working limit. That is a
            # recorded format ceiling, not an admission refusal, and the case leaves the fit.
            if AGGREGATE_SENTENCE in str(refused[0]):
                results.setdefault("aggregate_ceilings", {})[label] = refused[0]
                print(f"AGGREGATE CEILING {label}: {refused[0]}", flush=True)
            else:
                results["stops"].append(f"fitted case {label} was refused: {refused[0]}")
        if mode == "publish" and good:
            estimate = first.get("projection_scratch_estimate", 0)
            case["projection_scratch_estimate"] = estimate
            case["projection_ratio"] = round(estimate / case["max_e"], 3) if case["max_e"] else None
            # The publish projection charge is `projectionScratchEstimate()`,
            # the structural estimate plus `ProjectionMargin`, gated at
            # ≥ ⌈1.25 × max E⌉.
            floor = (case["max_e"] * 5 + 3) // 4
            if estimate < floor:
                results["stops"].append(f"{label}: projectionScratchEstimate {estimate:,} < "
                                        f"⌈1.25 × max E⌉ {floor:,}")

    print("\nCASE              px          enc  E runs (pre)                                        max E  twin")
    for label, case in results["cases"].items():
        good = [r for r in case["runs"] if "e" in r and "refused" not in r and "setup_refused" not in r]
        es = ", ".join(f"{r['e']:,} ({r['pre']:,})" for r in good)
        extra = ""
        if case["mode"] == "publish":
            extra = (f"  projectionScratchEstimate {case.get('projection_scratch_estimate', 0):,}"
                     f" ({case.get('projection_ratio')}× max E)")
        elif case["mode"] == "local":
            extra = f"  setup_retained {case['runs'][0].get('setup_retained', 0):,}"
            if "operations_after" in case:
                extra += (f"  operations_after {case['operations_after']}"
                          f"  no_result {sum(1 for v in case['no_result'] if v)}/{len(case['runs'])}")
        print(f"{label:14s} {case['px']:>11,} {case['enc']:>11,}  {es}  max {case['max_e']:,}  "
              f"twin {case['baseline'].get('e')}{extra}"
              + (f"  REFUSED: {case['refused']}" if case["refused"] else ""), flush=True)

    presence_local_check(results)
    fitted = [c for c in results["cases"].values() if c["fitted"] and not c["refused"] and c["max_e"] > 0]
    restore_rows = [c for c in fitted if c["mode"] == "restore"]
    local_rows = [c for c in fitted if c["mode"] == "local"]
    decode = committed_constants()
    fitted_constants = {}
    if restore_rows and any(r["px"] >= A_MIN_PX for r in restore_rows):
        fitted_constants.update({k: v for k, v in fit_restore(restore_rows).items()})
    if local_rows and any(r["px"] >= A_MIN_PX and r["shape"] == "L0" for r in local_rows):
        fitted_constants.update({k: v for k, v in fit_local(local_rows).items()})
    tone_cases = [r for r in local_rows if r["shape"] in TONE_SHAPES]
    if tone_cases and local_constants():
        base = dict(local_constants())
        base.update({n: fitted_constants[n] for n in LOCAL_NAMES if n in fitted_constants})
        fitted_constants.update(fit_tone(tone_cases, base))
        # LT16, LTS and LTD on a real camera file must admit at 8 GiB beside the opened document:
        # the request's budget, the session's 64 MiB display term and what the setup retained.
        for r in tone_cases:
            if r["shape"] in ("LT16", "LTS", "LTD") and r["fixture"] not in SIZES:
                reply = r["runs"][0]
                figure = reply.get("budget", 0) + 64 * MIB + reply.get("setup_retained", 0)
                r["admission_8gib"] = figure
                print(f"ADMIT 8GiB {r['label']}: budget {reply.get('budget', 0):,} + 64 MiB + retained "
                      f"{reply.get('setup_retained', 0):,} = {figure:,}", flush=True)
                if figure >= EIGHT_GIB:
                    results["stops"].append(f"{r['label']} would not admit at 8 GiB: {figure:,}")
    results["fit"] = fitted_constants
    print("\nFIT " + json.dumps(fitted_constants), flush=True)
    committed = local_constants()
    usable = {n: fitted_constants[n] for n in LOCAL_NAMES if n in fitted_constants}
    if committed and usable and any(committed[n] != usable[n] for n in usable):
        print(f"NOTE: RawLocalWork.swift's constants {committed} differ from this fit.")
    if committed:
        phase2_coverage(fitted, decode, committed, results)
        retained = menu_retained_per_px(output)
        source = "M0-D24 pre-preview reading"
        if not retained:
            source = "L0-D24 setup_retained (no menu-8589934592.json yet)"
            for depth in (16, 8):
                case = results["cases"].get(f"L0-{depth}-D24")
                if case and case["runs"][0].get("setup_retained"):
                    retained[depth] = case["runs"][0]["setup_retained"] / case["px"]
        results["ceilings"] = {"retained_per_px": retained, "source": source,
                               "rows": ceilings_table(decode, committed, retained, receipt,
                                                      fixture_pixels(results["cases"]))}
        print(f"\nCEILINGS (retained per px from {source}) " + json.dumps(results["ceilings"]))
    print("\nSTOPS " + json.dumps(results["stops"]))
    (output / "measure-phase2.json").write_text(json.dumps(results, indent=1, default=str))
    return 1 if results["stops"] else 0


def presence_local_check(results):
    """The committed local charge (`measuredScratch`, k 1 with effects) ÷ max E of every PX
    case; a STOP below ⌈1.25 × max E⌉ or for a PX case that did not run."""
    decode, local = committed_constants(), local_constants()
    checked = {}
    for label, case in results["cases"].items():
        if case["shape"] != "PX":
            continue
        if case["refused"] or not case["max_e"]:
            results["stops"].append(f"Presence case {label} did not run: {case['refused']}")
            continue
        value = measured_charge(decode, local, case["px"], case["enc"], case["k"], case["e"])
        need = need_of(case)
        checked[label] = {"need": need, "charge": value, "margin": value - need,
                          "ratio": round(value / case["max_e"], 3),
                          "budget": case["runs"][0].get("budget")}
        if value < need:
            results["stops"].append(f"the committed local charge covers {label} below 1.25×: {value:,} < {need:,}")
    if checked:
        results["presence_check"] = checked
        print("\nPRESENCE CASE         need            committed       margin         ratio")
        for label, row in checked.items():
            print(f"{label:20s} {row['need']:>15,} {row['charge']:>15,} {row['margin']:>14,}  {row['ratio']:6.3f}")


def run_menu(arguments, output, binary, files, receipt):
    limit = arguments.menu
    env = environment(output, limit)
    log = output / f"runs-menu-{limit}.log"
    results = {"limit": limit, "cases": {}, "stops": []}
    real = [name for name in files if name not in SIZES and name != "NEF"]
    plan = [c for c in menu_cases(arguments.camera, real) if not arguments.only or c[0] in arguments.only]
    for label, name, depth, sequence in plan:
        reply = run(binary, ["menu", files[name], depth, sequence], env, log)
        print(f"MENU {label} {json.dumps(reply)}", flush=True)
        results["cases"][label] = reply
        if reply.get("alerts"):
            results["stops"].append(f"{label} showed an [AutomationAlerts] line")
        setup = reply.get("setup_refused") or reply.get("error")
        if setup:
            # An admission refusal of the open itself is recorded as an open
            # ceiling. Any other setup failure is a STOP.
            if setup.startswith(ADMIT_PREFIX):
                results.setdefault("open_ceilings", {})[label] = setup
                print(f"OPEN CEILING {label}: {setup}", flush=True)
            else:
                results["stops"].append(f"{label} setup failed: {setup}")
        for stage in reply.get("stages", []):
            sentence = stage.get("sentence")
            if sentence and not sentence.startswith(ADMIT_PREFIX):
                results["stops"].append(f"{label} {stage['stage']} refused outside RasterResources.admit: {sentence}")
    print(f"\nMENU at DR_WORKING_LIMIT_BYTES={limit:,}")
    print("CASE         stage       outcome  charge           unique_retained  transient")
    for label, reply in results["cases"].items():
        for stage in reply.get("stages", []):
            outcome = "ok" if stage.get("ok") else "REFUSED"
            print(f"{label:12s} {stage['stage']:11s} {outcome:8s} {stage.get('charge', 0):>15,}  "
                  f"{stage.get('unique_retained_bytes', 0):>15,}  {stage.get('transient_estimate_bytes', 0):>12,}"
                  + (f"  {stage['sentence']}" if stage.get("sentence") else ""))
    decode, local = committed_constants(), local_constants()
    retained = {}
    for depth in (16, 8):
        reply = results["cases"].get(f"M0-{depth}-D24") or results["cases"].get(f"M0-{depth}-D3")
        stage = next((s for s in (reply or {}).get("stages", []) if s.get("stage") == "preview 1"), None)
        if stage and stage.get("unique_retained_bytes"):
            retained[depth] = stage["unique_retained_bytes"] / reply["px"]
    if decode and local and retained:
        pixels = {name: results["cases"][label]["px"] for label, name, _, _ in plan
                  if name in real and results["cases"][label].get("px")}
        results["ceilings"] = {"retained_per_px": retained,
                               "rows": ceilings_table(decode, local, retained, receipt, pixels)}
        print("\nCEILINGS (retained per px from M0's pre-preview reading) " + json.dumps(results["ceilings"]))
    print("\nSTOPS " + json.dumps(results["stops"]))
    (output / f"menu-{limit}.json").write_text(json.dumps(results, indent=1, default=str))
    return 1 if results["stops"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
