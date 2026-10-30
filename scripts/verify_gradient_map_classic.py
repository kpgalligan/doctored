#!/usr/bin/env python3
"""The MCP drive on an owned Debug app: the Gradient Map's
`method: "classic"` (the Classic method: Smoothness, the bracket
bound, stop midpoints) through the surfaces a user reaches it by. No
Photoshop.

The scene, fixed before the run: a W x H document, a "Ramp" layer of eight
gray patches (GRAYS, PATCH px wide, filled through `fill_selection`), then a
Gradient Map adjustment layer "Map". CLASSIC is a four-stop map with
Smoothness 0.5, distinct colour midpoints and one opacity midpoint on a
half-opaque stop (our model: stop i's midpoint governs segment i..i + 1);
LEGACY is the same stops with no `method` and no midpoint or Smoothness key.

The oracle, written here from `gradient_map_678_take.py`'s declared law
(never read off the core): t is the patch's Rec. 709 luma (8 and 16 bits),
the sRGB-encoded clamped value of a 32-bit patch (the map is display-mapped
there and its result decoded), or a CMYK patch's ink tone luma709(1 − c, 1 −
m, 1 − y)·(1 − k); f, remapped through stop i's midpoint (`pwl.pre`), gives
(1 − k)·linear + k·Catmull-Rom clamped to the bracketing stops per channel;
the opacity is linear through stop i's opacity midpoint and blends the patch
toward the mapped colour. Bounds: 1 code at 8 bits, 3 16-bit units at 16, 2e-3
at 32, 3 ink16 units on CMYK.

Gates (a failed gate IS the receipt):
- **G1 Classic at 8, 16 and 32 bits and in CMYK.** `add_adjustment_layer`
  gradient_map CLASSIC (CMYK: the ink stops) is ONE undo step, `get_document`
  reports `method` classic, the Smoothness and the midpoints, and every patch
  meets the oracle.
- **G2 the refusal.** The CLASSIC stops with `smoothness` but no `method`
  refuse in-band ("need method \"classic\"") and add no history step.
- **G3 `.rz`.** `save_copy` rz of the 8-bit and the 16-bit scene writes word
  111 right after the identity word and reopens with the same params and the
  same patches.
- **G4 Actions.** `start_recording` → `add_adjustment_layer` CLASSIC →
  `stop_recording` → `run_action` on a second, identical 8-bit scene reports
  the same params and the same patches.
- **G5 CMYK `grdm`.** `export_psd` layers of the CMYK scene, reopened
  (`psd_mode` layers), reports `method` classic with the same Smoothness and
  midpoints, and its patches meet the oracle of the stops it reopened with
  (the file quantizes a location to 1/4096 and an opacity to 1/255; the
  patches under the map read with it hidden) within 3 ink16 units.
- **G6 legacy.** LEGACY at 8 bits reports no `method` and meets the oracle at
  Smoothness 0 with every midpoint 0.5 (the linear map, unchanged).

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

# ---- the expectations, fixed here BEFORE anything is measured -------------
PATCH = 6
GRAYS = [16, 48, 80, 112, 144, 176, 208, 240]
W, H = PATCH * len(GRAYS), 8
SMOOTHNESS = 0.5
# (position, colour, opacity, midpoint, opacity midpoint)
STOPS = [(0.0, '#28c8dc', 1.0, 0.7, 0.5), (0.3, '#78501e', 1.0, 0.2, 0.3),
         (0.6, '#3c965a', 0.5, 0.6, 0.5), (1.0, '#fa143c', 1.0, 0.5, 0.5)]
INKS = [[0.0, 0.0, 0.0, 1.0], [0.8, 0.1, 0.0, 0.2], [0.0, 0.9, 0.7, 0.0], [0.0, 0.0, 0.0, 0.0]]
BOUND = {8: 1.0, 16: 3.0, 32: 2e-3, 'cmyk': 3.0}
REFUSAL = 'need method "classic"'
WORD = 111


def stop_json(stop, color=None, ink=None, midpoints=True):
    out = dict(position=stop[0], opacity=stop[2])
    if ink is not None:
        out['ink'] = ink
    else:
        out['color'] = color or stop[1]
    if midpoints and stop[3] != 0.5:
        out['midpoint'] = stop[3]
    if midpoints and stop[4] != 0.5:
        out['opacity_midpoint'] = stop[4]
    return out


def classic_params(cmyk=False):
    stops = [stop_json(s, ink=INKS[i] if cmyk else None) for i, s in enumerate(STOPS)]
    return dict(method='classic', dither=False,
                gradient=dict(smoothness=SMOOTHNESS, stops=stops))


def legacy_params():
    return dict(dither=False, gradient=dict(stops=[stop_json(s, midpoints=False) for s in STOPS]))


# ---- the oracle -----------------------------------------------------------

def unit(hex_):
    return [int(hex_[i:i + 2], 16) / 255.0 for i in (1, 3, 5)]


def remap(f, m):
    if m == 0.5:
        return f
    return f / (2 * m) if f <= m else 0.5 + (f - m) / (2 * (1 - m))


def mapped(stops, values, t, k):
    """(colour, opacity) of the declared Classic law at t (module doc)."""
    pos = [s[0] for s in stops]
    n = len(stops)
    if t <= pos[0]:
        return values[0], stops[0][2]
    if t >= pos[-1]:
        return values[-1], stops[-1][2]
    i = max(j for j in range(n - 1) if pos[j] <= t)
    f0 = (t - pos[i]) / (pos[i + 1] - pos[i])
    f, fo = remap(f0, stops[i][3]), remap(f0, stops[i][4])
    a, b = values[i], values[i + 1]
    before = values[i - 1] if i > 0 else a
    after = values[i + 2] if i + 2 < n else b
    f2, f3 = f * f, f * f * f
    out = []
    for c in range(len(a)):
        cr = ((2 * f3 - 3 * f2 + 1) * a[c] + (f3 - 2 * f2 + f) * (b[c] - before[c]) / 2
              + (-2 * f3 + 3 * f2) * b[c] + (f3 - f2) * (after[c] - a[c]) / 2)
        cr = min(max(cr, min(a[c], b[c])), max(a[c], b[c]))
        lin = a[c] + (b[c] - a[c]) * f
        out.append((1 - k) * lin + k * cr)
    return out, stops[i][2] + (stops[i + 1][2] - stops[i][2]) * fo


def blended(pixel, colour, opacity):
    o = min(max(opacity, 0.0), 1.0)
    return [p + (c - p) * o for p, c in zip(pixel, colour)]


def encode(v):
    v = min(max(v, 0.0), 1.0)
    return 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055


def decode(e):
    return e / 12.92 if e <= 0.04045 else ((e + 0.055) / 1.055) ** 2.4


def luma(rgb):
    return 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]


def want_rgb(pixel, depth, stops, k):
    """The oracle's output for a sampled patch (unit values)."""
    values = [unit(s[1]) for s in stops]
    if depth == 32:
        e = [encode(v) for v in pixel]
        colour, opacity = mapped(stops, values, min(max(luma(e), 0.0), 1.0), k)
        return [decode(v) for v in blended(e, colour, opacity)]
    colour, opacity = mapped(stops, values, min(max(luma(pixel), 0.0), 1.0), k)
    return blended(pixel, colour, opacity)


def want_ink(ink, k, stops=STOPS, inks=INKS):
    t = luma([1 - ink[0], 1 - ink[1], 1 - ink[2]]) * (1 - ink[3])
    colour, opacity = mapped(stops, inks, min(max(t, 0.0), 1.0), k)
    return blended(ink, colour, opacity)


def stops_of(params):
    """(stops, inks, smoothness) of reported CMYK params, in the oracle's
    shape: the numbers a file round trip quantized (location to 1/4096,
    opacity to 1/255), read back rather than assumed."""
    gradient = params.get('gradient') or {}
    listed = sorted(gradient.get('stops') or [], key=lambda s: s.get('position', 0))
    stops = [(s.get('position', 0), None, s.get('opacity', 1), s.get('midpoint', 0.5),
              s.get('opacity_midpoint', 0.5)) for s in listed]
    return stops, [s['ink'] for s in listed], gradient.get('smoothness', 1)


# ---- the drive ------------------------------------------------------------

class Drive:
    def __init__(self, app, folder):
        self.app, self.folder = app, folder

    def call(self, tool, **arguments):
        return self.app.call(tool, **arguments)

    def rows(self, doc):
        return self.call('get_document', document_id=doc)['layers']

    def index_of(self, doc, name):
        return next(r['index'] for r in self.rows(doc) if r['name'] == name)

    def undo_count(self, doc):
        return int(self.call('list_history', document_id=doc)['undo_count'])

    def scene(self, depth, cmyk=False):
        extra = dict(color_mode='cmyk') if cmyk else {}
        doc = self.call('new_document', width=W, height=H, unit='px', depth=depth,
                        background='white', **extra)['document']['id']
        self.call('new_layer', document_id=doc, name='Ramp')
        ramp = self.index_of(doc, 'Ramp')
        for k, g in enumerate(GRAYS):
            self.call('select_rect', document_id=doc, x=k * PATCH, y=0, width=PATCH, height=H)
            self.call('fill_selection', document_id=doc, layer=ramp, color=f'#{g:02x}{g:02x}{g:02x}')
        self.call('deselect', document_id=doc)
        return doc

    def samples(self, doc, depth, cmyk=False):
        """Each patch's centre sample as unit values (CMYK: unit inks)."""
        out = []
        for k in range(len(GRAYS)):
            s = self.call('sample_color', document_id=doc, x=k * PATCH + PATCH // 2, y=H // 2)
            if cmyk:
                out.append([v / 65535 for v in s['ink16']])
            elif depth == 8:
                out.append([s['r'] / 255, s['g'] / 255, s['b'] / 255])
            elif depth == 16:
                out.append([v / 65535 for v in s['center_rgba16'][:3]])
            else:
                out.append(list(s['center_rgba32'][:3]))
        return out

    def map_params(self, doc):
        row = next(r for r in self.rows(doc) if r['name'] == 'Map')
        return (row.get('adjustment') or {}).get('params') or {}

    def add_map(self, doc, params):
        before = self.undo_count(doc)
        self.call('add_adjustment_layer', document_id=doc, name='Map', op='gradient_map',
                  params=params)
        return self.undo_count(doc) - before


def scale(depth, cmyk):
    if cmyk:
        return 65535.0
    return {8: 255.0, 16: 65535.0, 32: 1.0}[depth]


def worst(got, want, depth, cmyk):
    s = scale(depth, cmyk)
    return max(abs(g - w) * s for gp, wp in zip(got, want) for g, w in zip(gp, wp))


def reports_classic(params):
    gradient = params.get('gradient') or {}
    stops = gradient.get('stops') or []
    return (params.get('method') == 'classic'
            and abs(gradient.get('smoothness', 1) - SMOOTHNESS) < 1e-6
            and len(stops) == 4
            and all(abs(stops[i].get('midpoint', 0.5) - STOPS[i][3]) < 1e-6 for i in range(3))
            and abs(stops[1].get('opacity_midpoint', 0.5) - STOPS[1][4]) < 1e-6)


def gate(gates, name, ok, detail):
    gates[name] = dict(passed=bool(ok), detail=detail)
    print(('PASS ' if ok else 'FAIL ') + name, flush=True)


def run(drive, gates):
    folder = drive.folder
    scenes = {}
    for depth, cmyk in ((8, False), (16, False), (32, False), (16, True)):
        label = 'CMYK16' if cmyk else f'RGB{depth}'
        doc = drive.scene(depth, cmyk)
        before = drive.samples(doc, depth, cmyk)
        steps = drive.add_map(doc, classic_params(cmyk))
        after = drive.samples(doc, depth, cmyk)
        if cmyk:
            want = [want_ink(p, SMOOTHNESS) for p in before]
        else:
            want = [want_rgb(p, depth, STOPS, SMOOTHNESS) for p in before]
        err = worst(after, want, depth, cmyk)
        params = drive.map_params(doc)
        bound = BOUND['cmyk' if cmyk else depth]
        gate(gates, f'G1 {label}: one step, reported, the oracle',
             steps == 1 and reports_classic(params) and err <= bound,
             dict(steps=steps, params=params, worst=err, bound=bound, before=before,
                  after=after, want=want))
        scenes[label] = (doc, depth, cmyk, after, params)

    # G2: the refusal, no history step.
    doc = drive.scene(8)
    before = drive.undo_count(doc)
    bad = legacy_params()
    bad['gradient']['smoothness'] = SMOOTHNESS
    message = drive.app.client.call('add_adjustment_layer', fail=True, document_id=doc,
                                    name='Map', op='gradient_map', params=bad)
    gate(gates, 'G2 smoothness without method refuses in-band, no step',
         REFUSAL in message and drive.undo_count(doc) == before,
         dict(message=message, steps=drive.undo_count(doc) - before))
    drive.call('close_document', document_id=doc, discard_changes=True)

    # G3: .rz.
    for label in ('RGB8', 'RGB16'):
        doc, depth, cmyk, after, params = scenes[label]
        path = folder / f'classic-{label}.rz'
        drive.call('save_copy', document_id=doc, path=str(path), format='rz')
        data = path.read_bytes()
        words = [struct.unpack('<I', data[at:at + 4])[0] for at in (4, 8)]
        back = drive.call('open_document', path=str(path))['document']['id']
        again = drive.samples(back, depth, cmyk)
        gate(gates, f'G3 {label}: word {WORD}, the same params and patches after .rz',
             words[1] == WORD and drive.map_params(back) == params and again == after,
             dict(words=words, params=drive.map_params(back), samples=again))
        drive.call('close_document', document_id=back, discard_changes=True)

    # G4: Actions.
    first = drive.scene(8)
    drive.call('start_recording', discard=True)
    drive.add_map(first, classic_params())
    stopped = drive.call('stop_recording', name='gm678-classic')
    recorded = drive.samples(first, 8)
    second = drive.scene(8)
    replay = drive.call('run_action', document_id=second, name='gm678-classic')
    replayed = drive.samples(second, 8)
    gate(gates, 'G4 the recorded Action replays the Classic map',
         reports_classic(drive.map_params(second)) and replayed == recorded,
         dict(stopped=stopped, replay=replay, params=drive.map_params(second),
              recorded=recorded, replayed=replayed))
    for doc in (first, second):
        drive.call('close_document', document_id=doc, discard_changes=True)

    # G5: the CMYK grdm.
    doc, depth, cmyk, after, params = scenes['CMYK16']
    psd = folder / 'classic-cmyk.psd'
    drive.call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = drive.call('inspect_psd', path=str(psd))
    back = drive.call('open_document', path=str(psd), psd_mode='layers',
                      psd_fingerprint=report['source_fingerprint'])['document']['id']
    reopened = drive.map_params(back)
    again = drive.samples(back, depth, cmyk)
    ramp = drive.call('get_document', document_id=back)['layers']
    # The reopened map against the oracle of its own (file-quantized) stops,
    # over the reopened Ramp's patches read with the map hidden.
    hidden = next(r['index'] for r in ramp if r['name'] == 'Map')
    drive.call('set_layer_properties', document_id=back, index=hidden, visible=False)
    base = drive.samples(back, depth, cmyk)
    stops, inks, k = stops_of(reopened)
    want = [want_ink(p, k, stops, inks) for p in base]
    err = worst(again, want, depth, cmyk)
    gate(gates, 'G5 CMYK export_psd reopens the Classic map',
         reports_classic(reopened) and err <= BOUND['cmyk'],
         dict(params=reopened, worst=err, against_original=worst(again, after, depth, cmyk)))
    drive.call('close_document', document_id=back, discard_changes=True)

    # G6: legacy.
    doc = drive.scene(8)
    before = drive.samples(doc, 8)
    steps = drive.add_map(doc, legacy_params())
    after = drive.samples(doc, 8)
    flat = [(s[0], s[1], s[2], 0.5, 0.5) for s in STOPS]
    want = [want_rgb(p, 8, flat, 0.0) for p in before]
    err = worst(after, want, 8, False)
    params = drive.map_params(doc)
    gate(gates, 'G6 a legacy map is the linear map, unchanged',
         steps == 1 and 'method' not in params and err <= BOUND[8],
         dict(params=params, worst=err))
    drive.call('close_document', document_id=doc, discard_changes=True)
    for doc, *_ in scenes.values():
        drive.call('close_document', document_id=doc, discard_changes=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', type=Path, default=None,
                        help='a fresh folder (default: build/verify-gradient-map-classic/<time>)')
    arguments = parser.parse_args()
    output = arguments.output or (ROOT / 'build' / 'verify-gradient-map-classic'
                                  / time.strftime('%Y%m%d-%H%M%S'))
    output = output.expanduser().resolve()
    assert not output.exists() or not any(output.iterdir()), 'use a fresh output folder'
    output.mkdir(parents=True, exist_ok=True)
    gates = {}
    with App(output / 'app', output / 'recovery', interval=600) as app:
        run(Drive(app, output), gates)
    (output / 'receipt.json').write_text(json.dumps(gates, indent=2, default=str) + '\n')
    failed = [name for name, g in gates.items() if not g['passed']]
    print('verify_gradient_map_classic: ' + ('PASS' if not failed else f'FAIL {failed}'))
    raise SystemExit(0 if not failed else 1)


if __name__ == '__main__':
    main()
