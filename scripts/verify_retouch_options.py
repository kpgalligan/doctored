#!/usr/bin/env python3
"""The MCP drive for the Sponge's Vibrance and Sample All Layers on
Blur, Sharpen and Smudge, over MCP at 8, 16 and 32 bits and on a 16-bit CMYK
document, on an owned Debug instance (`phase5_support.App`: its own scratch
socket, a dummy API key, a private directory for every preference library),
terminated by its own context manager; no Doctored process this file did not
start is signalled.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The fixtures are written here in
pure Python: an 8-bit PNG of hashed colour structure (opened, and converted to
16 bits and to 32-bit float with `convert_precision`), and a 16-bit chunky
CMYK TIFF in macOS's Generic CMYK Profile.

- **V1 Vibrance.** `sponge_stroke` saturate at strength 100 along STROKE with
  `vibrance` true changes the INSIDE probes, leaves the FAR probes exactly as
  they were, echoes `applied.vibrance` true, and lands somewhere other than
  the same stroke with `vibrance` false (which echoes false); desaturate with
  `vibrance` changes the inside too. On CMYK the K ink is EXACTLY as it was at
  every inside probe. One `undo` restores every probe exactly.
- **S1 Sample All Layers.** Over the background, `new_layer` adds an empty
  layer (index 1). `blur_stroke`, `sharpen_stroke` and `smudge_stroke` on it
  with `sample_all_layers` true echo `applied.sample_all_layers` true, and
  with the background then hidden the composite holds visible pixels (alpha
  above 0) at an inside probe — the empty layer filled from the picture below
  — and none at a far probe; the same Blur without `sample_all_layers` changes
  nothing (`changed` false). Undo (the hide, then the stroke) restores every
  probe of the full composite exactly.
- **S2 Sample All Layers reads the visible layers above.** Right after S1
  (layer 1 "Empty" present and empty, the composite equal to the original),
  `new_layer` adds "Above" over it (index 2, gated through `get_document`),
  filled whole with `select_all` + `fill_selection` (`#FF00FF`; ink
  [0, 100, 0, 0] on CMYK) + `deselect`. `blur_stroke` on layer 1 with
  `sample_all_layers`, then layers 0 and 2 hidden: every INSIDE probe has
  alpha above 0 and each channel within 2 % of full scale of the fill
  ((255, 0, 255) at 8 bits, (65535, 0, 65535) at 16, (1, 0, 1) at 32, ink16
  (0, 65535, 0, 0) on CMYK). With layer 2 hidden BEFORE the same stroke (then
  layer 0 hidden), at least one INSIDE probe differs from the fill by more
  than 10 % of full scale on some channel. Undo and `delete_layer` 2 leave
  every probe of the full composite exactly as it was, so A1 and R1 start
  from the stack they always did.
- **A1 Actions.** At 8, 16 and 32 bits (`run_action` is unavailable in a CMYK
  document), a recording of `sponge_stroke` with `vibrance` and `blur_stroke`
  with `sample_all_layers` on the empty layer, replayed after undoing both,
  gives the same probes exactly.
- **R1 refusals by name.** `blur_stroke` with `sample_all_layers` and a
  `mask` target refuses "sample_all_layers samples the picture into a layer's
  pixels; it does not apply to a mask or channel target"; `blur_stroke` with
  `vibrance` refuses "vibrance applies to the sponge only"; `sponge_stroke`
  with `sample_all_layers` refuses "sample_all_layers applies to blur,
  sharpen and smudge". Each at every depth.
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
W, H = 120, 90
STROKE = [[30, 45], [90, 45]]
STROKE_SIZE = 16
INSIDE = [(40, 45), (60, 45), (80, 45)]
FAR = [(60, 5), (60, 85), (110, 10), (5, 80)]
TARGET_REFUSAL = ("sample_all_layers samples the picture into a layer's pixels; it does not "
                  "apply to a mask or channel target")
VIBRANCE_REFUSAL = 'vibrance applies to the sponge only'
SAMPLE_REFUSAL = 'sample_all_layers applies to blur, sharpen and smudge'
# S2: the fill of layer "Above" as `reading` reports it, and its full scale.
ABOVE_FILL = {
    'rgba8': ((255, 0, 255), 255),
    'rgba16': ((65535, 0, 65535), 65535),
    'rgba32': ((1, 0, 1), 1.0),
    'cmyk16': ((0, 65535, 0, 0), 65535),
}
ABOVE_CLOSE = 0.02
HIDDEN_APART = 0.10
DEPTHS = ['rgba8', 'rgba16', 'rgba32', 'cmyk16']


def hashed(i, salt):
    return ((i * 2654435761 + salt * 40503) >> 7) & 0xFFFF


def rgb_pixels():
    return bytes(v for y in range(H) for x in range(W)
                 for v in (40 + (x * 2 + hashed(y * W + x, 1) % 50) % 180,
                           60 + (y * 2 + hashed(y * W + x, 2) % 60) % 160,
                           30 + hashed(y * W + x, 3) % 170, 255))


def cmyk_samples():
    samples = []
    for y in range(H):
        for x in range(W):
            i = y * W + x
            samples += [min(65535, 9000 + x * 300 + hashed(i, 1) % 6000),
                        min(65535, 12000 + y * 400 + hashed(i, 2) % 6000),
                        min(65535, 30000 + hashed(i, 3) % 9000),
                        min(65535, 4000 + (x + y) * 90 + hashed(i, 4) % 5000)]
    return samples


def reading(reply):
    """The deepest numbers `sample_color` reports, and the alpha among them."""
    if 'ink16' in reply:
        return tuple(reply['ink16']) + (reply.get('alpha16', reply.get('a')),)
    for key in ['center_rgba32', 'center_rgba_codes']:
        if key in reply:
            return tuple(reply[key])
    return (reply['r'], reply['g'], reply['b'], reply['a'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    output = Path(args.receipts or tempfile.mkdtemp(prefix='retouch-options-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_retouch_options.py before any measurement'}
    rgb_path = folder / 'textured.png'
    png(rgb_path, W, H, rgb_pixels())
    cmyk_path = folder / 'textured.tif'
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

        def stroke(tool, doc, layer, **extra):
            return call(tool, document_id=doc, layer=layer, points=STROKE, size=STROKE_SIZE,
                        strength=100, **extra)

        everything = INSIDE + FAR
        for depth in DEPTHS:
            row = receipts.setdefault(depth, {})
            doc = open_depth(depth)
            original = probe(doc, everything)

            # ---- V1: Vibrance -------------------------------------------------
            reply = stroke('sponge_stroke', doc, 0, mode='saturate', vibrance=True)
            gate(reply.get('applied', {}).get('vibrance') is True,
                 f'{depth} V1 applied.vibrance true')
            vivid = probe(doc, everything)
            gate(any(vivid[p] != original[p] for p in INSIDE), f'{depth} V1 inside changes')
            gate(all(vivid[p] == original[p] for p in FAR), f'{depth} V1 far probes stay')
            if depth == 'cmyk16':
                gate(all(vivid[p][3] == original[p][3] for p in INSIDE),
                     f'{depth} V1 K ink held exactly')
            call('undo', document_id=doc)
            gate(probe(doc, everything) == original, f'{depth} V1 undo is exact')
            reply = stroke('sponge_stroke', doc, 0, mode='saturate', vibrance=False)
            gate(reply.get('applied', {}).get('vibrance') is False,
                 f'{depth} V1 applied.vibrance false')
            plain = probe(doc, everything)
            gate(any(plain[p] != vivid[p] for p in INSIDE),
                 f'{depth} V1 Vibrance differs from the plain Sponge')
            call('undo', document_id=doc)
            stroke('sponge_stroke', doc, 0, mode='desaturate', vibrance=True)
            muted = probe(doc, everything)
            gate(any(muted[p] != original[p] for p in INSIDE),
                 f'{depth} V1 desaturate with Vibrance changes the inside')
            if depth == 'cmyk16':
                gate(all(muted[p][3] == original[p][3] for p in INSIDE),
                     f'{depth} V1 desaturate holds K exactly')
            call('undo', document_id=doc)
            row['vibrance'] = {str(p): [vivid[p], plain[p]] for p in INSIDE}

            # ---- S1: Sample All Layers ---------------------------------------
            call('new_layer', document_id=doc, name='Empty')
            assert probe(doc, everything) == original, (depth, 'an empty layer moved the composite')
            reply = stroke('blur_stroke', doc, 1)
            gate(reply.get('changed') is False, f'{depth} S1 a plain blur of an empty layer is a no-op')
            for tool in ['blur_stroke', 'sharpen_stroke', 'smudge_stroke']:
                reply = stroke(tool, doc, 1, sample_all_layers=True)
                gate(reply.get('applied', {}).get('sample_all_layers') is True,
                     f'{depth} S1 {tool} applied.sample_all_layers true')
                call('set_layer_properties', document_id=doc, index=0, visible=False)
                alone = probe(doc, everything)
                alpha = [alone[p][-1] or 0 for p in INSIDE]
                gate(any(a > 0 for a in alpha), f'{depth} S1 {tool} fills the empty layer {alpha}')
                gate(all((alone[p][-1] or 0) == 0 for p in FAR),
                     f'{depth} S1 {tool} leaves the far probes empty')
                call('undo', document_id=doc)
                call('undo', document_id=doc)
                gate(probe(doc, everything) == original, f'{depth} S1 {tool} undo is exact')

            # ---- S2: the visible layer above is read, a hidden one is not ------
            call('new_layer', document_id=doc, name='Above')
            layers = call('get_document', document_id=doc)['layers']
            gate(len(layers) == 3 and layers[2].get('name') == 'Above',
                 f'{depth} S2 "Above" is layer 2 of three: {[l.get("name") for l in layers]}')
            call('select_all', document_id=doc)
            if depth == 'cmyk16':
                call('fill_selection', document_id=doc, layer=2, ink=[0, 100, 0, 0])
            else:
                call('fill_selection', document_id=doc, layer=2, color='#FF00FF')
            call('deselect', document_id=doc)
            fill, scale = ABOVE_FILL[depth]

            def apart(value):
                return max(abs(value[c] - fill[c]) for c in range(len(fill))) / scale

            stroke('blur_stroke', doc, 1, sample_all_layers=True)
            call('set_layer_properties', document_id=doc, index=0, visible=False)
            call('set_layer_properties', document_id=doc, index=2, visible=False)
            seen = probe(doc, INSIDE)
            gate(all((seen[p][-1] or 0) > 0 and apart(seen[p]) <= ABOVE_CLOSE for p in INSIDE),
                 f'{depth} S2 the layer above fills the empty layer {list(seen.values())}')
            for _ in range(3):
                call('undo', document_id=doc)
            call('set_layer_properties', document_id=doc, index=2, visible=False)
            stroke('blur_stroke', doc, 1, sample_all_layers=True)
            call('set_layer_properties', document_id=doc, index=0, visible=False)
            unseen = probe(doc, INSIDE)
            gate(any(apart(unseen[p]) > HIDDEN_APART for p in INSIDE),
                 f'{depth} S2 a hidden layer above is not read {list(unseen.values())}')
            for _ in range(3):
                call('undo', document_id=doc)
            call('delete_layer', document_id=doc, index=2)
            gate(probe(doc, everything) == original, f'{depth} S2 undo and delete restore exactly')
            row['sample_above'] = {str(p): [seen[p], unseen[p]] for p in INSIDE}

            # ---- A1: a recorded Action replays equal --------------------------
            actions = depth != 'cmyk16'
            if actions:
                call('start_recording', discard=True)
            if actions:
                stroke('sponge_stroke', doc, 0, mode='saturate', vibrance=True)
                stroke('blur_stroke', doc, 1, sample_all_layers=True)
                recorded = probe(doc, everything)
                call('stop_recording', name=f'options-{depth}')
                call('undo', document_id=doc)
                call('undo', document_id=doc)
                gate(probe(doc, everything) == original, f'{depth} A1 undo before the replay')
                call('run_action', document_id=doc, name=f'options-{depth}')
                replayed = probe(doc, everything)
                gate(replayed == recorded, f'{depth} A1 the Action replays equal')

            # ---- R1: refusals by name -----------------------------------------
            for tool, extra, sentence in [
                ('blur_stroke', dict(sample_all_layers=True, target='mask'), TARGET_REFUSAL),
                ('blur_stroke', dict(vibrance=True), VIBRANCE_REFUSAL),
                ('smudge_stroke', dict(vibrance=True), VIBRANCE_REFUSAL),
                ('sponge_stroke', dict(sample_all_layers=True), SAMPLE_REFUSAL),
            ]:
                text = app.client.call(tool, fail=True, document_id=doc, layer=0, points=STROKE,
                                       size=STROKE_SIZE, strength=100, **extra)
                gate(sentence in text, f'{depth} R1 {tool} {sorted(extra)}: {text[:120]}')
            call('close_document', document_id=doc, discard_changes=True)

    receipts['failures'] = failures
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    print('verify_retouch_options: PASS' if not failures
          else f'verify_retouch_options: FAIL ({len(failures)})')
    raise SystemExit(0 if not failures else 1)


if __name__ == '__main__':
    main()
