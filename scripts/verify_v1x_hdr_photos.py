#!/usr/bin/env python3
"""The v1.x HDR photo drive: open, choice, export, EDR view.

    python3 scripts/hdr-photos/author.py --output ~/devel/doctored-fixtures/hdr-photos
    python3 scripts/verify_v1x_hdr_photos.py --fixtures ~/devel/doctored-fixtures/hdr-photos \\
        [--output <folder>] [--measure RUNS]

Every app is an owned instance (`phase5_support.App`: isolated preset folders,
a dummy API key, `DR_BACKGROUND=1`, `DR_EDR_HEADROOM=4` so an SDR Mac still
engages HDR Preview, and `-OpenHDRPhotosAsHDR YES` as an argv default, never
the user's preferences). The fixtures are the authored scenes of
`scripts/hdr-photos/author.py`; their oracle is its `oracle.json`.

# Bounds, declared before the first run

- PQ/HLG, lossy (HEIC, AVIF): the opened linear value, taken back to the file's
  primaries (`oracle.json`'s `to_file`) and through the inverse of the open's
  rule (PQ: inverse EOTF of 203·v; HLG: inverse OOTF at 1000 cd/m², γ 1.2, then
  the OETF), is within 2 ten-bit codes of the authored code.
- PQ/HLG, lossless (16-bit PNG cICP, JPEG XL): the same signal within 1/1023.
- Gain maps: |opened − authored| ≤ 0.04·|authored| + 0.004 per channel at
  every patch centre, in the document's own primaries.
- The ISO 21496-1 oracle, for every fixture whose gain map carries ISO-style
  metadata: (base + k_base)·2^(min + (max − min)·g^(1/γ)) − k_alt at full
  headroom, per channel, from `author.swift inspect`'s base linear values,
  gain codes and metadata, written here independently of Core Image; the
  opened value within 0.01·|oracle| + 0.002.
- Export round trips: a gain-map HEIC exported from an opened gain-map photo
  reopens within the gain-map bound of the exported document's own values; a
  PQ HEIC exported from the PQ PNG reopens within 2 ten-bit codes of the
  document's own signal (PQ of 203·v in BT.2020).
- Refusals return their exact sentences.
- EDR (`DR_EDR_HEADROOM=4`): get_view reports edr_headroom 4, hdr_preview true,
  and once hdr_preview_displayed is true: edr_layer true and edr_content_peak
  within 0.5 % of the document's peak; +1 stop doubles it (within 0.5 %); off
  restores edr_layer false and the view image hash the window showed before
  HDR Preview engaged; sample_pixel's linear values are identical on and off.

The owner's ProRAW DNG (`owner/IMG_0010.DNG`, sha256 a9af581f…8ae5c35) is a
real-world case, not an oracle gate: absent or a different hash is SKIP and
the file is not read further. It is opened in place, read-only, through the
RAW pipeline at linear float in an 8 GiB instance; the receipt holds numbers
and words only (the share of pixels above 1.0 and the maximum, HDR Preview's
engagement and peak, the view hash equality off/on/off, and what ImageIO
reports of its gain maps through `author.swift inspect`). Nothing of it is
copied, rendered or thumbnailed.

`--measure RUNS` runs no gate: on a Release app (`DR_VERIFY_APP`), one case
per process, `MallocLargeCache=0`, `DR_WORKING_LIMIT_BYTES` 8 GiB, RUNS runs
plus a baseline twin per run, E = `ri_interval_max_phys_footprint` − pre:
(a) the HDR photo open of the 12 and 48 MP gain-map and PQ HEICs
(`DR_EDR_HEADROOM=1`, so no EDR render follows), read once the footprint has
been still for 2 s; (b) the EDR view of the same documents: HDR Preview off,
settled, the interval reset, HDR Preview on, read once hdr_preview_displayed
is true and the footprint still; (c) the HDR HEIC export of the same documents
(`DR_EDR_HEADROOM=1`): opened and settled, the interval reset, then one
`export_image` (heic, bit_depth 32, `hdr_encoding` gain_map for a gain-map
source, pq for a PQ one) — the core's intermediate AND the host's encode — read
once the footprint has been still for 2 s. The plan each must cover is printed
beside it (`HDRStillDecoder.openScratch`, `float_view::extended_display_scratch`,
`doc_export::export_plan_for`'s 32-bit term). `--measure-phases` picks phases
from open,edr,export (all by default).
"""

import argparse
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import phase5_support  # noqa: E402

DNG_SHA256 = 'a9af581f6179adaac85792341a492f60879e1e6d390376bb1046385df8ae5c35'
DNG_NAME = 'owner/IMG_0010.DNG'
GIB8 = 8589934592
PQ_SDR = ('This image is an HDR photo coded in PQ, which has no SDR rendition to open. '
          'Open it as HDR (File ▸ Open HDR Photos as HDR, or hdr_photo "hdr") and convert '
          'it to 16 bits with Image ▸ Mode ▸ Convert Precision….')
HLG_SDR = PQ_SDR.replace('coded in PQ', 'coded in HLG')
BAD_CHOICE = 'hdr_photo must be hdr or sdr.'
AVIF_REFUSAL = ("This system has no HDR AVIF encoder (Core Image writes no AVIF, and ImageIO's "
                'HDR AVIF encode fails); export the 32-bit document as an HDR HEIC instead.')
NO_HDR_ENCODING = 'a 32-bit float export requires tiff, exr or hdr'


# --------------------------------------------------------------- the formulas

def pq_signal(nits):
    y = max(0.0, min(1.0, nits / 10000.0))
    p = y ** 0.1593017578125
    return ((0.8359375 + 18.8515625 * p) / (1 + 18.6875 * p)) ** 78.84375


def hlg_signal(display, luma):
    """Display-linear (1.0 = 203 cd/m2) to BT.2100 HLG, per channel."""
    nits = [max(0.0, c) * 203.0 for c in display]
    yd = sum(l * n for l, n in zip(luma, nits))
    if yd <= 0:
        return [0.0, 0.0, 0.0]
    ys = (yd / 1000.0) ** (1 / 1.2)
    scene = [n / (1000.0 * ys ** 0.2) for n in nits]
    a, b = 0.17883277, 0.28466892
    c = 0.55991073
    return [math.sqrt(3 * e) if e <= 1 / 12 else a * math.log(12 * e - b) + c for e in scene]


def apply(m, v):
    return [sum(m[i][k] * v[k] for k in range(3)) for i in range(3)]


def iso_gain(base, codes, metadata):
    """ISO 21496-1 at full headroom, per channel (BaseColorIsWorkingColor)."""
    channels = metadata.get('channels') or []
    out = []
    for c in range(3):
        meta = channels[c] if len(channels) == 3 else channels[0]
        g = codes[c] if len(codes) == 3 else codes[0]
        gamma = meta.get('Gamma', 1.0)
        lo, hi = meta['GainMapMin'], meta['GainMapMax']
        log_gain = lo + (hi - lo) * g ** (1 / gamma)
        weight = 1.0 if metadata.get('AlternateHeadroom', 1) > metadata.get('BaseHeadroom', 0) else 0.0
        out.append((base[c] + meta.get('BaseOffset', 0.0)) * 2 ** (log_gain * weight)
                   - meta.get('AlternateOffset', 0.0))
    return out


# ------------------------------------------------------------------- plumbing

class Footprint(ctypes.Structure):
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
    if reset and LIBSYSTEM.proc_reset_footprint_interval(pid) != 0:
        raise OSError(ctypes.get_errno(), 'proc_reset_footprint_interval failed')
    info = Footprint()
    if LIBSYSTEM.proc_pid_rusage(pid, 4, ctypes.byref(info)) != 0:
        raise OSError(ctypes.get_errno(), 'proc_pid_rusage failed')
    return info.ri_phys_footprint, info.ri_interval_max_phys_footprint


def call(app, tool, **arguments):
    """One MCP call (long timeout): (ok, payload or sentence)."""
    deadline = time.monotonic() + 30
    while True:
        reply = app.client.post({'jsonrpc': '2.0', 'id': 1, 'method': 'tools/call',
                                 'params': {'name': tool, 'arguments': arguments}}, timeout=900)
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


class Instance:
    """One owned app with this drive's environment."""

    def __init__(self, output, name, limit=2147483648, headroom='4'):
        self.folder = output / f'app-{name}'
        self.limit, self.headroom = limit, headroom

    def __enter__(self):
        self.saved = {key: os.environ.get(key) for key in
                      ('DR_WORKING_LIMIT_BYTES', 'DR_EDR_HEADROOM', 'DR_BACKGROUND')}
        os.environ['DR_WORKING_LIMIT_BYTES'] = str(self.limit)
        os.environ['DR_EDR_HEADROOM'] = self.headroom
        os.environ['DR_BACKGROUND'] = '1'
        self.app = phase5_support.App(self.folder, self.folder / 'recovery',
                                      extra_defaults=['-OpenHDRPhotosAsHDR', 'YES'])
        return self.app.__enter__()

    def __exit__(self, *details):
        try:
            self.app.__exit__(*details)
        finally:
            for key, value in self.saved.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value


class Gate:
    def __init__(self, report, name, must=True):
        self.report, self.name, self.must = report, name, must
        self.detail, self.passed = {}, True

    def check(self, label, condition, value=None):
        self.detail[label] = dict(ok=bool(condition), value=value)
        self.passed &= bool(condition)
        return bool(condition)

    def finish(self, error=None, skipped=None):
        if error is not None:
            self.detail['error'] = error
            self.passed = False
        state = 'SKIP' if skipped else ('PASS' if self.passed else 'FAIL')
        if skipped:
            self.detail['skipped'] = skipped
        self.report['gates'][self.name] = dict(result=state, must_pass=self.must, detail=self.detail)
        print(f'{self.name}: {state}', flush=True)


def run_gate(report, name, body, must=True):
    gate = Gate(report, name, must)
    try:
        skipped = body(gate)
        gate.finish(skipped=skipped)
    except Exception as error:  # recorded, never repaired
        gate.finish(f'{type(error).__name__}: {error}'[:1200])


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def sample(app, document, x, y):
    ok, reply = call(app, 'sample_pixel', document_id=document, x=x, y=y)
    assert ok, reply
    return reply['center_rgba32']


def wait_view(app, document, predicate, timeout=60, image_hash=False):
    """Poll `get_view` until `predicate`; `image_hash` asks for `view_image_sha256` (opt-in,
    because it hashes the whole view image)."""
    extra = dict(image_hash=True) if image_hash else {}
    deadline = time.monotonic() + timeout
    while True:
        ok, state = call(app, 'get_view', document_id=document, **extra)
        assert ok, state
        if predicate(state) or time.monotonic() > deadline:
            return state
        time.sleep(0.2)


def inspector(output):
    binary = output / 'author'
    if not binary.exists():
        subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0',
                        str(ROOT / 'scripts/hdr-photos/author.swift'), '-o', str(binary)],
                       check=True, capture_output=True, timeout=600)
    return binary


def inspect(output, path, patches=None):
    command = [str(inspector(output)), 'inspect', str(path)]
    if patches:
        command += ['--patches', str(patches[0]), str(patches[1])]
    return json.loads(subprocess.run(command, check=True, capture_output=True, text=True,
                                     timeout=600).stdout)


# ---------------------------------------------------------------------- gates

def open_gates(report, args, output, oracle):
    columns, rows = oracle['columns'], oracle['rows']
    for name, entry in sorted(oracle['fixtures'].items()):
        def body(gate, name=name, entry=entry):
            path = args.fixtures / name
            with Instance(output, f'open-{name}') as app:
                ok, reply = call(app, 'open_document', path=str(path))
                if not gate.check('open', ok, None if ok else reply):
                    return None
                document = reply['document']
                gate.check('precision', document['precision'] == 'rgba32_float', document['precision'])
                hdr = reply.get('hdr_open') or {}
                gate.check('hdr_open', hdr.get('kind') == entry['kind'] and hdr.get('rendition') == 'hdr'
                           and hdr.get('reference_white') == ('sdr_base' if entry['kind'] == 'gain_map'
                                                              else '203 cd/m2'), hdr)
                ok, info = call(app, 'get_document', document_id=document['id'])
                profile = (info.get('profile') or info.get('color_profile') or {}) if ok else {}
                gate.check('profile', ok, profile if isinstance(profile, str) else
                           {k: profile.get(k) for k in ('name', 'description') if isinstance(profile, dict)})
                values = [sample(app, document['id'], x, y) for x, y in entry['centres']]
                gate.detail['opened'] = [[round(c, 5) for c in v[:3]] for v in values]
                if entry['kind'] == 'gain_map':
                    worst = -math.inf
                    for got, want in zip(values, entry['linear']):
                        for c in range(3):
                            worst = max(worst, abs(got[c] - want[c]) - (0.04 * abs(want[c]) + 0.004))
                    gate.check('authored scene (4 % + 0.004)', worst <= 0, round(worst, 6))
                    seen = inspect(output, path, (columns, rows))
                    aux = seen['auxiliary']['iso'] or seen['auxiliary']['apple']
                    meta = (aux or {}).get('metadata') or {}
                    if meta.get('channels') and aux.get('codes') and seen.get('base_linear'):
                        worst = -math.inf
                        oracle_values = []
                        for got, base, codes in zip(values, seen['base_linear'], aux['codes']):
                            iso = iso_gain(base, codes, meta)
                            oracle_values.append([round(v, 5) for v in iso])
                            for c in range(3):
                                worst = max(worst, abs(got[c] - iso[c]) - (0.01 * abs(iso[c]) + 0.002))
                        gate.detail['iso_oracle'] = oracle_values
                        gate.check('ISO 21496-1 oracle (1 % + 0.002)', worst <= 0, round(worst, 6))
                    else:
                        gate.check('ISO-style metadata present', False, list(meta))
                else:
                    bits = entry['code_bits']
                    lossy = bits == 10
                    worst = -math.inf
                    for got, codes in zip(values, entry['codes']):
                        file_linear = apply(entry['to_file'], got[:3])
                        signal = ([pq_signal(203 * c) for c in file_linear] if entry['kind'] == 'pq'
                                  else hlg_signal(file_linear, entry['file_luma']))
                        scale = (1 << bits) - 1
                        for c in range(3):
                            if lossy:
                                worst = max(worst, abs(signal[c] * 1023 - codes[c]) - 2)
                            else:
                                worst = max(worst, abs(signal[c] - codes[c] / scale) - 1 / 1023)
                    gate.check('authored codes (2 ten-bit codes lossy, 1/1023 lossless)', worst <= 0,
                               round(worst, 6))
            return None
        run_gate(report, f'open {name}', body)


def choice_gate(report, args, output):
    def body(gate):
        with Instance(output, 'choice') as app:
            ok, reply = call(app, 'open_document', path=str(args.fixtures / 'gainmap-ci.heic'),
                             hdr_photo='sdr')
            if gate.check('gain map sdr open', ok, None if ok else reply):
                gate.check('sdr precision', reply['document']['precision'] == 'rgba8',
                           reply['document']['precision'])
                gate.check('sdr hdr_open', (reply.get('hdr_open') or {}).get('rendition') == 'sdr',
                           reply.get('hdr_open'))
            ok, reply = call(app, 'open_document', path=str(args.fixtures / 'pq.heic'), hdr_photo='sdr')
            gate.check('pq sdr refusal', not ok and reply == PQ_SDR, reply)
            ok, reply = call(app, 'open_document', path=str(args.fixtures / 'hlg.avif'), hdr_photo='sdr')
            gate.check('hlg sdr refusal', not ok and reply == HLG_SDR, reply)
            ok, reply = call(app, 'open_document', path=str(args.fixtures / 'pq.avif'), hdr_photo='both')
            gate.check('bad hdr_photo', not ok and reply == BAD_CHOICE, reply)
    run_gate(report, 'choice', body)


def export_gate(report, args, output, oracle):
    def body(gate):
        out = output / 'exports'
        out.mkdir(exist_ok=True)
        with Instance(output, 'export') as app:
            for source, encoding, kind in (('gainmap-ci.heic', 'gain_map', 'gain_map'),
                                           ('pq-cicp.png', 'pq', 'pq')):
                ok, opened = call(app, 'open_document', path=str(args.fixtures / source))
                if not gate.check(f'{source} open', ok, None if ok else opened):
                    continue
                document = opened['document']['id']
                entry = oracle['fixtures'][source]
                before = [sample(app, document, x, y) for x, y in entry['centres']]
                target = out / f'{Path(source).stem}-{encoding}.heic'
                ok, written = call(app, 'export_image', document_id=document, path=str(target),
                                   overwrite=True, settings=dict(format='heic', bit_depth=32,
                                                                 hdr_encoding=encoding))
                if not gate.check(f'{encoding} export', ok, written if not ok else
                                  {k: written.get(k) for k in ('hdr_encoding', 'content_headroom',
                                                               'bit_depth')}):
                    continue
                gate.check(f'{encoding} reply', written.get('hdr_encoding') == encoding
                           and written.get('content_headroom', 0) >= 1, written.get('content_headroom'))
                ok, back = call(app, 'open_document', path=str(target))
                if not gate.check(f'{encoding} reopen', ok, None if ok else back):
                    continue
                gate.check(f'{encoding} reopen kind', (back.get('hdr_open') or {}).get('kind') == kind,
                           back.get('hdr_open'))
                after = [sample(app, back['document']['id'], x, y) for x, y in entry['centres']]
                worst = -math.inf
                for got, want in zip(after, before):
                    for c in range(3):
                        if kind == 'gain_map':
                            worst = max(worst, abs(got[c] - want[c]) - (0.04 * abs(want[c]) + 0.004))
                        else:
                            to2020 = oracle['fixtures']['pq.heic']['to_file']
                            g, w = apply(to2020, got[:3]), apply(to2020, want[:3])
                            worst = max(worst, abs(pq_signal(203 * g[c]) - pq_signal(203 * w[c])) * 1023 - 2)
                gate.detail[f'{encoding} reopened'] = [[round(c, 5) for c in v[:3]] for v in after]
                gate.check(f'{encoding} round trip', worst <= 0, round(worst, 6))
            ok, opened = call(app, 'open_document', path=str(args.fixtures / 'pq.avif'))
            if gate.check('avif source open', ok, None if ok else opened):
                document = opened['document']['id']
                ok, refused = call(app, 'export_image', document_id=document,
                                   path=str(out / 'refused.avif'), overwrite=True,
                                   settings=dict(format='avif', bit_depth=32, hdr_encoding='pq'))
                gate.check('avif refusal', not ok and refused == AVIF_REFUSAL, refused)
                ok, refused = call(app, 'export_image', document_id=document,
                                   path=str(out / 'refused.heic'), overwrite=True,
                                   settings=dict(format='heic', bit_depth=32))
                gate.check('heic 32 without hdr_encoding', not ok and NO_HDR_ENCODING in refused, refused)
    run_gate(report, 'export', body)


def edr_steps(app, gate, document, peak, label=''):
    """HDR Preview on / +1 stop / off, and the view hash before and after."""
    ok, state = call(app, 'set_view', document_id=document, hdr_preview=False)
    gate.check(f'{label}off first', ok, None if ok else state)
    state = wait_view(app, document, lambda s: not s['edr_layer'], image_hash=True)
    before_hash = state['view_image_sha256']
    gate.check(f'{label}edr_headroom', abs(state['edr_headroom'] - 4) < 1e-9, state['edr_headroom'])
    ok, state = call(app, 'set_view', document_id=document, hdr_preview=True)
    state = wait_view(app, document, lambda s: s['hdr_preview_displayed'])
    gate.check(f'{label}displayed', state['hdr_preview_displayed'] and state['edr_layer'],
               {k: state[k] for k in ('hdr_preview', 'hdr_preview_displayed', 'edr_layer',
                                      'edr_content_peak')})
    shown = state['edr_content_peak']
    if peak is not None:
        gate.check(f'{label}peak', shown is not None and abs(shown - peak) <= 0.005 * peak, [shown, peak])
    else:
        gate.check(f'{label}peak above 1', shown is not None and shown > 1, shown)
    ok, state = call(app, 'set_view', document_id=document, exposure=1)
    state = wait_view(app, document, lambda s: s['hdr_preview_displayed'] and s['exposure'] == 1)
    doubled = state['edr_content_peak']
    gate.check(f'{label}+1 stop doubles', doubled is not None and shown is not None
               and abs(doubled - 2 * shown) <= 0.005 * 2 * shown, [doubled, shown])
    ok, state = call(app, 'set_view', document_id=document, exposure=0, hdr_preview=False)
    state = wait_view(app, document, lambda s: not s['edr_layer'] and s['view_image_sha256'] == before_hash,
                      timeout=20, image_hash=True)
    gate.check(f'{label}off restores', not state['edr_layer'] and not state['hdr_preview_displayed']
               and state['view_image_sha256'] == before_hash,
               {k: state[k] for k in ('edr_layer', 'hdr_preview_displayed')})
    return shown


def edr_gate(report, args, output, oracle):
    def body(gate):
        with Instance(output, 'edr') as app:
            entry = oracle['fixtures']['gainmap-ci.heic']
            ok, opened = call(app, 'open_document', path=str(args.fixtures / 'gainmap-ci.heic'))
            if not gate.check('open', ok, None if ok else opened):
                return None
            document = opened['document']['id']
            state = wait_view(app, document, lambda s: s['hdr_preview_displayed'])
            gate.check('engages by default', state['hdr_preview'] and state['hdr_preview_displayed'],
                       state['hdr_preview_displayed'])
            values_on = [sample(app, document, x, y) for x, y in entry['centres']]
            ok, histogram = call(app, 'histogram', document_id=document)
            gate.check('histogram', ok, None if ok else histogram)
            peak = max(histogram['max_rgba'][:3])
            edr_steps(app, gate, document, peak)
            values_off = [sample(app, document, x, y) for x, y in entry['centres']]
            gate.check('readouts are document values', values_on == values_off)
        return None
    run_gate(report, 'edr', body)


def dng_gate(report, args, output):
    def body(gate):
        path = args.fixtures / DNG_NAME
        if not path.is_file():
            return 'the owner DNG is absent on this machine'
        if digest(path) != DNG_SHA256:
            return 'the owner DNG does not match its recorded SHA-256; not read further'
        with Instance(output, 'dng', limit=GIB8) as app:
            ok, opened = call(app, 'open_document', path=str(path), raw={'output_encoding': 'linear_float'})
            if not gate.check('open', ok, None if ok else opened):
                return None
            document = opened['document']['id']
            gate.check('precision', opened['document']['precision'] == 'rgba32_float',
                       opened['document']['precision'])
            ok, histogram = call(app, 'histogram', document_id=document)
            if gate.check('histogram', ok, None if ok else histogram):
                pixels = histogram.get('sampled_pixels') or (opened['document']['width']
                                                             * opened['document']['height'])
                gate.detail['pixels'] = pixels
                gate.detail['above_white_share_rgb'] = [count / pixels
                                                        for count in histogram['above_white_rgba'][:3]]
                gate.detail['max_rgb'] = histogram['max_rgba'][:3]
            state = wait_view(app, document, lambda s: s['hdr_preview_displayed'], timeout=120)
            edr_steps(app, gate, document, None, label='dng ')
        seen = inspect(output, path)
        gate.detail['imageio'] = dict(
            apple_gain_map=seen['auxiliary']['apple'] is not None,
            iso_gain_map=seen['auxiliary']['iso'] is not None,
            maker_apple_33=seen.get('maker_apple_33'), maker_apple_48=seen.get('maker_apple_48'),
            apple_headroom_formula=seen.get('apple_headroom_formula'),
            apple_hdr_gain_map_headroom=(seen['auxiliary']['apple'] or {}).get('hdr_gain_map_headroom'),
            iso_alternate_headroom_log2=((seen['auxiliary']['iso'] or {}).get('metadata') or {})
            .get('AlternateHeadroom'),
            hdr_decode_content_headroom=seen.get('hdr_decode_content_headroom'))
        return None
    run_gate(report, 'dng (real-world)', body, must=False)


# ------------------------------------------------------------------ measuring

def measure(args, output, oracle):
    os.environ['MallocLargeCache'] = '0'
    cases = []
    sizes = {name: entry for name, entry in oracle['admission'].items()}

    def settle(pid, timeout=120):
        last, still, deadline = None, time.monotonic(), time.monotonic() + timeout
        while time.monotonic() < deadline and time.monotonic() - still < 2:
            time.sleep(0.25)
            now, _ = footprint(pid)
            if now != last:
                last, still = now, time.monotonic()

    phases = args.measure_phases.split(',')
    for run in range(1, args.measure + 1):
        for phase in phases:
            with Instance(output, f'measure-baseline-{phase}-{run}', limit=GIB8,
                          headroom='4' if phase == 'edr' else '1') as app:
                call(app, 'resource_usage')
                time.sleep(3)
                pid = app.process.pid
                pre, _ = footprint(pid, reset=True)
                call(app, 'resource_usage')
                settle(pid)
                _, peak = footprint(pid)
                cases.append(dict(case=f'baseline-{phase}', run=run, E=peak - pre))
                print(json.dumps(cases[-1]), flush=True)
        for name, entry in sorted(sizes.items()):
            path = args.fixtures / entry['path']
            pixels = entry['width'] * entry['height']
            if 'export' in phases:
                with Instance(output, f'measure-export-{name}-{run}', limit=GIB8,
                              headroom='1') as app:
                    ok, reply = call(app, 'open_document', path=str(path))
                    document = reply['document']['id']
                    pid = app.process.pid
                    settle(pid)
                    encoding = 'gain_map' if name.startswith('gainmap') else 'pq'
                    target = output / f'measure-export-{name}-{run}.heic'
                    pre, _ = footprint(pid, reset=True)
                    ok, written = call(app, 'export_image', document_id=document, path=str(target),
                                       overwrite=True, settings=dict(format='heic', bit_depth=32,
                                                                     hdr_encoding=encoding))
                    settle(pid)
                    _, peak = footprint(pid)
                    # doc_export::export_plan_for at 32 bits, no resample: the
                    # base 8 B/px, the float term, the encode term, bookkeeping.
                    plan = (8 + 8 + 32 + 48) * pixels + 1048576 + 16 * entry['width'] \
                        + 134217728 + 1048576
                    cases.append(dict(case=f'export-{name}', run=run, ok=ok, E=peak - pre,
                                      pixels=pixels, encoding=encoding, plan=plan,
                                      written=target.stat().st_size if ok else written[:300]))
                    print(json.dumps(cases[-1]), flush=True)
            if 'open' in phases:
                with Instance(output, f'measure-open-{name}-{run}', limit=GIB8, headroom='1') as app:
                    call(app, 'resource_usage')
                    time.sleep(3)
                    pid = app.process.pid
                    pre, _ = footprint(pid, reset=True)
                    ok, reply = call(app, 'open_document', path=str(path))
                    settle(pid)
                    _, peak = footprint(pid)
                    cases.append(dict(case=f'open-{name}', run=run, ok=ok, E=peak - pre, pixels=pixels,
                                      encoded=path.stat().st_size,
                                      precision=reply['document']['precision'] if ok else reply[:300]))
                    print(json.dumps(cases[-1]), flush=True)
            if 'edr' in phases:
                with Instance(output, f'measure-edr-{name}-{run}', limit=GIB8, headroom='4') as app:
                    ok, reply = call(app, 'open_document', path=str(path))
                    document = reply['document']['id']
                    wait_view(app, document, lambda s: s['hdr_preview_displayed'], timeout=120)
                    call(app, 'set_view', document_id=document, hdr_preview=False)
                    wait_view(app, document, lambda s: not s['edr_layer'])
                    pid = app.process.pid
                    settle(pid)
                    pre, _ = footprint(pid, reset=True)
                    call(app, 'set_view', document_id=document, hdr_preview=True)
                    state = wait_view(app, document, lambda s: s['hdr_preview_displayed'], timeout=120)
                    settle(pid)
                    _, peak = footprint(pid)
                    cases.append(dict(case=f'edr-{name}', run=run, E=peak - pre, pixels=pixels,
                                      displayed=state['hdr_preview_displayed']))
                    print(json.dumps(cases[-1]), flush=True)
    return cases


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--measure', type=int, metavar='RUNS')
    parser.add_argument('--measure-phases', default='open,edr,export')
    args = parser.parse_args()
    args.fixtures = args.fixtures.expanduser()
    output = args.output or Path(tempfile.mkdtemp(prefix='rz-hdr-photos-'))
    output.mkdir(parents=True, exist_ok=True)
    oracle = json.loads((args.fixtures / 'oracle.json').read_text())
    provenance = json.loads((args.fixtures / 'provenance.json').read_text())
    report = dict(drive=Path(__file__).name, fixtures=str(args.fixtures), gates={},
                  drive_sha256=digest(Path(__file__)))
    stale = [name for name, entry in provenance['files'].items()
             if (args.fixtures / name).is_file() and digest(args.fixtures / name) != entry['sha256']]
    if stale:
        raise SystemExit(f'fixtures differ from provenance.json: {stale}')
    if args.measure:
        report['measurements'] = measure(args, output, oracle)
    else:
        open_gates(report, args, output, oracle)
        choice_gate(report, args, output)
        export_gate(report, args, output, oracle)
        edr_gate(report, args, output, oracle)
        dng_gate(report, args, output)
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    failed = [name for name, gate in report['gates'].items()
              if gate['result'] == 'FAIL' and gate['must_pass']]
    print(json.dumps(dict(receipt=str(output / 'receipt.json'), failed=failed)))
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
