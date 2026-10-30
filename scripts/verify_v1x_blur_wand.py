#!/usr/bin/env python3
"""MCP drive for batch v1x-blur-wand: Filter > Blur > Gaussian
Blur runs its fitted kernel at 8, 16 and 32 bits, and the Magic Wand and
Paint Bucket admit by the seven-projection box.

The script owns one app instance (`verify_v1_lens.App`): the Debug app (or
`DR_VERIFY_APP`) on its own scratch MCP socket with `DR_BACKGROUND=1`, a
dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s.
It terminates only that process. Every expected number is computed here, by
this file's own oracle, never read back from the app.

GAUSSIAN. The oracle is a binary64 extended box (Gwosdek et al. 2011): K
passes per axis of radius l and end weight alpha for variance sigma^2/K, the
line padded once by its full support (exact clamp-to-edge), rows then
columns, on premultiplied samples. PASSES, SCALE and MIN_RADIUS restate
the fitted law of `core/src/gaussian_blur_filter.rs`: from radius 7 up, 4
boxes at sigma = 0.99 r (below 7 Gaussian Blur keeps its old kernel). Radii
7.5 and 14. The
fixture is a 48x40 opaque picture: a diagonal step plus a smooth ramp plus
splitmix noise. Each radius is checked on a grid of sampled pixels:
- 8 bits: every channel within 1 code of the oracle;
- 16 bits (`convert_precision rgba16`): within 1 8-bit level (257 codes);
- 32 bits (`convert_precision rgba32`, linear light): `center_rgba32`
  within 1e-4 of the oracle run on the decoded linear values.

WAND / BUCKET. A 48x40 field of grey 128 holding 8x8 patches of hue and
brightness shifts (a shift of d in one channel moves R-G or R-B by d; a
paired (d, -d, 0) moves R-G by 2d). For tolerance t in 10, 32, contiguous on
and off, `select_magic_wand` at a field pixel must equal the oracle's
seven-projection region exactly over every pixel (`get_selection` samples),
and `fill` with (0, 255, 0) must change exactly that region (read back by a
tolerance-0 global wand on the fill colour, which no fixture pixel holds).
The 16-bit document (codes x 257, tolerance as sent, the host scaling t x
257) must select the same region.

Usage: python3 scripts/verify_v1x_blur_wand.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS` when every case passed.
"""
import argparse
import math
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_phase1 import png  # noqa: E402
from verify_v1_lens import App  # noqa: E402

W, H = 48, 40

# The fitted law of `core/src/gaussian_blur_filter.rs`, restated: from radius
# 7 up, PASSES extended boxes at sigma = SCALE * radius.
PASSES, SCALE, MIN_RADIUS = 4, 0.99, 7.0
RADII = [7.5, 14.0]


# ---------------------------------------------------------------- Gaussian --

def box_line(x, sigma, passes):
    v = sigma * sigma / passes
    l = max(0, math.floor(math.sqrt(12 * v + 1) / 2 - 0.5))
    alpha = (2 * l + 1) * (l * (l + 1) - 3 * v) / (6 * (v - (l + 1) ** 2))
    support = passes * (l + 1)
    n = len(x)
    line = [x[min(max(i, 0), n - 1)] for i in range(-support, n + support)]
    weight = 2 * l + 1 + 2 * alpha
    for _ in range(passes):
        reach = l + 1
        line = [(sum(line[i - l:i + l + 1]) + alpha * (line[i - reach] + line[i + reach])) / weight
                for i in range(reach, len(line) - reach)]
    return line


def gaussian(plane, radius):
    """plane: [h][w][3] floats (opaque, so premultiplied = straight)."""
    assert radius >= MIN_RADIUS
    sigma = SCALE * radius

    def line(x):
        return box_line(x, sigma, PASSES)
    rows = [[line([p[c] for p in row]) for c in range(3)] for row in plane]
    rows = [[[rows[y][c][x] for c in range(3)] for x in range(W)] for y in range(H)]
    cols = [[line([rows[y][x][c] for y in range(H)]) for c in range(3)] for x in range(W)]
    return [[[cols[x][c][y] for c in range(3)] for x in range(W)] for y in range(H)]


def splitmix(state):
    state = (state + 0x9E3779B97F4A7C15) & (2 ** 64 - 1)
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & (2 ** 64 - 1)
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & (2 ** 64 - 1)
    return state, z ^ (z >> 31)


def blur_picture():
    state = 0x443
    out = []
    for y in range(H):
        row = []
        for x in range(W):
            state, v = splitmix(state)
            step = 160 if x + y > 40 else 30
            row.append([min(255, step + (v & 31)), min(255, 20 + 4 * x + ((v >> 8) & 15)),
                        min(255, 200 - 3 * y + ((v >> 16) & 15))])
        out.append(row)
    return out


def write(folder, name, picture):
    data = bytearray()
    for row in picture:
        for p in row:
            data += bytes(p + [255])
    path = Path(folder) / name
    png(path, W, H, bytes(data))
    return path


POINTS = [(x, y) for y in range(0, H, 3) for x in range(0, W, 4)] + [(W - 1, H - 1), (0, H - 1)]


def sample(app, document, x, y):
    return app.call('sample_pixel', document_id=document, source='layer', layer=0, x=x, y=y)


def srgb_decode(v):
    v = v / 255
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def blur_case(app, folder, depth):
    picture = blur_picture()
    worst = {}
    for radius in RADII:
        document = app.call('open_document', path=str(write(folder, f'blur-{depth}-{radius}.png',
                                                             picture)))['document']['id']
        if depth == 16:
            app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
        if depth == 32:
            app.call('convert_precision', document_id=document, mode='rgba32',
                     structure='preserve')
        source = picture if depth != 32 else [[[srgb_decode(c) for c in p] for p in row]
                                              for row in picture]
        want = gaussian(source, radius)
        reply = app.call('apply_filter', document_id=document, filter='blur', sigma=radius)
        assert reply.get('ok', True), reply
        error = 0.0
        for x, y in POINTS:
            got = sample(app, document, x, y)
            for c in range(3):
                if depth == 8:
                    d = abs(got['center_rgba_codes'][c] - (math.floor(want[y][x][c] + 0.5)))
                    assert d <= 1, ('8-bit', radius, x, y, c, got, want[y][x])
                elif depth == 16:
                    d = abs(got['center_rgba_codes'][c] / 257 - want[y][x][c])
                    assert d <= 1.0, ('16-bit', radius, x, y, c, got, want[y][x])
                else:
                    d = abs(got['center_rgba32'][c] - want[y][x][c])
                    assert d <= 1e-4, ('32-bit', radius, x, y, c, got, want[y][x])
                error = max(error, d)
        worst[radius] = round(error, 6)
        app.call('close_document', document_id=document, discard_changes=True)
    return worst


# ------------------------------------------------------------ wand / bucket --

SEED = (128, 128, 128)
FILL = (0, 255, 0)
SHIFTS = [(10, 10, 10), (11, 11, 11), (10, 0, 0), (11, 0, 0), (0, 0, -10), (5, -5, 0), (6, -6, 0),
          (16, -16, 0), (-17, 17, 0), (32, 0, 0), (33, 0, 0), (20, 20, 0), (-30, -30, -30),
          (0, 16, -16), (0, 17, -17), (40, 40, 40), (8, -3, 2), (-9, 9, 9), (3, 3, -3), (0, 0, 33)]


def wand_picture():
    picture = [[list(SEED) for _ in range(W)] for _ in range(H)]
    for k, shift in enumerate(SHIFTS):
        j, i = divmod(k, 5)
        colour = [s + d for s, d in zip(SEED, shift)]
        for y in range(2 + 10 * j, 10 + 10 * j):
            for x in range(4 + 9 * i, 12 + 9 * i):
                picture[y][x] = colour
    return picture


def admitted(p, seed, t):
    d = [a - b for a, b in zip(p, seed)]
    return max(abs(d[0]), abs(d[1]), abs(d[2]), abs(d[0] - d[1]), abs(d[1] - d[2]),
               abs(d[0] - d[2])) <= t


def region(picture, click, t, contiguous):
    seed = picture[click[1]][click[0]]
    ok = [[admitted(picture[y][x], seed, t) for x in range(W)] for y in range(H)]
    if not contiguous:
        return ok
    out = [[False] * W for _ in range(H)]
    out[click[1]][click[0]] = True
    stack = [click]
    while stack:
        x, y = stack.pop()
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if 0 <= nx < W and 0 <= ny < H and not out[ny][nx] and ok[ny][nx]:
                out[ny][nx] = True
                stack.append((nx, ny))
    return out


ALL = [dict(x=x, y=y) for y in range(H) for x in range(W)]


def selected(app, document):
    samples = app.call('get_selection', document_id=document, points=ALL)['samples']
    out = [[False] * W for _ in range(H)]
    for s in samples:
        out[s['y']][s['x']] = s['coverage'] == 255
    return out


def wand_case(app, folder, depth):
    picture = wand_picture()
    path = write(folder, f'wand-{depth}.png', picture)
    click = (1, 1)
    checked = 0
    for t in (10, 32):
        for contiguous in (True, False):
            want = region(picture, click, t, contiguous)
            document = app.call('open_document', path=str(path))['document']['id']
            if depth == 16:
                app.call('convert_precision', document_id=document, mode='rgba16',
                         structure='preserve')
            app.call('select_magic_wand', document_id=document, x=click[0], y=click[1],
                     tolerance=t, contiguous=contiguous)
            assert selected(app, document) == want, ('wand', depth, t, contiguous)
            app.call('deselect', document_id=document)
            # At 16 bits a colour-matched global fill is `fill_mode color_match`
            # (a bare `contiguous: false` keeps its selection-wide meaning there).
            mode = dict(fill_mode='color_match') if depth == 16 else {}
            app.call('fill', document_id=document, layer=0, x=click[0], y=click[1],
                     color='#00ff00', tolerance=t, contiguous=contiguous, **mode)
            app.call('select_magic_wand', document_id=document, x=click[0], y=click[1],
                     tolerance=0, contiguous=False)
            assert selected(app, document) == want, ('bucket', depth, t, contiguous)
            app.call('close_document', document_id=document, discard_changes=True)
            checked += sum(row.count(True) for row in want)
    return dict(selected_pixels=checked)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='keep the run folder here')
    args = parser.parse_args()
    folder = Path(args.output) if args.output else Path(tempfile.mkdtemp(prefix='rz-v1x-bw-'))
    folder.mkdir(parents=True, exist_ok=True)
    failures = 0
    with App(folder / 'instance', 2 * 1024 ** 3) as app:
        cases = []
        for depth in (8, 16, 32):
            cases.append((f'blur {depth}-bit', lambda depth=depth: blur_case(app, folder, depth)))
        for depth in (8, 16):
            cases.append((f'wand and bucket {depth}-bit',
                          lambda depth=depth: wand_case(app, folder, depth)))
        for name, run in cases:
            try:
                detail = run()
                print(f'PASS {name}: {detail}', flush=True)
            except Exception as error:  # noqa: BLE001 - every case reports
                failures += 1
                print(f'FAIL {name}: {error!r}', flush=True)
    if failures:
        print(f'{failures} CASE(S) FAILED; the run folder is {folder}')
        return 1
    print('ALL CASES PASS')
    return 0


if __name__ == '__main__':
    sys.exit(main())
