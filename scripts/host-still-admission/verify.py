#!/usr/bin/env python3
"""The host-still admission MCP drive, on a built Release app.

Host-still admission is the measured per-depth affine figure
(`HostStillDecoder.scratchFixed8` + `scratchPerPixel8`·px, and the `Wide`
pair) and plain
PNG/JPEG placement the measured `ImageSourceFile.scratchPerPixel8` /
`scratchPerPixel16`; their measurement records sit beside the constants.
This drive pins what those figures and the whole-open charge
(`HostStillDecoder.openScratch`) admit, at the limits named per gate
below. The gates are FIXED here before the drive runs; a miss is recorded in
`receipt.json` and fails its gate, and nothing is widened after the fact.

Every gate runs in its own fresh owned `HostApp` (there is no
`close_document` tool), and every call names its `document_id`. The
fixtures are authored into this run's private folder by `author.swift`
(ImageIO / Core Image only, no Doctored code) and `avifenc`, and their
bytes and SHA-256 are recorded.

Each gate runs at an explicit working limit, set through
`DR_WORKING_LIMIT_BYTES` for its own app and checked as the gate's first
check (`resource_usage.working_limit_bytes`): `MINIMUM_MAC_LIMIT`
(8,589,934,592, what the 16 GB minimum Mac gets automatically) for the
48 MP and larger opens and the place, the 2 GiB test pin for A0, A4–A6 and A9, and
`PLAN_PROBE_LIMIT` for A7-plan.

- **A0** `resource_usage.working_limit_bytes == 2147483648`, the test pin
  the runner sets.
- **A1** at `MINIMUM_MAC_LIMIT`: `open_document` on heic8-48 (8064x6048
  8-bit HEIC) succeeds as an 8064x6048 8-bit document. Its whole-open charge (`HostStillDecoder.openScratch`) is
  2,180,368,909.
- **A2** the same for heic8-gainmap-48,
  also at `MINIMUM_MAC_LIMIT` (open charge 2,176,287,400).
- **A3** the same for avif8-48, at `MINIMUM_MAC_LIMIT` (open charge
  2,103,027,151).
- **A4** `/System/Library/Desktop Pictures/iMac Blue.heic` opens as 6016x6016,
  from a byte-identical copy in the run's output folder (the path guard
  refuses `/System`).
- **A5** from `new_document` 4032x3024 8-bit: (1) `place_image` heic8-48
  embedded succeeds; (2) `place_image` of heic8-gainmap-48 succeeds.
- **A6** (12 MP regression) heic8-12, avif8-12 and heic10-12 open;
  heic10-12 opens at 16 bits with `precision: preserve`.
- **A7** (plain open) `open_document` on jpeg8-48 and, in a second fresh
  app, png8-48 each succeed as an 8064x6048 8-bit document, at
  `MINIMUM_MAC_LIMIT`. The core's flat JPEG/PNG open is charged whole up
  front (`FlatOpenAdmission.openScratch`: 34 B/px + 3·encoded,
  about 1.72 GB for the JPEG and 1.97 GB for the PNG here).
- **A7-plan** at `PLAN_PROBE_LIMIT` (1 GiB): `open_document` on jpeg8-48
  is refused with `RasterResources.admit`'s "…of working memory on its
  own…" sentence, so the flat open records a plan.
- **A8** (plain place) from `new_document` 8064x6048 8-bit, `place_image`
  jpeg8-48 embedded succeeds, at `MINIMUM_MAC_LIMIT`. MUST PASS: the
  measured place fit check passed (`PLACE_FIT_PASSED`). The place's display
  preparation (`PreparedDocumentEdit`) also charges the projection with
  `ProjectionMargin`'s 7/16 at 8 bits beside the document's own bytes; the
  place's own charge is unchanged.
- **A9** (multi-image container) heic8-pair-12, a two-image 8-bit HEIC whose
  primary image is index 1 (4032x3024 at 240 ppi; index 0 is 2016x1512 at
  72 ppi), opens as a 4032x3024 8-bit document with `get_document` resolution 240x240 ppi
  (the primary image's metadata, not index 0's),
  and `/System/Library/Desktop Pictures/Sonoma.heic` (a two-image light/dark
  wallpaper, primary index 0) opens as 6016x6016, from a byte-identical copy
  in the run's output folder, as A4.
- **A10** (wide) at `MINIMUM_MAC_LIMIT`: heic10 and avif10 at 48, 67, 96 and
  100 MP each open with `precision: preserve` as a 16-bit document of the
  fixture's size (8064x6048, 9456x7096, 12000x8000, 12500x8000; one
  16-bit image now holds the 100 MP document limit). MUST PASS: the measured
  wide fit check passed (`WIDE_FIT_PASSED`; the whole-open record beside
  `HostStillDecoder.scratchFixedWide`, 45 MiB + 42 B/px, refit on the open
  that lands one raster layer). After the heic10-100 open, `add_layer_mask`
  on layer 0 is RECORDED, not gated: it SUCCEEDS (one 16-bit raster layer is
  8 B/px, 800 MB at 100 MP of the typed decoded cap, then 1.6 GB and now
  8 GiB, so a canvas-sized mask fits). Earlier, the wide still kept its
  16-bit master beside the canvas raster (16 B/px), which filled the 1.6 GB
  cap, and the mask was refused. A refusal must leave `get_document`
  unchanged, and that is gated.
- **A11** at `MINIMUM_MAC_LIMIT`: heic10-96 and avif10-96 open (they were once
  refused before the decode; they now fit), 12000x8000 at 16 bits.
- **A12** at `MINIMUM_MAC_LIMIT`: png16-96 and png16-100 open with `precision:
  preserve` (the core's own 16-bit open) as 16-bit documents of their size,
  and `add_layer_mask` on layer 0 of png16-100 succeeds (an 8 B/px PNG is
  800 MB of the typed decoded cap, now 8 GiB).
- **N1** at `MINIMUM_MAC_LIMIT`: `new_document` 10000x10000 at depth 16 (File
  ▸ New at 100 MP), `save_copy` to a `.rz`, and `open_document` of that file
  each succeed, and the reopened document is 10000x10000 at 16 bits.
- **R** (recorded) png16-48 and png16-67 open with `precision: preserve`, at
  `MINIMUM_MAC_LIMIT`. Their open is the core's own precision admission,
  not the host-still charge, so it is recorded, not gated.

`--measure RUNS` runs no gate: it records the wide-open footprint table (the 96
and 100 MP rows as whole opens) instead, every `WIDE_KINDS`
fixture at every `WIDE_SIZES` size in `RUNS` fresh apps each plus a baseline
twin per run, at `MINIMUM_MAC_LIMIT` with `MallocLargeCache=0`, into
`receipt.json`'s `measurements`.

Usage: python3 -B scripts/host-still-admission/verify.py --app <Doctored.app>
[--output <folder outside the repository>] [--measure RUNS]. Exit 0 only when
every must-pass gate passed.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))
import support  # noqa: E402
import app_under_test  # noqa: E402

LIMIT = 2147483648
# What the 16 GB minimum Mac gets automatically (`WorkingLimit.automatic`,
# 8 GiB): A1–A3, A7, A8, A10–A12, N1 and R run here; A0, A4–A6 and A9 stay at
# LIMIT, the 2 GiB test pin.
MINIMUM_MAC_LIMIT = 8589934592
# A7-plan: below the 48 MP JPEG's flat-open charge on its own.
PLAN_PROBE_LIMIT = 1073741824
IMAC = Path('/System/Library/Desktop Pictures/iMac Blue.heic')
SONOMA = Path('/System/Library/Desktop Pictures/Sonoma.heic')
# Step 3's fit checks, decided from the measurement before this drive ran.
PLACE_FIT_PASSED = True
WIDE_FIT_PASSED = True
# 100 MP (12500x8000) is the document limit (`MAX_PIXELS`); 67 MP (9456x7096)
# was the largest 4:3 16-bit image the core once kept.
SIZES = {12: (4032, 3024), 48: (8064, 6048), 67: (9456, 7096), 96: (12000, 8000),
         100: (12500, 8000)}
# The wide-open measurement (`--measure`): every size a wide still is measured
# at. With a 1 GiB `MAX_DRDC_PNG_LEN` and no decoder cap on a 16-bit plane,
# every size opens whole, 96 and 100 MP included; those two were once measured
# as opens the core refused after the capture, and later refused before the decode.
WIDE_SIZES = (12, 48, 67, 96, 100)
WIDE_KINDS = ('heic10', 'avif10', 'png16')


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def system_copy(output: Path, source: Path) -> Path:
    """Copy a system wallpaper into the run folder and return the copy: the path
    guard refuses `/System` to every agent tool, so the gates open the copy."""
    folder = output / 'system'
    folder.mkdir(exist_ok=True)
    copy = folder / source.name
    shutil.copyfile(source, copy)
    return copy


def author(output):
    """Build the author, then write every fixture this drive opens."""
    binary = output / 'author'
    subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0',
                    str(HERE / 'author.swift'), '-o', str(binary)], check=True,
                   capture_output=True, timeout=600)
    folder = output / 'fixtures'
    folder.mkdir()
    made = {}
    for kind, size, ext in [('heic8', 12, 'heic'), ('heic8', 48, 'heic'),
                            ('heic8-gainmap', 48, 'heic'), ('jpeg8', 48, 'jpg'), ('png8', 12, 'png'),
                            ('png8', 48, 'png'), ('heic8-pair', 12, 'heic'),
                            *[(kind, size, ext) for kind, ext in (('heic10', 'heic'), ('png16', 'png'))
                              for size in WIDE_SIZES]]:
        path = folder / f'{kind}-{size}.{ext}'
        width, height = SIZES[size]
        subprocess.run([str(binary), kind, str(width), str(height), str(path)], check=True,
                       capture_output=True, timeout=600)
        made[f'{kind}-{size}'] = path
    for name, png, deep in [('avif8-12', 'png8-12', False), ('avif8-48', 'png8-48', False),
                            *[(f'avif10-{size}', f'png16-{size}', True) for size in WIDE_SIZES]]:
        path = folder / f'{name}.avif'
        subprocess.run(['avifenc', '-s', '8', '-y', '420', *(['-d', '10'] if deep else []),
                        str(made[png]), str(path)], check=True, capture_output=True, timeout=600)
        made[name] = path
    return made


def call(app, tool, **arguments):
    """One MCP call with a long timeout; answers (ok, payload-or-sentence).

    The shared worker's explicit busy refusal is retried (periodic recovery
    may own it between calls); every other refusal is returned as is.
    """
    deadline = time.monotonic() + 30
    while True:
        reply = app.client.post({'jsonrpc': '2.0', 'id': 1, 'method': 'tools/call',
                                 'params': {'name': tool, 'arguments': arguments}}, timeout=600)
        assert reply is not None and 'error' not in reply, reply
        result = reply['result']
        text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
        if result.get('isError') and 'Another background operation is running.' in text \
                and time.monotonic() < deadline:
            time.sleep(0.05)
            continue
        if result.get('isError'):
            return False, text
        return True, json.loads(text) if text else result


class Gate:
    def __init__(self, report, name, must=True):
        self.report, self.name, self.must = report, name, must
        self.detail = {}
        self.passed = True

    def check(self, label, condition, value=None):
        self.detail[label] = dict(ok=bool(condition), value=value)
        self.passed &= bool(condition)
        return bool(condition)

    def finish(self, error=None):
        if error is not None:
            self.detail['error'] = error
            self.passed = False
        self.report['gates'][self.name] = dict(passed=self.passed, must_pass=self.must,
                                               detail=self.detail)


def owned(output, name, executable):
    return support.HostApp(output / f'app-{name}', output / f'recovery-{name}', executable)


def run_gate(report, output, executable, name, body, must=True, limit=LIMIT):
    """One gate in its own app at `limit`, checked as the gate's first check."""
    gate = Gate(report, name, must)
    previous = os.environ.get('DR_WORKING_LIMIT_BYTES')
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(limit)
    try:
        with owned(output, name, executable) as app:
            ok, usage = call(app, 'resource_usage')
            gate.check('working_limit_bytes', ok and usage.get('working_limit_bytes') == limit,
                       usage.get('working_limit_bytes') if ok else usage)
            body(app, gate)
        gate.finish()
    except Exception as error:                      # Recorded, never repaired.
        gate.finish(f'{type(error).__name__}: {error}'[:800])
    finally:
        if previous is None:
            os.environ.pop('DR_WORKING_LIMIT_BYTES', None)
        else:
            os.environ['DR_WORKING_LIMIT_BYTES'] = previous


def opened(app, gate, label, path, width, height, expect='rgba8', **arguments):
    ok, reply = call(app, 'open_document', path=str(path), **arguments)
    if not gate.check(f'{label} open', ok, reply if not ok else None):
        return None
    document = reply['document']
    gate.check(f'{label} document', (document['width'], document['height'], document['precision'])
               == (width, height, expect),
               dict(width=document['width'], height=document['height'],
                    precision=document['precision']))
    return document['id']


def a0(app, gate):
    ok, usage = call(app, 'resource_usage')
    gate.check('working_limit_bytes', ok and usage.get('working_limit_bytes') == LIMIT,
               usage.get('working_limit_bytes') if ok else usage)


def a5(fixtures):
    def body(app, gate):
        ok, made = call(app, 'new_document', width=4032, height=3024, depth=8)
        if not gate.check('new_document', ok, None if ok else made):
            return
        document = made['document']['id']
        ok, placed = call(app, 'place_image', document_id=document,
                          path=str(fixtures['heic8-48']), storage='embedded')
        gate.check('place embedded', ok, None if ok else placed)
        ok, gainmap = call(app, 'place_image', document_id=document,
                           path=str(fixtures['heic8-gainmap-48']), storage='embedded')
        gate.check('place gain-map still', ok, None if ok else gainmap)
    return body


def a8(fixtures):
    def body(app, gate):
        ok, made = call(app, 'new_document', width=8064, height=6048, depth=8)
        if not gate.check('new_document', ok, None if ok else made):
            return
        ok, placed = call(app, 'place_image', document_id=made['document']['id'],
                          path=str(fixtures['jpeg8-48']), storage='embedded')
        gate.check('place jpeg8-48', ok, placed if not ok else placed.get('source'))
    return body


class Footprint(ctypes.Structure):
    """`rusage_info_v4`, read through `proc_pid_rusage(pid, RUSAGE_INFO_V4)`."""
    _fields_ = [('ri_uuid', ctypes.c_uint8 * 16)] + [(name, ctypes.c_uint64) for name in (
        'ri_user_time', 'ri_system_time', 'ri_pkg_idle_wkups', 'ri_interrupt_wkups',
        'ri_pageins', 'ri_wired_size', 'ri_resident_size', 'ri_phys_footprint',
        'ri_proc_start_abstime', 'ri_proc_exit_abstime', 'ri_child_user_time',
        'ri_child_system_time', 'ri_child_pkg_idle_wkups', 'ri_child_interrupt_wkups',
        'ri_child_pageins', 'ri_child_elapsed_abstime', 'ri_diskio_bytesread',
        'ri_diskio_byteswritten', 'ri_cpu_time_qos_default', 'ri_cpu_time_qos_maintenance',
        'ri_cpu_time_qos_background', 'ri_cpu_time_qos_utility', 'ri_cpu_time_qos_legacy',
        'ri_cpu_time_qos_user_initiated', 'ri_cpu_time_qos_user_interactive',
        'ri_billed_system_time', 'ri_serviced_system_time', 'ri_logical_writes',
        'ri_lifetime_max_phys_footprint', 'ri_instructions', 'ri_cycles', 'ri_billed_energy',
        'ri_serviced_energy', 'ri_interval_max_phys_footprint', 'ri_runnable_time')]


LIBSYSTEM = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
LIBSYSTEM.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
LIBSYSTEM.proc_reset_footprint_interval.argtypes = [ctypes.c_int]


def footprint(pid, reset=False):
    """(phys_footprint, interval max) of an owned process; `reset` restarts the interval first."""
    if reset and LIBSYSTEM.proc_reset_footprint_interval(pid) != 0:
        raise OSError(ctypes.get_errno(), 'proc_reset_footprint_interval failed')
    info = Footprint()
    if LIBSYSTEM.proc_pid_rusage(pid, 4, ctypes.byref(info)) != 0:
        raise OSError(ctypes.get_errno(), 'proc_pid_rusage failed')
    return info.ri_phys_footprint, info.ri_interval_max_phys_footprint


def measure_case(output, executable, name, path):
    """One wide open (or, with `path` None, the baseline twin) in a fresh app at 8 GiB.

    E = `ri_interval_max_phys_footprint` − pre around MCP `open_document`
    (`precision: preserve`), read after the footprint has been still for 2 s, so
    a projection drawn after the reply is inside the interval.
    """
    record = dict(case=name)
    with owned(output, name, executable) as app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        time.sleep(3)
        pid = app.process.pid
        pre, _ = footprint(pid, reset=True)
        if path is None:
            ok, reply = call(app, 'resource_usage')
        else:
            ok, reply = call(app, 'open_document', path=str(path), precision='preserve')
        last, still_since, deadline = None, time.monotonic(), time.monotonic() + 60
        while time.monotonic() < deadline and time.monotonic() - still_since < 2:
            time.sleep(0.25)
            now, _ = footprint(pid)
            if now != last:
                last, still_since = now, time.monotonic()
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if path is not None:
            document = reply.get('document', {}) if ok else {}
            record['document'] = (dict(width=document.get('width'), height=document.get('height'),
                                       precision=document.get('precision'))
                                  if ok else reply[:400])
    return record


def measure(output, executable, fixtures, runs):
    """Every wide case at every `WIDE_SIZES` size, `runs` fresh apps each, plus a
    baseline twin per run, at `MINIMUM_MAC_LIMIT` with `MallocLargeCache=0`."""
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(MINIMUM_MAC_LIMIT)
    os.environ['MallocLargeCache'] = '0'
    cases = []
    for run in range(1, runs + 1):
        cases.append(dict(measure_case(output, executable, f'baseline-{run}', None), run=run))
        print(json.dumps(cases[-1]), flush=True)
        for kind in WIDE_KINDS:
            for size in WIDE_SIZES:
                name = f'{kind}-{size}'
                path = fixtures[name]
                case = measure_case(output, executable, f'{name}-{run}', path)
                case.update(run=run, kind=kind, size=size, width=SIZES[size][0],
                            height=SIZES[size][1], encoded=path.stat().st_size)
                cases.append(case)
                print(json.dumps(case), flush=True)
    return cases


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path,
                        default=app_under_test.default_app_bundle())
    parser.add_argument('--output', type=Path)
    parser.add_argument('--measure', type=int, metavar='RUNS',
                        help='record the wide-open footprint table (RUNS fresh apps per '
                             'case) instead of running the gates')
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-host-still-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)), author_sha256=digest(HERE / 'author.swift'),
                  avifenc=shutil.which('avifenc'), place_fit_passed=PLACE_FIT_PASSED,
                  wide_fit_passed=WIDE_FIT_PASSED, gates={}, error=None)
    try:
        fixtures = author(output)
        report['fixtures'] = {name: dict(file=str(path), bytes=path.stat().st_size,
                                         sha256=digest(path)) for name, path in fixtures.items()}
        imac = system_copy(output, IMAC)
        sonoma = system_copy(output, SONOMA)
        report['imac_blue'] = dict(file=str(IMAC), copy=str(imac), bytes=IMAC.stat().st_size,
                                   sha256=digest(IMAC), copy_sha256=digest(imac))
        report['sonoma'] = dict(file=str(SONOMA), copy=str(sonoma),
                                bytes=SONOMA.stat().st_size, sha256=digest(SONOMA),
                                copy_sha256=digest(sonoma))
        if arguments.measure:
            report['measurements'] = measure(output, executable, fixtures, arguments.measure)
            (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
            print(output / 'receipt.json')
            return
        run_gate(report, output, executable, 'A0', a0)
        run_gate(report, output, executable, 'A1', lambda app, gate: opened(
            app, gate, 'heic8-48', fixtures['heic8-48'], 8064, 6048), limit=MINIMUM_MAC_LIMIT)
        # hdr_photo sdr (v1.x): A2 measures the gain map's SDR-base open.
        run_gate(report, output, executable, 'A2', lambda app, gate: opened(
            app, gate, 'heic8-gainmap-48', fixtures['heic8-gainmap-48'], 8064, 6048,
            hdr_photo='sdr'), limit=MINIMUM_MAC_LIMIT)
        run_gate(report, output, executable, 'A3', lambda app, gate: opened(
            app, gate, 'avif8-48', fixtures['avif8-48'], 8064, 6048), limit=MINIMUM_MAC_LIMIT)
        run_gate(report, output, executable, 'A4', lambda app, gate: opened(
            app, gate, 'imac-blue', imac, 6016, 6016))
        run_gate(report, output, executable, 'A5', a5(fixtures))

        def a6(app, gate):
            opened(app, gate, 'heic8-12', fixtures['heic8-12'], 4032, 3024)
            opened(app, gate, 'avif8-12', fixtures['avif8-12'], 4032, 3024)
            opened(app, gate, 'heic10-12', fixtures['heic10-12'], 4032, 3024, expect='rgba16',
                   precision='preserve')
        run_gate(report, output, executable, 'A6', a6)
        run_gate(report, output, executable, 'A7-jpeg8', lambda app, gate: opened(
            app, gate, 'jpeg8-48', fixtures['jpeg8-48'], 8064, 6048),
            limit=MINIMUM_MAC_LIMIT)
        run_gate(report, output, executable, 'A7-png8', lambda app, gate: opened(
            app, gate, 'png8-48', fixtures['png8-48'], 8064, 6048),
            limit=MINIMUM_MAC_LIMIT)
        def a7_plan(app, gate):
            ok, reply = call(app, 'open_document', path=str(fixtures['jpeg8-48']))
            gate.check('jpeg8-48 refused on its own', not ok and isinstance(reply, str)
                       and 'of working memory on its own' in reply, reply if not ok else 'opened')
        run_gate(report, output, executable, 'A7-plan', a7_plan, limit=PLAN_PROBE_LIMIT)
        run_gate(report, output, executable, 'A8', a8(fixtures), must=PLACE_FIT_PASSED,
                 limit=MINIMUM_MAC_LIMIT)
        def a9_pair(app, gate):
            document = opened(app, gate, 'heic8-pair-12', fixtures['heic8-pair-12'], 4032, 3024)
            if document is None:
                return
            ok, info = call(app, 'get_document', document_id=document)
            ppi = (info.get('resolution') or {}) if ok else {}
            gate.check('heic8-pair-12 resolution', ok and
                       (ppi.get('ppi_x'), ppi.get('ppi_y')) == (240, 240), ppi if ok else info)
        run_gate(report, output, executable, 'A9-pair', a9_pair)
        run_gate(report, output, executable, 'A9-sonoma', lambda app, gate: opened(
            app, gate, 'sonoma', sonoma, 6016, 6016))
        def wide_open(app, gate, name, mask=None):
            """A 16-bit open of `name`. `mask`: `add_layer_mask` on layer 0 afterwards, 'must'
            (gated) or 'recorded' (its outcome is recorded; a refusal must leave `get_document`
            unchanged)."""
            width, height = SIZES[int(name.rsplit('-', 1)[1])]
            document = opened(app, gate, name, fixtures[name], width, height, expect='rgba16',
                              precision='preserve')
            if document is None or mask is None:
                return
            _, before = call(app, 'get_document', document_id=document)
            ok, reply = call(app, 'add_layer_mask', document_id=document, layer=0)
            if mask == 'must':
                gate.check(f'{name} add_layer_mask', ok, None if ok else reply)
                return
            gate.detail[f'{name} add_layer_mask (recorded)'] = dict(
                ok=ok, recorded=True, value=None if ok else reply)
            if not ok:
                _, after = call(app, 'get_document', document_id=document)
                gate.check(f'{name} refused mask left the document unchanged', after == before,
                           None if after == before else dict(before=before, after=after))
        for kind in ('heic10', 'avif10'):
            for size in (48, 67, 96, 100):
                name = f'{kind}-{size}'
                run_gate(report, output, executable, f'A10-{name}',
                         lambda app, gate, name=name: wide_open(
                             app, gate, name, mask='recorded' if name == 'heic10-100' else None),
                         must=WIDE_FIT_PASSED, limit=MINIMUM_MAC_LIMIT)
        for name in ('heic10-96', 'avif10-96'):
            run_gate(report, output, executable, f'A11-{name}',
                     lambda app, gate, name=name: wide_open(app, gate, name),
                     limit=MINIMUM_MAC_LIMIT)
        for name in ('png16-96', 'png16-100'):
            run_gate(report, output, executable, f'A12-{name}',
                     lambda app, gate, name=name: wide_open(
                         app, gate, name, mask='must' if name == 'png16-100' else None),
                     limit=MINIMUM_MAC_LIMIT)

        def n1(app, gate):
            ok, made = call(app, 'new_document', width=10000, height=10000, depth=16)
            if not gate.check('new_document', ok, None if ok else made):
                return
            document = made['document']
            gate.check('new document', (document['width'], document['height'],
                                        document['precision']) == (10000, 10000, 'rgba16'),
                       dict(width=document['width'], height=document['height'],
                            precision=document['precision']))
            path = output / 'n1' / 'blank-100.rz'
            path.parent.mkdir()
            ok, saved = call(app, 'save_copy', document_id=document['id'], path=str(path))
            if not gate.check('save_copy', ok, None if ok else saved):
                return
            gate.check('saved file', path.is_file() and path.stat().st_size > 0,
                       path.stat().st_size if path.is_file() else None)
            ok, reply = call(app, 'open_document', path=str(path), precision='preserve')
            if not gate.check('open_document', ok, None if ok else reply):
                return
            reopened = reply['document']
            gate.check('reopened document', (reopened['width'], reopened['height'],
                                             reopened['precision']) == (10000, 10000, 'rgba16'),
                       dict(width=reopened['width'], height=reopened['height'],
                            precision=reopened['precision']))
        run_gate(report, output, executable, 'N1', n1, limit=MINIMUM_MAC_LIMIT)
        for size in (48, 67):
            name, (width, height) = f'png16-{size}', SIZES[size]
            run_gate(report, output, executable, f'R-{name}',
                     lambda app, gate, name=name, width=width, height=height: opened(
                         app, gate, name, fixtures[name], width, height, expect='rgba16',
                         precision='preserve'), must=False, limit=MINIMUM_MAC_LIMIT)
    except BaseException as error:                 # Recorded, never repaired.
        report['error'] = f'{type(error).__name__}: {error}'
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    summary = {name: ('PASS' if gate['passed'] else 'FAIL' if gate['must_pass'] else 'RECORDED-FAIL')
               for name, gate in report['gates'].items()}
    print(json.dumps(dict(error=report['error'], gates=summary,
                          receipt=str(output / 'receipt.json')), indent=2))
    passed = not report['error'] and report['gates'] and all(
        gate['passed'] for gate in report['gates'].values() if gate['must_pass'])
    raise SystemExit(0 if passed else 1)


if __name__ == '__main__':
    main()
