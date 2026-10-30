#!/usr/bin/env python3
"""The MCP drive for the Spot Healing Brush's Type (Content-Aware,
Create Texture, Proximity Match) over MCP at 8, 16 and 32 bits and on a 16-bit
CMYK document, on an owned Debug instance (`phase5_support.App`: its own
scratch socket, a dummy API key, a private directory for every preference
library), terminated by its own context manager; no Doctored process this
file did not start is signalled.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The fixtures are written here in
pure Python: an 8-bit PNG of hashed noise (opened, and converted to 16 bits and
to 32-bit float with `convert_precision`), and a 16-bit chunky CMYK TIFF of
hashed ink in macOS's Generic CMYK Profile. Each holds a dark spot of radius
9 at SPOT, well inside the 21 px stroke's footprint whatever its anti-aliased
edge, and 40 px to its right a copy of the spot's surroundings (out to 16.5 px,
past the footprint's band) around a flat disc of radius 9: the one patch whose
band matches the stroke's band exactly, so the blend has nothing to correct.
(The first run put both discs at 10.5, the footprint's own nominal radius; the
brush's anti-aliased contour left dark spot pixels outside the hole, against
the planted patch, and T3 measured that mismatch, not the type.)

- **T1 per type.** `spot_heal_stroke` with one point at SPOT, size 21, for each
  `type`: the reply echoes the type (`applied.type`, or `details.applied.type`
  on a typed document); at least one INSIDE probe changes; every FAR probe is
  exactly as it was; one `undo` restores every probe exactly.
- **T2 the types differ.** The three types' INSIDE readings are pairwise
  different.
- **T3 Proximity Match takes the planted patch.** Every INSIDE probe after
  `proximity_match` is within 2 % of full scale, on every colour channel (ink
  on CMYK), of the reading at the planted patch's centre.
- **A1 Actions.** At 8, 16 and 32 bits (`run_action` is unavailable in a CMYK
  document), a recording of `create_texture` then `proximity_match` at SPOT,
  replayed after undoing both, gives the same probes exactly.
- **R1 refusal by name.** `type: "bogus"` refuses "type must be content_aware,
  create_texture or proximity_match", at every depth.
"""
import argparse
import json
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
W, H = 200, 140
SPOT = (80, 70)
SHIFT = 40
RADIUS = 9.0
STROKE_SIZE = 21
INSIDE = [(80, 70), (76, 70), (84, 70), (80, 66), (80, 74)]
FAR = [(5, 5), (195, 5), (195, 135), (5, 135), (190, 70)]
PLANTED = (SPOT[0] + SHIFT, SPOT[1])
TYPES = ['content_aware', 'create_texture', 'proximity_match']
BOGUS_REFUSAL = 'type must be content_aware, create_texture or proximity_match'
PLANTED_CLOSE = 0.02
FULL_SCALE = {'rgba8': 255, 'rgba16': 65535, 'rgba32': 1.0, 'cmyk16': 65535}
FLAT_RGB = (200, 40, 120)
FLAT_INK = (5000, 50000, 20000, 3000)
DEPTHS = ['rgba8', 'rgba16', 'rgba32', 'cmyk16']


def hashed(x, y, salt):
    v = (x * 0x9E3779B1 + y * 0x85EBCA77 + salt * 0xC2B2AE3D) & 0xFFFFFFFF
    v ^= v >> 15
    v = (v * 0x2C1B3C6D) & 0xFFFFFFFF
    v ^= v >> 12
    return v >> 16


def region(x, y):
    """'flat', 'copy', 'spot' or 'field' for canvas pixel (x, y)."""
    planted = ((x - PLANTED[0]) ** 2 + (y - PLANTED[1]) ** 2) ** 0.5
    if planted <= RADIUS:
        return 'flat'
    if planted <= 16.5:
        return 'copy'
    if ((x - SPOT[0]) ** 2 + (y - SPOT[1]) ** 2) ** 0.5 <= RADIUS:
        return 'spot'
    return 'field'


def rgb_pixels():
    out = bytearray()
    for y in range(H):
        for x in range(W):
            kind = region(x, y)
            if kind == 'flat':
                out += bytes(FLAT_RGB + (255,))
            elif kind == 'spot':
                out += bytes((12, 12, 12, 255))
            else:
                sx = x - SHIFT if kind == 'copy' else x
                out += bytes([hashed(sx, y, c) & 0xFF for c in (1, 2, 3)] + [255])
    return bytes(out)


def cmyk_samples():
    samples = []
    for y in range(H):
        for x in range(W):
            kind = region(x, y)
            if kind == 'flat':
                samples += list(FLAT_INK)
            elif kind == 'spot':
                samples += [60000, 60000, 60000, 60000]
            else:
                sx = x - SHIFT if kind == 'copy' else x
                samples += [hashed(sx, y, c) for c in (1, 2, 3, 4)]
    return samples


def reading(reply):
    """The deepest numbers `sample_color` reports, and the alpha among them."""
    if 'ink16' in reply:
        return tuple(reply['ink16']) + (reply.get('alpha16', reply.get('a')),)
    for key in ['center_rgba32', 'center_rgba_codes']:
        if key in reply:
            return tuple(reply[key])
    return (reply['r'], reply['g'], reply['b'], reply['a'])


def applied_type(reply):
    applied = reply.get('applied') or reply.get('details', {}).get('applied') or {}
    return applied.get('type')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    output = Path(args.receipts or tempfile.mkdtemp(prefix='spot-heal-types-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_spot_heal_types.py before any measurement'}
    rgb_path = folder / 'spotted.png'
    png(rgb_path, W, H, rgb_pixels())
    cmyk_path = folder / 'spotted.tif'
    cmyk_path.write_bytes(cmyk_tiff(W, H, cmyk_samples(), GENERIC.read_bytes()))
    failures = []

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def gate(ok, what):
            print(('ok   ' if ok else 'FAIL ') + what)
            if not ok:
                failures.append(what)

        def probe(doc, points):
            return {p: reading(call('sample_color', document_id=doc, x=p[0], y=p[1]))
                    for p in points}

        def open_depth(depth):
            if depth == 'cmyk16':
                return call('open_document', path=str(cmyk_path))['document']['id']
            doc = call('open_document', path=str(rgb_path))['document']['id']
            if depth != 'rgba8':
                call('convert_precision', document_id=doc, mode=depth, structure='preserve')
            return doc

        def heal(doc, kind):
            return call('spot_heal_stroke', document_id=doc, layer=0, points=[list(SPOT)],
                        size=STROKE_SIZE, type=kind)

        everything = INSIDE + FAR
        for depth in DEPTHS:
            row = receipts.setdefault(depth, {})
            doc = open_depth(depth)
            original = probe(doc, everything)
            planted = probe(doc, [PLANTED])[PLANTED]
            channels = 4 if depth == 'cmyk16' else 3
            scale = FULL_SCALE[depth]

            # ---- T1 / T2 / T3 --------------------------------------------------
            inside = {}
            for kind in TYPES:
                reply = heal(doc, kind)
                gate(applied_type(reply) == kind, f'{depth} T1 {kind} echoes its type')
                healed = probe(doc, everything)
                gate(any(healed[p] != original[p] for p in INSIDE), f'{depth} T1 {kind} inside changes')
                gate(all(healed[p] == original[p] for p in FAR), f'{depth} T1 {kind} far probes stay')
                inside[kind] = tuple(healed[p] for p in INSIDE)
                if kind == 'proximity_match':
                    worst = max(abs(healed[p][c] - planted[c]) / scale
                                for p in INSIDE for c in range(channels))
                    gate(worst <= PLANTED_CLOSE,
                         f'{depth} T3 the planted patch within 2 % (worst {worst:.4f})')
                    row['proximity_worst'] = worst
                call('undo', document_id=doc)
                gate(probe(doc, everything) == original, f'{depth} T1 {kind} undo is exact')
            gate(len(set(inside.values())) == 3, f'{depth} T2 the three types differ inside')
            row['inside'] = {k: v for k, v in inside.items()}

            # ---- A1: a recorded Action replays equal --------------------------
            if depth != 'cmyk16':
                call('start_recording', discard=True)
                heal(doc, 'create_texture')
                heal(doc, 'proximity_match')
                recorded = probe(doc, everything)
                call('stop_recording', name=f'spot-types-{depth}')
                call('undo', document_id=doc)
                call('undo', document_id=doc)
                gate(probe(doc, everything) == original, f'{depth} A1 undo before the replay')
                call('run_action', document_id=doc, name=f'spot-types-{depth}')
                gate(probe(doc, everything) == recorded, f'{depth} A1 the Action replays equal')

            # ---- R1: refusal by name -----------------------------------------
            text = app.client.call('spot_heal_stroke', fail=True, document_id=doc, layer=0,
                                   points=[list(SPOT)], size=STROKE_SIZE, type='bogus')
            gate(BOGUS_REFUSAL in text, f'{depth} R1 bogus type: {text[:120]}')
            call('close_document', document_id=doc, discard_changes=True)

    receipts['failures'] = failures
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    print('verify_spot_heal_types: PASS' if not failures
          else f'verify_spot_heal_types: FAIL ({len(failures)})')
    raise SystemExit(0 if not failures else 1)


if __name__ == '__main__':
    main()
