#!/usr/bin/env python3
"""The MCP drive: `crop`'s `fill` (the Crop tool's Fill popup) with
Content-Aware Fill and Background (Default) on the Debug app. Generative Expand's crop is driven by
`scripts/verify_generative_fill.py` (XK cases) against its stub providers.

The instance is owned here (`phase5_support.App`: a scratch MCP socket, a dummy API key, a private
folder for every preference library, the cloud providers pointed at a dead loopback port,
DR_BACKGROUND=1); no case reaches the network. The document is a 160 × 120 noise picture (record 0,
the Background) plus a small opaque patch on an upper layer, opened at 8 bits and converted where a
case says so. Whole pictures are read back through `save_copy` and the independent PNG reader
(`scripts/raw-probe/native.py`).

The gates are FIXED here BEFORE anything is measured; a failed gate IS the receipt.

- **CB** `crop` (-20, -10, 200 × 140) with `fill` "background" is pixel-identical to the same crop
  with no `fill`.
- **CA8 / CA16 / CAF / CAC** (8 bits, 16 bits, 32-bit float, 8-bit CMYK, read back as an 8-bit
  sRGB PNG export) `crop` (-20, -10,
  200 × 140) with `fill` "content_aware": ok, completed, `extended` true, `fill` "content_aware";
  the canvas 200 × 140; the old rect bit-exact against the picture before at its new offset
  (20, 10); with the Background hidden the upper layer's new area is transparent (on CMYK, whose
  export flattens onto paper, the composite's `sample_pixel` alpha at the new area's corners and
  strip middles); with the upper
  layer hidden the Background's new area is opaque and takes more than one value (not the
  uniform pad); the history cursor +1, and one undo restores 160 × 120 and the picture.
- **CAR** angle 5 at bicubic on a dark-noise Background (codes 0–100), pad #FFFFFF, upper layer
  hidden: the four corners opaque and no Background pixel with a channel above 200 (no pad-tinted
  seam along the old edge).
- **CAN** the Background hidden (no qualifying Background): refused naming the scope sentence.
- **CAX** `delete_cropped` false: refused naming the delete sentence.
- **CAI** a rectangle inside the canvas: refused naming the inside-window sentence.
- **CAU** `fill` "nope": refused naming the three fills.
  Each of CAN / CAX / CAI / CAU leaves the canvas 160 × 120 and the history cursor unchanged.
- **CAA** a recorded `content_aware` crop, replayed by `run_action` on a fresh twin, gives the
  same pixels.

Usage: python3 scripts/verify_crop_fill.py [--receipts DIR]. Prints PASS/FAIL per gate and exits
0 only when every gate passes.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import random
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402
from verify_phase1 import png  # noqa: E402

# ---- the expectations, fixed here BEFORE anything is measured -------------
WIDTH, HEIGHT = 160, 120
WINDOW = (-20, -10, 200, 140)
PATCH = (60, 40, 30, 20)
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
SCOPE = 'Content-Aware Fill on a crop fills the Background'
KEEPS = 'fill content_aware deletes the cropped pixels'
INSIDE = 'fill applies only to a rectangle past the canvas'
UNKNOWN = 'fill must be background, generative_expand or content_aware'
SEAM_CEILING = 200


def _load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


png_samples = _load('crop_fill_png_reader', 'scripts/raw-probe/native.py').png_samples


def noise(folder, name, *, low=0, high=255, seed=665):
    path = folder / name
    rng = random.Random(seed)
    pixels = bytes(v for _ in range(WIDTH * HEIGHT)
                   for v in (rng.randint(low, high), rng.randint(low, high), rng.randint(low, high), 255))
    png(path, WIDTH, HEIGHT, pixels)
    return path


def opened(app, folder, mode, tag, **noise_options):
    doc = app.call('open_document', path=str(noise(folder, f'noise-{tag}.png', **noise_options)),
                   precision='rgba8')['document']['id']
    if mode == 'cmyk':
        app.call('convert_mode', document_id=doc, mode='cmyk', profile='file', path=str(GENERIC))
    elif mode != 8:
        app.call('convert_precision', document_id=doc, mode=f'rgba{mode}', structure='preserve')
    app.call('new_layer', document_id=doc, name='Top')
    app.call('select_rect', document_id=doc, x=PATCH[0], y=PATCH[1], width=PATCH[2], height=PATCH[3])
    app.call('fill_selection', document_id=doc, layer=1, color='#20C040')
    app.call('deselect', document_id=doc)
    return doc


def shot(app, doc, folder, name):
    """The composite as RGBA samples: `save_copy` at the document's depth, or a CMYK document's
    8-bit sRGB PNG export (a CMYK document saves no PNG)."""
    path = folder / f'{name}.png'
    if name.startswith('CAC'):
        app.call('export_image', document_id=doc, path=str(path),
                 settings={'format': 'png', 'profile': 'srgb'})
    else:
        app.call('save_copy', document_id=doc, path=str(path))
    decoded = png_samples(path.read_bytes())
    return decoded['width'], decoded['height'], decoded['depth'], list(decoded['samples'])


def cursor(app, doc):
    return app.call('list_history', document_id=doc)['cursor']


def size_of(app, doc):
    summary = app.call('get_document', document_id=doc)
    return summary.get('width'), summary.get('height')


def visible(app, doc, index, on):
    app.call('set_layer_properties', document_id=doc, index=index, visible=on)


def crop(app, doc, fail=False, window=WINDOW, **arguments):
    x, y, w, h = window
    return app.call('crop', fail=fail, document_id=doc, x=x, y=y, width=w, height=h, **arguments)


def pixel(picture, x, y):
    width, _, _, samples = picture
    i = (y * width + x) * 4
    return samples[i:i + 4]


def old_rect_moved(after, before):
    """Pixels of the old canvas that are not bit-exact at their new offset."""
    moved = 0
    for y in range(HEIGHT):
        for x in range(WIDTH):
            if pixel(after, x - WINDOW[0], y - WINDOW[1]) != pixel(before, x, y):
                moved += 1
    return moved


def new_area(picture):
    width, height, _, _ = picture
    for y in range(height):
        for x in range(width):
            ox, oy = x + WINDOW[0], y + WINDOW[1]
            if not (0 <= ox < WIDTH and 0 <= oy < HEIGHT):
                yield pixel(picture, x, y)


def content_aware_case(app, folder, mode, tag, receipts, gates):
    doc = opened(app, folder, mode, tag)
    before = shot(app, doc, folder, f'{tag}-before')
    start = cursor(app, doc)
    reply = crop(app, doc, fill='content_aware')
    after_cursor = cursor(app, doc)
    size = size_of(app, doc)
    after = shot(app, doc, folder, f'{tag}-after')
    moved = old_rect_moved(after, before)
    app.call('undo', document_id=doc)
    undone_size = size_of(app, doc)
    undone = shot(app, doc, folder, f'{tag}-undone')
    undo_cursor = cursor(app, doc)
    app.call('redo', document_id=doc)
    visible(app, doc, 0, False)
    if mode == 'cmyk':
        # A CMYK export flattens onto paper, so the composite's own alpha (`sample_pixel`) is
        # read instead, at the new area's corners and the middle of each new strip.
        points = ((0, 0), (WINDOW[2] - 1, 0), (0, WINDOW[3] - 1), (WINDOW[2] - 1, WINDOW[3] - 1),
                  (5, WINDOW[3] // 2), (WINDOW[2] - 5, WINDOW[3] // 2), (WINDOW[2] // 2, 3),
                  (WINDOW[2] // 2, WINDOW[3] - 4))
        upper_clear = all(app.call('sample_pixel', document_id=doc, x=x, y=y)['mean_rgba16'][3] == 0
                          for x, y in points)
    else:
        upper_clear = all(p[3] == 0 for p in new_area(shot(app, doc, folder, f'{tag}-upper')))
    visible(app, doc, 0, True)
    visible(app, doc, 1, False)
    background = shot(app, doc, folder, f'{tag}-background')
    full = (1 << background[2]) - 1
    area = list(new_area(background))
    opaque = all(p[3] == full for p in area)
    values = len({tuple(p) for p in area})
    receipts[tag] = dict(reply=reply, start=start, after_cursor=after_cursor, size=size,
                         moved=moved, undone_size=undone_size, undo_cursor=undo_cursor,
                         upper_clear=upper_clear, opaque=opaque, new_area_values=values)
    gates[tag] = (
        reply.get('ok') is True and reply.get('state') == 'completed'
        and reply.get('extended') is True and reply.get('fill') == 'content_aware'
        and size == (WINDOW[2], WINDOW[3]) and moved == 0 and after_cursor == start + 1
        and undone_size == (WIDTH, HEIGHT) and undone == before and undo_cursor == start
        and upper_clear and opaque and values > 1)


def turned_case(app, folder, receipts, gates):
    doc = opened(app, folder, 8, 'CAR', high=100, seed=5)
    reply = crop(app, doc, fill='content_aware', angle=5, sampler='bicubic', background_color='#FFFFFF')
    visible(app, doc, 1, False)
    picture = shot(app, doc, folder, 'CAR-background')
    width, height, _, samples = picture
    corners = [pixel(picture, x, y) for x, y in ((0, 0), (width - 1, 0), (0, height - 1),
                                                 (width - 1, height - 1))]
    brightest = max(max(samples[i:i + 3]) for i in range(0, len(samples), 4))
    receipts['CAR'] = dict(reply=reply, corners=corners, brightest=brightest)
    gates['CAR'] = (reply.get('state') == 'completed' and all(c[3] == 255 for c in corners)
                    and brightest <= SEAM_CEILING)


def refusal_cases(app, folder, receipts, gates):
    doc = opened(app, folder, 8, 'refusals')
    for tag, prepare, arguments, expected in (
            ('CAN', lambda: visible(app, doc, 0, False), dict(fill='content_aware'), SCOPE),
            ('CAX', None, dict(fill='content_aware', delete_cropped=False), KEEPS),
            ('CAI', None, dict(fill='content_aware', window=(10, 10, 100, 80)), INSIDE),
            ('CAU', None, dict(fill='nope'), UNKNOWN)):
        if prepare:
            prepare()
        start = cursor(app, doc)
        message = crop(app, doc, fail=True, **arguments)
        size = size_of(app, doc)
        receipts[tag] = dict(message=message, size=size, cursor=cursor(app, doc), start=start)
        gates[tag] = expected in message and size == (WIDTH, HEIGHT) and cursor(app, doc) == start
        if prepare:
            visible(app, doc, 0, True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='crop-fill-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_crop_fill.py before any measurement'}
    gates = {}
    saved = os.environ.get('DR_BACKGROUND')
    try:
        os.environ['DR_BACKGROUND'] = '1'
        with App(folder / 'app', folder / 'recovery', interval=600) as app:
            plain, named = opened(app, folder, 8, 'CB-plain'), opened(app, folder, 8, 'CB-named')
            crop(app, plain)
            crop(app, named, fill='background')
            receipts['CB'] = dict(sizes=[size_of(app, plain), size_of(app, named)])
            gates['CB'] = shot(app, plain, folder, 'CB-plain') == shot(app, named, folder, 'CB-named')

            for mode, tag in ((8, 'CA8'), (16, 'CA16'), (32, 'CAF'), ('cmyk', 'CAC')):
                content_aware_case(app, folder, mode, tag, receipts, gates)
            turned_case(app, folder, receipts, gates)
            refusal_cases(app, folder, receipts, gates)

            recorded = opened(app, folder, 8, 'CAA')
            app.call('start_recording', discard=True)
            crop(app, recorded, fill='content_aware')
            stopped = app.call('stop_recording', name='crop-fill-caa')
            twin = opened(app, folder, 8, 'CAA-twin')
            replay = app.call('run_action', document_id=twin, name='crop-fill-caa')
            same = shot(app, recorded, folder, 'CAA-recorded') == shot(app, twin, folder, 'CAA-replayed')
            receipts['CAA'] = dict(steps=stopped.get('steps'), replay=replay, same=same)
            gates['CAA'] = same and any(step.get('arguments', {}).get('fill') == 'content_aware'
                                        for step in stopped.get('steps') or [])
    finally:
        if saved is None:
            os.environ.pop('DR_BACKGROUND', None)
        else:
            os.environ['DR_BACKGROUND'] = saved
    receipts['gates'] = gates
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str))
    for name, ok in gates.items():
        print(f'{"PASS" if ok else "FAIL"} {name}')
    print(f'receipt: {output / "receipt.json"}')
    return 0 if gates and all(gates.values()) else 1


if __name__ == '__main__':
    raise SystemExit(main())
