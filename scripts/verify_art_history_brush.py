#!/usr/bin/env python3
"""The MCP drive: the Art History Brush (`art_history_brush_stroke`) at 8,
16 and 32 bits and on 8- and 16-bit CMYK documents, on an owned Debug instance
(`phase5_support.App`: its own scratch socket, a dummy API key, a private
directory for every preference library), terminated by its own context
manager; no Doctored process this file did not start is signalled.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. Which depths paint is read from
the take's receipt (`art_history_take.py` take1, `receipt.json`'s
`availability`): a depth the take marks DISABLED must refuse by name, any
other must paint. 8-bit RGB and 8-bit CMYK always paint (the take's controls).

The fixtures are written here in pure Python: an 8-bit PNG with R = x, G = y,
B = 0 (opened, and converted to 16 bits and to 32-bit float with
`convert_precision`), and a 16-bit chunky CMYK TIFF in macOS's Generic CMYK
Profile with C = x, M = y (×257), Y = K = 0 (converted to 8 bits with
`convert_precision` mode cmyk8). Each document's history is the opened state,
then its layer filled white (paper on CMYK), so the History panel's source —
the opened state — differs from the layer everywhere.

- **T1 per depth.** A one-point Dab stroke, Area 0, size 9, seed 1, at each
  INSIDE point: every INSIDE probe becomes exactly the opened state's sample
  there (a Dab at Area 0 is one dab centred on its click, coloured from the
  source pixel under it); every FAR probe is exactly as it was; one `undo`
  per stroke restores every probe exactly. At a DISABLED depth the call refuses with that
  depth's sentence and every probe is exactly as it was.
- **T2 the seed.** tight_medium, Area 50, size 7, seed 42 at the centre: the
  GRID probes change, and after an `undo` the same call gives the same GRID
  probes exactly (8-bit RGB and 8-bit CMYK).
- **T3 tolerance.** On the opened state itself (nothing filled), tolerance 100
  replies `changed: false` (8-bit RGB and 8-bit CMYK).
- **T4 the heading (fixed before it ran).** 8-bit RGB and 8-bit
  CMYK: tight_long, Area 0, size 9, seed 1, at CENTRE (one stroke starting at
  the click, coloured from the opened state there). The shipped travel rule
  (BRIGHT-RIGHT: the source's luma contour, the brighter side on the
  stroke's right) heads it −26.95° on the RGB gradient and 153.05° on the
  CMYK one (C = x, M = y: its naive RGB `1 − ink` brightens up and left).
  The probes 10 and 16 px along that heading change and equal the opened
  state's sample at CENTRE exactly; the probes 12 px behind CENTRE and 10 px
  either side of it, perpendicular to the heading, stay exactly. One `undo`
  restores them.
- **A1 Actions.** At 8, 16 and 32 bits (`run_action` is unavailable in a CMYK
  document): a recording of tight_short, Area 40, seed 7 at the centre,
  replayed after undoing it, gives the same GRID probes exactly. At a DISABLED
  depth the call refuses with that depth's sentence instead.
- **R1 refusal by name.** `style: "bogus"` and `blend_mode: "Multiply"` refuse
  with their sentences, at every depth.
"""
import argparse
import json
import math
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402
from verify_phase1 import png  # noqa: E402
from verify_cmyk_edit import GENERIC, cmyk_tiff  # noqa: E402

# ---- the expectations, fixed here BEFORE anything is measured -------------
W = H = 256
TAKE = Path('~/devel/doctored-fixtures/photoshop/art-history-658/take1/receipt.json').expanduser()
INSIDE = [(64, 64), (192, 192)]
FAR = [(2, 2), (253, 2), (2, 253), (253, 253), (128, 10)]
CENTRE = (128, 128)
GRID = [(CENTRE[0] + dx, CENTRE[1] + dy) for dy in range(-24, 25, 6) for dx in range(-24, 25, 6)]
DEPTHS = ['rgba8', 'rgba16', 'rgba32', 'cmyk8', 'cmyk16']
AVAILABILITY_KEY = {'rgba16': 'rgb16', 'rgba32': 'float32', 'cmyk16': 'cmyk16'}
SENTENCES = {
    'rgba16': 'The Art History Brush is unavailable in a 16-bit document.',
    'rgba32': 'The Art History Brush is unavailable in a 32-bit document.',
    'cmyk16': 'The Art History Brush is unavailable in a 16-bit CMYK document.',
}
STYLE_REFUSAL = ('style must be tight_short, tight_medium, tight_long, loose_medium, loose_long, '
                 'dab, tight_curl, tight_curl_long, loose_curl or loose_curl_long')
MODE_REFUSAL = 'blend_mode must be Normal, Darken, Lighten, Hue, Saturation, Color or Luminosity'
# T4: the shipped travel rule's heading per depth, in degrees (y down).
T4_TRAVEL = {'rgba8': -26.95, 'cmyk8': 153.05}


def t4_probes(depth):
    """(along, still): the pixels 10 and 16 px along T4's heading from CENTRE,
    and those 12 px behind it and 10 px either side, perpendicular."""
    a = math.radians(T4_TRAVEL[depth])
    at = lambda d, s: (math.floor(CENTRE[0] + d * math.cos(a) - s * math.sin(a)),  # noqa: E731
                       math.floor(CENTRE[1] + d * math.sin(a) + s * math.cos(a)))
    return [at(10, 0), at(16, 0)], [at(-12, 0), at(0, 10), at(0, -10)]


def rgb_pixels():
    return bytes(v for y in range(H) for x in range(W) for v in (x, y, 0, 255))


def cmyk_samples():
    return [v for y in range(H) for x in range(W) for v in (x * 257, y * 257, 0, 0)]


def reading(reply):
    """The deepest numbers `sample_color` reports."""
    if 'ink16' in reply:
        return tuple(reply['ink16']) + (reply.get('alpha16', reply.get('a')),)
    for key in ['center_rgba32', 'center_rgba_codes']:
        if key in reply:
            return tuple(reply[key])
    return (reply['r'], reply['g'], reply['b'], reply['a'])


def paints(depth, availability):
    key = AVAILABILITY_KEY.get(depth)
    return key is None or availability.get(key) != 'DISABLED'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    assert TAKE.is_file(), f'take1 has no receipt at {TAKE}'
    availability = json.loads(TAKE.read_text())['availability']
    output = Path(args.receipts or tempfile.mkdtemp(prefix='art-history-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_art_history_brush.py before any '
                                'measurement', 'availability': availability}
    rgb_path = folder / 'gradient.png'
    png(rgb_path, W, H, rgb_pixels())
    cmyk_path = folder / 'gradient.tif'
    cmyk_path.write_bytes(cmyk_tiff(W, H, cmyk_samples(), GENERIC.read_bytes()))
    failures = []

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def gate(ok, what):
            print(('ok   ' if ok else 'FAIL ') + what, flush=True)
            if not ok:
                failures.append(what)

        def probe(doc, points):
            return {p: reading(call('sample_color', document_id=doc, x=p[0], y=p[1]))
                    for p in points}

        def open_depth(depth):
            if depth.startswith('cmyk'):
                doc = call('open_document', path=str(cmyk_path))['document']['id']
                if depth == 'cmyk8':
                    call('convert_precision', document_id=doc, mode='cmyk8')
                return doc
            doc = call('open_document', path=str(rgb_path))['document']['id']
            if depth != 'rgba8':
                call('convert_precision', document_id=doc, mode=depth, structure='preserve')
            return doc

        def whiten(doc, depth):
            call('select_rect', document_id=doc, x=0, y=0, width=W, height=H)
            if depth.startswith('cmyk'):
                call('fill_selection', document_id=doc, layer=0, ink=[0, 0, 0, 0])
            else:
                call('fill_selection', document_id=doc, layer=0, color='#FFFFFF')
            call('deselect', document_id=doc)

        def stroke(doc, fail=False, **options):
            arguments = dict(document_id=doc, layer=0, **options)
            if fail:
                return app.client.call('art_history_brush_stroke', fail=True, **arguments)
            return call('art_history_brush_stroke', **arguments)

        dab = dict(style='dab', area=0, size=9, seed=1)
        for depth in DEPTHS:
            row = receipts.setdefault(depth, {})
            painting = paints(depth, availability)
            row['paints'] = painting
            doc = open_depth(depth)
            source = probe(doc, INSIDE + [CENTRE])

            # ---- T3: tolerance 100 on the opened state ------------------------
            if painting and depth in ('rgba8', 'cmyk8'):
                reply = stroke(doc, points=[list(CENTRE)], style='tight_short', area=50, size=7,
                               tolerance=100, seed=3)
                row['t3'] = reply
                gate(reply.get('changed') is False, f'{depth} T3 tolerance 100 changes nothing')

            whiten(doc, depth)
            everything = INSIDE + FAR + GRID
            original = probe(doc, everything)

            # ---- R1: refusal by name -------------------------------------------
            text = stroke(doc, fail=True, points=[list(CENTRE)], style='bogus')
            gate(STYLE_REFUSAL in text, f'{depth} R1 bogus style: {text[:100]}')
            text = stroke(doc, fail=True, points=[list(CENTRE)], blend_mode='Multiply')
            gate(MODE_REFUSAL in text, f'{depth} R1 Multiply: {text[:100]}')

            # ---- T1 ------------------------------------------------------------
            if not painting:
                text = stroke(doc, fail=True, points=[list(INSIDE[0])], **dab)
                gate(SENTENCES[depth] in text, f'{depth} T1 refused by name: {text[:110]}')
                gate(probe(doc, everything) == original, f'{depth} T1 the refusal changes nothing')
                if depth.startswith('rgba'):
                    gate(SENTENCES[depth] in stroke(doc, fail=True, points=[list(CENTRE)],
                                                    style='tight_short', area=40, seed=7),
                         f'{depth} A1 refused by name instead of recorded')
                call('close_document', document_id=doc, discard_changes=True)
                continue
            for p in INSIDE:
                stroke(doc, points=[list(p)], **dab)
            after = probe(doc, everything)
            for p in INSIDE:
                gate(after[p] == source[p], f'{depth} T1 inside {p} takes the source '
                                            f'{source[p]} (got {after[p]})')
            gate(all(after[p] == original[p] for p in FAR), f'{depth} T1 far probes stay')
            for _ in INSIDE:
                call('undo', document_id=doc)
            gate(probe(doc, everything) == original, f'{depth} T1 one undo a stroke restores')

            # ---- T2: the same seed, the same strokes ---------------------------
            seeded = dict(points=[list(CENTRE)], style='tight_medium', area=50, size=7, seed=42)
            stroke(doc, **seeded)
            first = probe(doc, GRID)
            gate(any(first[p] != original[p] for p in GRID), f'{depth} T2 the strokes land')
            call('undo', document_id=doc)
            stroke(doc, **seeded)
            gate(probe(doc, GRID) == first, f'{depth} T2 the same seed repeats exactly')
            call('undo', document_id=doc)

            # ---- T4: the luma heading and its travel ----------------------------
            if depth in T4_TRAVEL:
                along, still = t4_probes(depth)
                before = probe(doc, along + still)
                stroke(doc, points=[list(CENTRE)], style='tight_long', area=0, size=9, seed=1)
                got = probe(doc, along + still)
                row['t4'] = {str(p): got[p] for p in along + still}
                for p in along:
                    gate(got[p] != before[p] and got[p] == source[CENTRE],
                         f'{depth} T4 {p} along the heading takes the source at CENTRE '
                         f'{source[CENTRE]} (got {got[p]})')
                for p in still:
                    gate(got[p] == before[p], f'{depth} T4 {p} off the heading stays (got {got[p]})')
                call('undo', document_id=doc)
                gate(probe(doc, along + still) == before, f'{depth} T4 one undo restores')

            # ---- A1: a recorded Action replays equal ---------------------------
            if depth.startswith('rgba'):
                call('start_recording', discard=True)
                stroke(doc, points=[list(CENTRE)], style='tight_short', area=40, size=7, seed=7)
                recorded = probe(doc, GRID)
                call('stop_recording', name=f'art-history-{depth}')
                call('undo', document_id=doc)
                gate(probe(doc, GRID) == {p: original[p] for p in GRID},
                     f'{depth} A1 undo before the replay')
                call('run_action', document_id=doc, name=f'art-history-{depth}')
                gate(probe(doc, GRID) == recorded, f'{depth} A1 the Action replays equal')
            call('close_document', document_id=doc, discard_changes=True)

    receipts['failures'] = failures
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    print('verify_art_history_brush: PASS' if not failures
          else f'verify_art_history_brush: FAIL ({len(failures)})')
    raise SystemExit(0 if not failures else 1)


if __name__ == '__main__':
    main()
