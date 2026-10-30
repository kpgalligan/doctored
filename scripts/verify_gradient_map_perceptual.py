#!/usr/bin/env python3
"""The MCP drive on an owned Debug app: the Gradient Map's
`method: "perceptual"` (the Perceptual method: the Classic law over
the stops' Oklab values) through the surfaces a user reaches it by. No
Photoshop.

The scene is `verify_gradient_map_classic.py`'s (its `Drive`, eight gray
patches under a Gradient Map layer "Map"); PERCEPTUAL is its CLASSIC map with
`method: "perceptual"`: four stops, Smoothness 0.5, distinct colour
midpoints and one opacity midpoint on a half-opaque stop.

The oracle, written here from `gradient_map_678_take.py`'s PERC-OK (never
read off the core): each stop's sRGB colour is decoded and taken to Oklab
with Ottosson's published M1 / M2; the classic drive's `mapped` (f remapped
through stop i's midpoint, (1 − k)·linear + k·Catmull-Rom clamped to the
bracketing stops per channel, the opacity linear through stop i's opacity
midpoint) runs over those Oklab values; the result goes back through his
published inverses and the sRGB encode, and the patch blends toward it by
the opacity. t is the patch's Rec. 709 luma (8 and 16 bits) or the
sRGB-encoded clamped value of a 32-bit patch (display-mapped, its result
decoded). Bounds: 1 code at 8 bits, 3 16-bit units at 16, 2e-3 at 32.

Gates (a failed gate IS the receipt):
- **P1 Perceptual at 8, 16 and 32 bits.** `add_adjustment_layer`
  gradient_map PERCEPTUAL is ONE undo step, `get_document` reports `method`
  perceptual, the Smoothness and the midpoints, and every patch meets the
  oracle.
- **P2 CMYK refuses it.** The same map with ink stops on a 16-bit CMYK
  document refuses in-band with "The gradient map's Perceptual method is
  unavailable in a CMYK document" and adds no history step.
- **P3 `.rz`.** `save_copy` rz of the 8-bit and the 16-bit scene writes word
  111 right after the identity word and reopens with the same params and the
  same patches.
- **P4 Actions.** `start_recording` → `add_adjustment_layer` PERCEPTUAL →
  `stop_recording` → `run_action` on a second, identical 8-bit scene reports
  the same params and the same patches.

Launch: `phase5_support.App` (scratch socket, dummy key, background, every
preset folder private); only that instance is ever stopped.
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402
import verify_gradient_map_classic as C  # noqa: E402

# ---- the expectations, fixed here BEFORE anything is measured -------------
BOUND = {8: 1.0, 16: 3.0, 32: 2e-3}
SENTENCE = "The gradient map's Perceptual method is unavailable in a CMYK document"

# Ottosson (2020): M1, M2 and his published inverses.
M1 = [[0.4122214708, 0.5363325363, 0.0514459929],
      [0.2119034982, 0.6806995451, 0.1073969566],
      [0.0883024619, 0.2817188376, 0.6299787005]]
M2 = [[0.2104542553, 0.7936177850, -0.0040720468],
      [1.9779984951, -2.4285922050, 0.4505937099],
      [0.0259040371, 0.7827717662, -0.8086757660]]
M2_INV = [[1.0, 0.3963377774, 0.2158037573],
          [1.0, -0.1055613458, -0.0638541728],
          [1.0, -0.0894841775, -1.2914855480]]
M1_INV = [[4.0767416621, -3.3077115913, 0.2309699292],
          [-1.2684380046, 2.6097574011, -0.3413193965],
          [-0.0041960863, -0.7034186147, 1.7076147010]]


def mul(m, v):
    return [sum(m[r][c] * v[c] for c in range(3)) for r in range(3)]


def cbrt(v):
    return v ** (1.0 / 3.0) if v >= 0 else -((-v) ** (1.0 / 3.0))


def to_oklab(srgb):
    return mul(M2, [cbrt(v) for v in mul(M1, [C.decode(c) for c in srgb])])


def from_oklab(lab):
    return [C.encode(v) for v in mul(M1_INV, [v ** 3 for v in mul(M2_INV, lab)])]


def perceptual_params(cmyk=False):
    params = C.classic_params(cmyk)
    params['method'] = 'perceptual'
    return params


def want_rgb(pixel, depth, stops, k):
    """The oracle's output for a sampled patch (unit values, module doc)."""
    values = [to_oklab(C.unit(s[1])) for s in stops]
    if depth == 32:
        e = [C.encode(v) for v in pixel]
        lab, opacity = C.mapped(stops, values, min(max(C.luma(e), 0.0), 1.0), k)
        return [C.decode(v) for v in C.blended(e, from_oklab(lab), opacity)]
    lab, opacity = C.mapped(stops, values, min(max(C.luma(pixel), 0.0), 1.0), k)
    return C.blended(pixel, from_oklab(lab), opacity)


def reports_perceptual(params):
    return (params.get('method') == 'perceptual'
            and C.reports_classic(dict(params, method='classic')))


def run(drive, gates):
    folder = drive.folder
    scenes = {}
    for depth in (8, 16, 32):
        label = f'RGB{depth}'
        doc = drive.scene(depth)
        before = drive.samples(doc, depth)
        steps = drive.add_map(doc, perceptual_params())
        after = drive.samples(doc, depth)
        want = [want_rgb(p, depth, C.STOPS, C.SMOOTHNESS) for p in before]
        err = C.worst(after, want, depth, False)
        classic = [C.want_rgb(p, depth, C.STOPS, C.SMOOTHNESS) for p in before]
        params = drive.map_params(doc)
        C.gate(gates, f'P1 {label}: one step, reported, the Oklab oracle',
               steps == 1 and reports_perceptual(params) and err <= BOUND[depth],
               dict(steps=steps, params=params, worst=err, bound=BOUND[depth],
                    from_classic=C.worst(after, classic, depth, False), before=before,
                    after=after, want=want))
        scenes[label] = (doc, depth, after, params)

    # P2: CMYK refuses Perceptual by name, no history step.
    doc = drive.scene(16, cmyk=True)
    before = drive.undo_count(doc)
    message = drive.app.client.call('add_adjustment_layer', fail=True, document_id=doc,
                                    name='Map', op='gradient_map',
                                    params=perceptual_params(cmyk=True))
    C.gate(gates, 'P2 CMYK refuses perceptual in-band by name, no step',
           SENTENCE in str(message) and drive.undo_count(doc) == before,
           dict(message=message, steps=drive.undo_count(doc) - before))
    drive.call('close_document', document_id=doc, discard_changes=True)

    # P3: .rz.
    for label in ('RGB8', 'RGB16'):
        doc, depth, after, params = scenes[label]
        path = folder / f'perceptual-{label}.rz'
        drive.call('save_copy', document_id=doc, path=str(path), format='rz')
        data = path.read_bytes()
        words = [struct.unpack('<I', data[at:at + 4])[0] for at in (4, 8)]
        back = drive.call('open_document', path=str(path))['document']['id']
        again = drive.samples(back, depth)
        C.gate(gates, f'P3 {label}: word {C.WORD}, the same params and patches after .rz',
               words[1] == C.WORD and drive.map_params(back) == params and again == after,
               dict(words=words, params=drive.map_params(back), samples=again))
        drive.call('close_document', document_id=back, discard_changes=True)

    # P4: Actions.
    first = drive.scene(8)
    drive.call('start_recording', discard=True)
    drive.add_map(first, perceptual_params())
    stopped = drive.call('stop_recording', name='gm678-perceptual')
    recorded = drive.samples(first, 8)
    second = drive.scene(8)
    replay = drive.call('run_action', document_id=second, name='gm678-perceptual')
    replayed = drive.samples(second, 8)
    C.gate(gates, 'P4 the recorded Action replays the Perceptual map',
           reports_perceptual(drive.map_params(second)) and replayed == recorded,
           dict(stopped=stopped, replay=replay, params=drive.map_params(second),
                recorded=recorded, replayed=replayed))
    for doc in (first, second):
        drive.call('close_document', document_id=doc, discard_changes=True)
    for doc, *_ in scenes.values():
        drive.call('close_document', document_id=doc, discard_changes=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', type=Path, default=None,
                        help='a fresh folder (default: build/verify-gradient-map-perceptual/<time>)')
    arguments = parser.parse_args()
    output = arguments.output or (ROOT / 'build' / 'verify-gradient-map-perceptual'
                                  / time.strftime('%Y%m%d-%H%M%S'))
    output = output.expanduser().resolve()
    assert not output.exists() or not any(output.iterdir()), 'use a fresh output folder'
    output.mkdir(parents=True, exist_ok=True)
    gates = {}
    with App(output / 'app', output / 'recovery', interval=600) as app:
        run(C.Drive(app, output), gates)
    (output / 'receipt.json').write_text(json.dumps(gates, indent=2, default=str) + '\n')
    failed = [name for name, g in gates.items() if not g['passed']]
    print('verify_gradient_map_perceptual: ' + ('PASS' if not failed else f'FAIL {failed}'))
    raise SystemExit(0 if not failed else 1)


if __name__ == '__main__':
    main()
