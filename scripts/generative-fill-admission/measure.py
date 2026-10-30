#!/usr/bin/env python3
"""The Generative Fill and Generative Expand admission drive (M3b phases 1 and 2), on a built
Release app.

`generative_fill` runs ONE admitted job (`GenerativeFillRun.start` through `DocumentJobs.start`):
the core's input (`dr_*_gen_fill_input`, the full-composite plate included), the two PNG encodes
and the multipart body, the provider's request, its JSON and base64 reply, the PNG decode and the
core's apply (`dr_*_gen_fill_apply`), at P = the reply's `planned_bytes` = the core's plan
(`dr_*_gen_fill_plan` for three variations) + `GenerativeFillRun.hostBytesPerPixel` x n +
`hostBytesPerCell` x the native cells x the variations. The gates are FIXED here before the drive runs; a miss is recorded in
`receipt.json` and fails its gate, and nothing is widened after the fact. A miss raises
`hostBytesPerCell`, then `hostBytesPerPixel`.

The provider is a stub this file owns (a `ThreadingHTTPServer` on 127.0.0.1:0, DR_OPENAI_BASE_URL
pointed at it) that answers `n` (three, the default variations) SEEDED-NOISE RGB PNGs at the
requested size (stdlib `zlib`), so the reply, its base64 text, its parse, its decodes and the
core's stored variations reach their real size. No case reaches the
network or a real key: each app's DR_CLOUD_DIR (`HostApp` puts it in the app's own folder) is
seeded with `openai_api_key` = `dummy-openai-key`, OpenAI's consent and the agent switch on.

Every case runs in its own fresh owned app, on its own scratch MCP socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with
the same document and selection, then `resource_usage` instead of the measured call), `--runs`
times. The document is `new_document` 6000 x 4000 filled #808080 at the case's depth, then
`apply_filter` add_noise (amount 1, seed 665) in BOTH the case and its twin, so the image PNG is
incompressible; the selection is `select_rect` 2000 x 1500 in the middle. E is
`ri_interval_max_phys_footprint` - pre around the one call, read once the footprint has been
still for 2 s.

- **rgba8** / **rgba16**: layer 0, prompt "a red boat", three variations.
- **cmyk8**: the rgba8 document converted with `convert_mode` cmyk (the Generic CMYK profile,
  8 bits) after the noise, in the case and its twin; then the same call.
- **expand8** / **expand16** / **expandcmyk8** (Generative Expand, phase 2 part 2): `new_document`
  4800 x 3200 #808080 at the case's depth, the same add_noise (and `convert_mode` cmyk for
  expandcmyk8), then `canvas_size` 6000 x 4000 centred (24 MP, a transparent frame), in the case
  and its twin, with NO selection; then `generative_expand` layer 0 prompt "a red boat". Its job
  is `GenerativeFillRun.start` over `.transparency`: the core's coverage
  (`dr_*_gen_expand_coverage`), then the same input, request and apply, at P = the core's plan at
  the provider's largest native size + the same host terms.
- Gate per case: P >= ceil(1.25 x max E) over its runs, every run ok and completed. The baseline
  twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/generative-fill-admission/measure.py --app <Doctored.app>
--output <folder outside the repository> [--runs 3]
[--cases rgba8,rgba16,cmyk8,expand8,expand16,expandcmyk8]. Exit 0 only when
every gate passes. Imports `call` and `footprint` from `scripts/host-still-admission/verify.py`
and `HostApp` from `scripts/host-source-probe/support.py`; memory, not wall-clock: run it only
when no other lane is building.
"""
import argparse
import base64
import hashlib
import http.server
import json
import math
import os
from pathlib import Path
import random
import struct
import sys
import tempfile
import threading
import time
import zlib

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-still-admission'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))
from verify import call, footprint  # noqa: E402
import support  # noqa: E402

MINIMUM_MAC_LIMIT = 8589934592
WIDTH, HEIGHT = 6000, 4000
SELECTION = dict(x=2000, y=1250, width=2000, height=1500)
PROMPT = 'a red boat'
KEY = 'dummy-openai-key'
CASES = {'rgba8': 8, 'rgba16': 16, 'cmyk8': 8, 'expand8': 8, 'expand16': 16, 'expandcmyk8': 8}
CMYK_CASES = {'cmyk8', 'expandcmyk8'}
EXPAND_CASES = {'expand8', 'expand16', 'expandcmyk8'}
# The expand cases' picture before Canvas Size grows it to WIDTH x HEIGHT.
EXPAND_PICTURE = (4800, 3200)
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')


def noise_png(width, height, seed=665):
    """A seeded-noise RGB PNG (filter 0 rows), incompressible by design."""
    rows = bytearray()
    generator = random.Random(seed)
    for _ in range(height):
        rows.append(0)
        rows.extend(generator.randbytes(width * 3))

    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))

    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(bytes(rows), 1)) + chunk(b'IEND', b''))


class Stub:
    """The owned OpenAI stub: `n` noise PNGs at the requested size, every time."""

    def __init__(self):
        self.requests = 0
        self.cache = {}
        stub = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read(int(self.headers.get('Content-Length') or 0))
                stub.requests += 1
                size = b'1024x1024'
                marker = body.find(b'name="size"')
                if marker >= 0:
                    start = body.find(b'\r\n\r\n', marker) + 4
                    size = body[start:body.find(b'\r\n', start)]
                w, h = (int(v) for v in size.decode().split('x'))
                n = 1
                marker = body.find(b'name="n"')
                if marker >= 0:
                    start = body.find(b'\r\n\r\n', marker) + 4
                    n = int(body[start:body.find(b'\r\n', start)].decode())
                data = []
                for i in range(n):
                    if (w, h, i) not in stub.cache:
                        stub.cache[(w, h, i)] = base64.b64encode(noise_png(w, h, 665 + i)).decode()
                    data.append({'b64_json': stub.cache[(w, h, i)]})
                reply = json.dumps({'created': 0, 'data': data}).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(reply)))
                self.end_headers()
                self.wfile.write(reply)

            def log_message(self, *_):
                pass

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def base(self):
        return f'http://127.0.0.1:{self.server.server_address[1]}/v1'

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.server.shutdown()
        self.server.server_close()


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def settle(pid, timeout=240):
    """Waits until the footprint has been still for 2 s; answers it."""
    last, still_since, deadline = None, time.monotonic(), time.monotonic() + timeout
    while time.monotonic() < deadline and time.monotonic() - still_since < 2:
        time.sleep(0.25)
        now, _ = footprint(pid)
        if now != last:
            last, still_since = now, time.monotonic()
    return last


def seed_cloud(folder):
    """The app's DR_CLOUD_DIR (`HostApp` puts it at <folder>/cloud): key, consent, switch on."""
    cloud = folder / support.HostApp.PRESET_DIRECTORIES['DR_CLOUD_DIR']
    cloud.mkdir(parents=True, exist_ok=True)
    (cloud / 'openai_api_key').write_text(KEY + '\n')
    (cloud / 'cloud.json').write_text(json.dumps({
        'consents': {'openai': {'terms_version': 1, 'accepted_at': '2026-10-15T00:00:00Z'}},
        'agent_cloud_tools': True}))


def measure_case(output, executable, label, case, baseline):
    record = dict(case=case, label=label, baseline=baseline)
    folder = output / f'app-{label}'
    seed_cloud(folder)
    app = support.HostApp(folder, output / f'recovery-{label}', executable)
    with app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        expand = case in EXPAND_CASES
        width, height = EXPAND_PICTURE if expand else (WIDTH, HEIGHT)
        ok, made = call(app, 'new_document', width=width, height=height, depth=CASES[case],
                        background='#808080')
        assert ok, made
        document = made['document']['id'] if 'document' in made else made['id']
        ok, noised = call(app, 'apply_filter', document_id=document, layer=0, filter='add_noise',
                          amount=1, seed=665)
        assert ok, noised
        if case in CMYK_CASES:
            ok, converted = call(app, 'convert_mode', document_id=document, mode='cmyk',
                                 profile='file', path=str(GENERIC))
            assert ok and converted.get('depth') == 8, converted
        if expand:
            ok, grown = call(app, 'canvas_size', document_id=document, width=WIDTH, height=HEIGHT,
                             anchor='center')
            assert ok, grown
        else:
            ok, selected = call(app, 'select_rect', document_id=document, **SELECTION)
            assert ok, selected
        pid = app.process.pid
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        if baseline:
            ok, reply = call(app, 'resource_usage')
        else:
            tool = 'generative_expand' if expand else 'generative_fill'
            ok, reply = call(app, tool, document_id=document, layer=0, prompt=PROMPT)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            if ok:
                for key in ('state', 'changed', 'planned_bytes', 'native_size', 'crop', 'output_layer',
                            'variations'):
                    record[key] = reply.get(key)
            else:
                record['refusal'] = reply[:800]
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path,
                        default=Path(os.environ['DR_VERIFY_APP']) if os.environ.get('DR_VERIFY_APP')
                        else None)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--cases', default=','.join(CASES))
    arguments = parser.parse_args()
    if arguments.app is None:
        parser.error('name the Release app with --app or DR_VERIFY_APP')
    selected = [case.strip() for case in arguments.cases.split(',') if case.strip()]
    unknown = [case for case in selected if case not in CASES]
    if unknown:
        parser.error(f'unknown case(s): {",".join(unknown)}')
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-generative-fill-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(MINIMUM_MAC_LIMIT)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)), runs=arguments.runs,
                  working_limit_bytes=MINIMUM_MAC_LIMIT, selection=SELECTION, records=[], gates={})
    previous = os.environ.get('DR_OPENAI_BASE_URL')
    with Stub() as stub:
        os.environ['DR_OPENAI_BASE_URL'] = stub.base
        try:
            for run in range(1, arguments.runs + 1):
                for case in selected:
                    for baseline in (True, False):
                        label = f'{case}-{"baseline" if baseline else "call"}-{run}'
                        try:
                            record = measure_case(output, executable, label, case, baseline)
                        except Exception as error:          # Recorded, never repaired.
                            record = dict(case=case, label=label, baseline=baseline, ok=False,
                                          error=f'{type(error).__name__}: {error}'[:800])
                        record['run'] = run
                        report['records'].append(record)
                        print(json.dumps(record), flush=True)
        finally:
            if previous is None:
                os.environ.pop('DR_OPENAI_BASE_URL', None)
            else:
                os.environ['DR_OPENAI_BASE_URL'] = previous
        report['stub_requests'] = stub.requests
    table = {}
    passed = True
    for case in selected:
        runs = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        twins = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(runs) and all(r.get('ok') and r.get('state') == 'completed' and r.get('changed') is True
                                and r.get('variations') == 3
                                and isinstance(r.get('planned_bytes'), int) for r in runs)
        limit_ok = all(r.get('working_limit_bytes') == MINIMUM_MAC_LIMIT for r in runs + twins)
        plan = min((r.get('planned_bytes', 0) for r in runs), default=0) if ok else 0
        max_e = max((r.get('E', 0) for r in runs), default=0)
        need = math.ceil(1.25 * max_e)
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
                   planned=[r.get('planned_bytes') for r in runs], plan=plan, need=need,
                   ratio=(plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and plan >= need)
        passed &= row['passed']
        table[case] = row
        report['gates'][case] = dict(passed=row['passed'])
    report['table'] = table
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        print(f'{case:8} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} baseline {row["baseline_E"]} '
              f'{"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
