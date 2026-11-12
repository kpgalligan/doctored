#!/usr/bin/env python3
"""The MCP `liquify` on an 8-bit document under each evaluation policy, on an
owned Debug instance (or the app `DR_VERIFY_APP` names).

The 8-bit Liquify is the one resampling edit whose host route changed when the
mesh warps, the selected-pixels transforms and Liquify moved to linear light
(11E ADR, D7): the host now hands the captured document to the core
(`dr_doc_filter_op`), so the policy decides the domain. The core tests prove
the kernels (`core/tests/evaluation_storage_resample.rs`); this drive proves
the host passes the document, from the tool a recorded Action replays.

The oracle is the core test's FEASIBILITY check, written again here from the
analytic sRGB curve: every pixel of an opaque picture of two colours A and B,
however a resample mixed it, is `w·A + (1 − w)·B` with ONE w for all three
channels in the domain the mix ran in. Each stored code ± (GATE + ½) bounds w
to an interval in a model's domain (a code at either end also stands for every
value the clamp folds onto it); the model holds where the three intervals
meet. Gates, fixed before any run:

- L1 encoded: every pixel opaque and explained by the CODE model; at least
  MIN_MIXED pixels mix both colours and at least MIN_TELLING of them refuse
  the linear model.
- L2 linear_light (`set_evaluation` first): every pixel explained by the
  LINEAR model; at least MIN_MIXED mixed and MIN_TELLING refuse the code model.

A single opaque Normal layer projects to its own stored codes under either
policy (decode then encode returns a byte), so `render` reads the layer.

    python3 scripts/verify_linear_light_warps.py
"""
import base64
import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import app_under_test  # noqa: E402
from mcp_client import Client, remove_scratch, scratch_socket, wait_ready  # noqa: E402
from pixels import png_rgba, write_png8  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402

# ---- fixed before anything is measured -------------------------------------
GATE = 1            # byte codes: linear_oracle's byte gate
MIN_MIXED = 40
MIN_TELLING = 10

A = (30, 60, 200)
B = (220, 180, 40)
WIDTH, HEIGHT = 64, 24
STROKES = [{"tool": "forward_warp", "size": 30, "strength": 0.5, "density": 1,
            "points": [[16, 12], [44, 12]]}]


def decode(x):
    """The analytic sRGB decode."""
    return x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4


def codes(x):
    return x


def stripes():
    """4-pixel bands of A and B, opaque, row 0 first."""
    out = bytearray()
    for _ in range(HEIGHT):
        for x in range(WIDTH):
            c = A if (x // 4) % 2 == 0 else B
            out += bytes((c[0], c[1], c[2], 255))
    return bytes(out)


def w_interval(p, a, b, f):
    slack = GATE + 0.5
    low, high = p - slack, p + slack
    low = float('-inf') if low <= 0 else f(low / 255)
    high = float('inf') if high >= 255 else f(high / 255)
    fa, fb = f(a / 255), f(b / 255)
    w0, w1 = (low - fb) / (fa - fb), (high - fb) / (fa - fb)
    return (w0, w1) if fa > fb else (w1, w0)


def feasible(pixel, f):
    low, high = float('-inf'), float('inf')
    for c in range(3):
        l, h = w_interval(pixel[c], A[c], B[c], f)
        low, high = max(low, l), min(high, h)
    return low <= high


def check(pixels, right, wrong, label):
    mixed = telling = 0
    for i, p in enumerate(pixels):
        assert p[3] == 255, f'{label}: pixel {i} {p} lost alpha'
        assert feasible(p, right), f'{label}: pixel {i} {p} is not one mix'
        near = lambda c: all(abs(p[k] - c[k]) <= GATE for k in range(3))
        if not near(A) and not near(B):
            mixed += 1
            telling += not feasible(p, wrong)
    assert mixed >= MIN_MIXED, f'{label}: only {mixed} mixed pixels'
    assert telling >= MIN_TELLING, f'{label}: only {telling} pixels tell the domains apart'
    print(f'{label}: PASS ({mixed} mixed, {telling} refuse the other domain)', flush=True)


def liquified(client, path, policy):
    document = client.open(path)
    state = client.call('get_evaluation', document_id=document)
    assert state['policy'] == 'encoded', state
    if policy == 'linear_light':
        reply = client.call('set_evaluation', document_id=document, policy='linear_light')
        assert reply['policy'] == 'linear_light', reply
    reply = client.call('liquify', document_id=document, layer=0, strokes=STROKES)
    assert reply['changed'] is True, reply
    result = client.rpc('tools/call', {'name': 'render',
                                       'arguments': {'document_id': document, 'max_side': 4096}})
    assert not result.get('isError'), result
    data = next(item['data'] for item in result['content'] if item['type'] == 'image')
    image = png_rgba(base64.b64decode(data))
    assert (image['width'], image['height'], image['depth']) == (WIDTH, HEIGHT, 8), image['width']
    s = image['samples']
    return [tuple(s[i:i + 4]) for i in range(0, len(s), 4)]


def main():
    exe = app_under_test.default_app_executable()
    app_under_test.require_fresh_app(exe)
    folder = Path(tempfile.mkdtemp(prefix='linear-warps-'))
    # One file per gate: opening a path that is already open answers the
    # document already open, which L1 has liquified.
    pictures = {policy: folder / f'stripes-{policy}.png' for policy in ('encoded', 'linear_light')}
    for path in pictures.values():
        write_png8(path, WIDTH, HEIGHT, stripes())
    socket = scratch_socket()
    log = open(folder / 'app.log', 'wb')
    app = subprocess.Popen(
        [str(exe), '-ApplePersistenceIgnoreState', 'YES'],
        stdout=log, stderr=subprocess.STDOUT,
        env=dict(os.environ, DR_BACKGROUND='1', DR_AGENT_SOCKET=str(socket),
                 ANTHROPIC_API_KEY='dummy', DR_WORKING_LIMIT_BYTES='2147483648',
                 DR_AUTOMATION_ALERTS='record', DR_EDR_HEADROOM='1',
                 DR_ACTIONS_DIR=str(folder / 'actions'), DR_RECOVERY_DIR=str(folder / 'recovery'),
                 **preset_environment(folder)))
    try:
        client = Client(socket)
        wait_ready(client, app, 60, str(folder / 'app.log'))
        check(liquified(client, pictures['encoded'], 'encoded'), codes, decode, 'L1 encoded')
        check(liquified(client, pictures['linear_light'], 'linear_light'), decode, codes,
              'L2 linear_light')
    finally:
        app.terminate()
        try:
            app.wait(timeout=20)
        except subprocess.TimeoutExpired:
            app.kill()
        log.close()
        remove_scratch(socket)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
