#!/usr/bin/env python3
"""HEVC and HDR movie frames through `open_document`, `place_image` and
`add_live_photo_layer` on an owned Debug instance.

Every movie is authored here with ffmpeg and libx265 (lossless) or libx264
from twelve flat colour blocks, so 4:2:0 chroma never reaches a block centre.
The oracle is ffmpeg's bit-exact decode of the same file to its own YCbCr
planes — a different decoder from the app's — turned into RGB here by the
published limited-range matrix. Bounds, fixed before any run: every SDR block
centre within 4 codes at the source depth; every HDR block centre inside the
light obtained by pushing each code ± 4 (10-bit) through BT.2100's PQ or HLG
decode (1.0 = 203 cd/m2) and the BT.2020 to sRGB primaries conversion the
core's intake states. A frame whose blocks are all equal or all zero fails its
case: a zero-filled decode is never a pass.

Cases: SDR HEVC (`hvc1` 8-bit and Main10) opens at its depth and places as a
layer through place_image and add_live_photo_layer (one undo step, undone);
HLG and PQ HEVC Main10 open as 32-bit float documents with an `hdr_open`
record; the refusals (`hev1`, 4:2:2 HEVC, an 8-bit PQ-tagged H.264,
`hdr_photo` "sdr" on an HLG movie, placing an HLG or PQ movie, a Live Photo
pair whose movie is HLG) answer their sentences in-band.

Launches its own instance (the root CLAUDE.md's recipe, plus Preserve
Embedded Profiles so an 8-bit frame keeps its decoded numbers) and never
another. ffmpeg or libx265 missing is NOT RUN with exit 2. Exit 0 when every
case passes. `DR_VERIFY_APP` selects another app.
"""

import json
import math
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

BLOCK, COLUMNS, ROWS = 32, 4, 3
WIDTH, HEIGHT = BLOCK * COLUMNS, BLOCK * ROWS
# Signal values 0…1 per block: greys across the range, mid-level colours,
# and the two HDR reference levels (PQ 0.58 and HLG 0.75 are about 203 cd/m2).
PALETTE = [(0.05, 0.05, 0.05), (0.25, 0.25, 0.25), (0.5, 0.5, 0.5), (0.75, 0.75, 0.75),
           (0.9, 0.9, 0.9), (0.7, 0.2, 0.15), (0.15, 0.6, 0.3), (0.2, 0.25, 0.7),
           (0.5, 0.4, 0.25), (0.35, 0.55, 0.75), (0.8, 0.75, 0.2), (0.58, 0.58, 0.58)]

SDR_TAGS = dict(primaries='bt709', trc='bt709', matrix='bt709')
HLG_TAGS = dict(primaries='bt2020', trc='arib-std-b67', matrix='bt2020nc')
PQ_TAGS = dict(primaries='bt2020', trc='smpte2084', matrix='bt2020nc')

# name: (codec, pixel format, tags, container tag, extra x265 params)
MOVIES = {
    'hevc8-hvc1': ('libx265', 'yuv420p', SDR_TAGS, 'hvc1', ''),
    'hevc8-hev1': ('libx265', 'yuv420p', SDR_TAGS, 'hev1', ''),
    'hevc10-sdr': ('libx265', 'yuv420p10le', SDR_TAGS, 'hvc1', ''),
    'hevc10-hlg': ('libx265', 'yuv420p10le', HLG_TAGS, 'hvc1', ''),
    'hevc10-pq': ('libx265', 'yuv420p10le', PQ_TAGS, 'hvc1',
                  ':master-display=G(13250,34500)B(7500,3000)R(34000,16000)'
                  'WP(15635,16450)L(10000000,1):max-cll=1000,400'),
    'hevc422-10': ('libx265', 'yuv422p10le', SDR_TAGS, 'hvc1', ''),
    'h264-pq8': ('libx264', 'yuv420p', PQ_TAGS, 'avc1', ''),
    # The place cases' target document: an SDR H.264 frame in the same
    # BT.709 space, which opens before and after HEVC support.
    'h264-sdr8': ('libx264', 'yuv420p', SDR_TAGS, 'avc1', ''),
}


def frame_bytes():
    """One rgb48le frame of the palette's blocks."""
    rows = []
    for y in range(HEIGHT):
        row = bytearray()
        for x in range(WIDTH):
            colour = PALETTE[(y // BLOCK) * COLUMNS + x // BLOCK]
            row += struct.pack('<3H', *(round(c * 65535) for c in colour))
        rows.append(bytes(row))
    return b''.join(rows)


def author(folder):
    """Writes every movie in MOVIES under `folder`; returns name → path."""
    raw = folder / 'frame.rgb48'
    raw.write_bytes(frame_bytes() * 5)
    out = {}
    for name, (codec, pix, tags, tag, extra) in MOVIES.items():
        path = folder / f'{name}.mov'
        command = ['ffmpeg', '-y', '-v', 'error', '-f', 'rawvideo', '-pix_fmt', 'rgb48le',
                   '-s', f'{WIDTH}x{HEIGHT}', '-r', '10', '-i', str(raw),
                   '-vf', f"scale=out_color_matrix={tags['matrix'].replace('nc', '')}"
                          f":out_range=tv,format={pix}",
                   '-c:v', codec]
        if codec == 'libx265':
            command += ['-x265-params', f"lossless=1:colorprim={tags['primaries']}"
                        f":transfer={tags['trc']}:colormatrix={tags['matrix']}{extra}"]
        elif codec == 'libx264':
            # A negative case: only its tags matter, so an ordinary 8-bit
            # High-profile stream.
            command += ['-crf', '12', '-x264-params',
                        f"colorprim={tags['primaries']}:transfer={tags['trc']}"
                        f":colormatrix={tags['matrix']}"]
        command += ['-color_primaries', tags['primaries'], '-color_trc', tags['trc'],
                    '-colorspace', tags['matrix'], '-color_range', 'tv', '-tag:v', tag, str(path)]
        subprocess.run(command, check=True)
        out[name] = path
    return out


# ------------------------------------------------------------------ oracle --

# name: (bit depth, horizontal and vertical chroma subsampling)
PLANES = {'yuv420p': (8, 2, 2), 'yuv420p10le': (10, 2, 2), 'yuv422p10le': (10, 2, 1)}
LUMA = {'bt709': (0.2126, 0.0722), 'bt2020': (0.2627, 0.0593)}


def oracle(path, pix, matrix):
    """The signal E' (0…1 per channel) at each block centre: ffmpeg's
    bit-exact decode of frame 0 to its own YCbCr planes, turned into RGB here
    by the published limited-range matrix (BT.709 or BT.2020 non-constant
    luminance), in floating point."""
    depth, sx, sy = PLANES[pix]
    data = subprocess.run(['ffmpeg', '-v', 'error', '-i', str(path), '-frames:v', '1',
                           '-f', 'rawvideo', '-pix_fmt', pix, '-'],
                          check=True, capture_output=True).stdout
    size = 1 if depth == 8 else 2
    cw, ch = WIDTH // sx, HEIGHT // sy

    def sample(plane, width, x, y):
        at = (plane + y * width + x) * size
        return data[at] if size == 1 else struct.unpack('<H', data[at:at + 2])[0]

    scale = 1 << (depth - 8)
    kr, kb = LUMA[matrix]
    kg = 1 - kr - kb
    centres = []
    for by in range(ROWS):
        for bx in range(COLUMNS):
            x, y = bx * BLOCK + BLOCK // 2, by * BLOCK + BLOCK // 2
            luma = (sample(0, WIDTH, x, y) - 16 * scale) / (219 * scale)
            cb = (sample(WIDTH * HEIGHT, cw, x // sx, y // sy) - 128 * scale) / (224 * scale)
            cr = (sample(WIDTH * HEIGHT + cw * ch, cw, x // sx, y // sy) - 128 * scale) / (224 * scale)
            r = luma + 2 * (1 - kr) * cr
            b = luma + 2 * (1 - kb) * cb
            g = (luma - kr * r - kb * b) / kg
            centres.append(tuple(min(1.0, max(0.0, v)) for v in (r, g, b)))
    return centres


# BT.2100 decode to float-document light (1.0 = 203 cd/m2), the core's
# `hdr_transfer` rules written out here from the published constants.
M1, M2 = 2610 / 16384, 2523 / 4096 * 128
C1, C2, C3 = 3424 / 4096, 2413 / 4096 * 32, 2392 / 4096 * 32
HLG_A = 0.17883277


def pq_light(signal):
    e = max(0.0, min(1.0, signal)) ** (1 / M2)
    return 10000 * (max(e - C1, 0.0) / (C2 - C3 * e)) ** (1 / M1) / 203


def hlg_light(signal, luma_row):
    def scene(s):
        s = max(0.0, min(1.0, s))
        b, c = 1 - 4 * HLG_A, 0.5 - HLG_A * math.log(4 * HLG_A)
        return s * s / 3 if s <= 0.5 else (math.exp((s - c) / HLG_A) + b) / 12
    e = [scene(s) for s in signal]
    ys = sum(l * v for l, v in zip(luma_row, e))
    factor = 1000 * ys ** 0.2 if ys > 0 else 0.0
    return [factor * v / 203 for v in e]


def to_xyz(primaries, white=(0.3127, 0.3290)):
    """The RGB → XYZ matrix of chromaticities `primaries` under `white`."""
    def column(x, y):
        return [x / y, 1.0, (1 - x - y) / y]
    m = [column(*p) for p in primaries]  # columns
    w = column(*white)
    s = solve([[m[c][r] for c in range(3)] for r in range(3)], w)
    return [[m[c][r] * s[c] for c in range(3)] for r in range(3)]


def solve(a, v):
    """a⁻¹ · v for a 3×3 matrix (Cramer's rule)."""
    def det(m):
        return (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
                - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))
    d = det(a)
    out = []
    for c in range(3):
        m = [row[:] for row in a]
        for r in range(3):
            m[r][c] = v[r]
        out.append(det(m) / d)
    return out


REC709 = to_xyz([(0.64, 0.33), (0.30, 0.60), (0.15, 0.06)])
REC2020 = to_xyz([(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)])


def bt2020_to_srgb_primaries(rgb):
    xyz = [sum(REC2020[r][c] * rgb[c] for c in range(3)) for r in range(3)]
    return solve(REC709, xyz)


def hdr_light(kind, signal):
    linear = ([pq_light(s) for s in signal] if kind == 'pq'
              else hlg_light(signal, REC2020[1]))
    return bt2020_to_srgb_primaries(linear)


def hdr_bounds(kind, signal, codes=4):
    """Per channel (lo, hi) over every corner of signal ± `codes`/1023."""
    step = codes / 1023
    corners = [hdr_light(kind, [s + d * step for s, d in zip(signal, (dr, dg, db))])
               for dr in (-1, 1) for dg in (-1, 1) for db in (-1, 1)]
    return [(min(c[i] for c in corners) - 1e-4, max(c[i] for c in corners) + 1e-4)
            for i in range(3)]


def tools_present():
    if not shutil.which('ffmpeg'):
        return 'ffmpeg is not on PATH'
    encoders = subprocess.run(['ffmpeg', '-hide_banner', '-encoders'], capture_output=True,
                              text=True).stdout
    for encoder in ('libx265', 'libx264'):
        if encoder not in encoders:
            return f'ffmpeg has no {encoder} encoder'
    return None


# ------------------------------------------------------------------- drive --

def call(app, tool, **arguments):
    """(ok, reply or sentence)."""
    result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
    text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
    if result.get('isError'):
        return False, text
    try:
        return True, json.loads(text)
    except json.JSONDecodeError:
        return True, text


class Case:
    def __init__(self, results, name):
        self.results, self.name, self.failures = results, name, []

    def check(self, label, condition, value=None):
        if not condition:
            self.failures.append(f'{label}: {value!r}'[:700])
        return bool(condition)

    def finish(self):
        state = 'PASS' if not self.failures else 'FAIL'
        self.results[self.name] = state
        print(f'{self.name}: {state}', flush=True)
        for line in self.failures:
            print(f'  {line}', flush=True)


def centres():
    return [(bx * BLOCK + BLOCK // 2, by * BLOCK + BLOCK // 2)
            for by in range(ROWS) for bx in range(COLUMNS)]


def read(app, doc, x, y, layer=None):
    """RGB at (x, y) of the composite (or one layer) in the document's own
    numbers: 8-bit codes, 16-bit codes or float light."""
    scope = dict(source='layer', layer=layer) if layer is not None else dict(source='composite')
    ok, reply = call(app, 'sample_pixel', document_id=doc, x=x, y=y, **scope)
    assert ok, reply
    for key in ('center_rgba32', 'center_rgba_codes'):
        if key in reply:
            return list(reply[key][:3])
    return [reply['r'], reply['g'], reply['b']]


def live(values):
    """A decode that is not zero-filled or flat."""
    return len({tuple(v) for v in values}) > 1 and any(any(c) for c in values)


def sdr_case(app, results, name, path, signal, maximum, extra=None):
    """Opens `path`; each block centre within 4 source codes of `signal`."""
    case = Case(results, f'{name}-open')
    tolerance = 4 * maximum / (255 if '8' in name.split('-')[0] else 1023)
    ok, reply = call(app, 'open_document', path=str(path), **(extra or {}))
    doc = None
    if case.check('opens', ok, reply):
        doc = reply['document']['id']
        ok, document = call(app, 'get_document', document_id=doc)
        assert ok, document
        case.check('one layer', len(document['layers']) == 1, document['layers'])
        got = [read(app, doc, x, y) for x, y in centres()]
        case.check('a live decode', live(got), got)
        for index, (value, want) in enumerate(zip(got, signal)):
            expected = [s * maximum for s in want]
            case.check(f'block {index}', all(abs(a - b) <= tolerance for a, b in zip(value, expected)),
                       (value, [round(e, 1) for e in expected]))
    case.finish()
    return doc


def place_case(app, results, name, doc, tool, path, signal):
    """Places `path` into the 8-bit document `doc`: one new layer whose block
    centres are within 4 8-bit codes of `signal`; one undo removes it."""
    case = Case(results, name)
    ok, before = call(app, 'get_document', document_id=doc)
    assert ok, before
    ok, reply = call(app, tool, document_id=doc, path=str(path))
    if case.check('places', ok, reply):
        ok, after = call(app, 'get_document', document_id=doc)
        assert ok, after
        case.check('one layer more', len(after['layers']) == len(before['layers']) + 1,
                   len(after['layers']))
        layer = len(after['layers']) - 1
        got = [read(app, doc, x, y, layer=layer) for x, y in centres()]
        case.check('a live decode', live(got), got)
        for index, (value, want) in enumerate(zip(got, signal)):
            expected = [s * 255 for s in want]
            case.check(f'block {index}', all(abs(a - b) <= 4 for a, b in zip(value, expected)),
                       (value, [round(e, 1) for e in expected]))
        ok, undone = call(app, 'undo', document_id=doc)
        ok, restored = call(app, 'get_document', document_id=doc)
        case.check('undo removes it', len(restored['layers']) == len(before['layers']),
                   len(restored['layers']))
    case.finish()


def hdr_case(app, results, name, path, kind, signal):
    case = Case(results, f'{name}-open')
    ok, reply = call(app, 'open_document', path=str(path))
    if case.check('opens', ok, reply):
        doc = reply['document']['id']
        record = reply.get('hdr_open') or {}
        case.check('hdr_open', record.get('kind') == kind and record.get('rendition') == 'hdr'
                   and record.get('source') == 'movie_frame'
                   and record.get('reference_white') == '203 cd/m2', record)
        if kind == 'pq':
            case.check('static metadata noted', record.get('static_metadata') == 'present, not applied',
                       record)
        ok, document = call(app, 'get_document', document_id=doc)
        assert ok, document
        case.check('a float document', 'float' in document, sorted(document))
        got = [read(app, doc, x, y) for x, y in centres()]
        case.check('a live decode', live(got), got)
        for index, (value, want) in enumerate(zip(got, signal)):
            bounds = hdr_bounds(kind, want)
            case.check(f'block {index}', all(lo <= v <= hi for v, (lo, hi) in zip(value, bounds)),
                       (value, [(round(lo, 5), round(hi, 5)) for lo, hi in bounds]))
    # The oracle tells the two transfers apart: the other one's reading of the
    # same codes falls outside these bounds for most blocks.
    other = 'pq' if kind == 'hlg' else 'hlg'
    apart = sum(1 for want in signal
                if not all(lo <= v <= hi for v, (lo, hi)
                           in zip(hdr_light(other, want), hdr_bounds(kind, want))))
    case.check('the oracle separates PQ from HLG', apart >= 8, apart)
    case.finish()


def refusal_case(app, results, name, tool, needle, **arguments):
    case = Case(results, name)
    ok, reply = call(app, tool, **arguments)
    case.check('refuses in-band', not ok, reply)
    case.check('names why', needle in str(reply), reply)
    case.finish()


def main():
    missing = tools_present()
    if missing:
        print(f'verify_movie_frames: NOT RUN ({missing})')
        return 2
    import phase5_support  # noqa: E402 (after the NOT RUN check: it resolves the app)
    results = {}
    folder = Path(tempfile.mkdtemp(prefix='movie-frames-'))
    movies = author(folder)
    signals = {name: oracle(path, MOVIES[name][1],
                            'bt2020' if MOVIES[name][2]['primaries'] == 'bt2020' else 'bt709')
               for name, path in movies.items()}
    os.environ['DR_BACKGROUND'] = '1'
    # Preserve Embedded Profiles: an 8-bit frame keeps the numbers it decoded
    # to, so the oracle meets the document in the frame's own space.
    with phase5_support.App(folder / 'app', folder / 'recovery',
                            extra_defaults=['-ColorRGBPolicy', 'preserve',
                                            '-OpenHDRPhotosAsHDR', 'YES']) as app:
        sdr_case(app, results, 'hevc8-hvc1', movies['hevc8-hvc1'], signals['hevc8-hvc1'], 255)
        sdr_case(app, results, 'hevc10-sdr', movies['hevc10-sdr'], signals['hevc10-sdr'], 65535,
                 extra=dict(precision='preserve'))
        ok, target = call(app, 'open_document', path=str(movies['h264-sdr8']))
        assert ok, target
        doc = target['document']['id']
        if doc is not None:
            place_case(app, results, 'hevc8-place-image', doc, 'place_image',
                       movies['hevc8-hvc1'], signals['hevc8-hvc1'])
            place_case(app, results, 'hevc10-add-live-photo-layer', doc, 'add_live_photo_layer',
                       movies['hevc10-sdr'], signals['hevc10-sdr'])
        hdr_case(app, results, 'hevc10-hlg', movies['hevc10-hlg'], 'hlg', signals['hevc10-hlg'])
        hdr_case(app, results, 'hevc10-pq', movies['hevc10-pq'], 'pq', signals['hevc10-pq'])
        refusal_case(app, results, 'hev1-refuses', 'open_document', 'hev1',
                     path=str(movies['hevc8-hev1']))
        refusal_case(app, results, 'hevc422-refuses', 'open_document', '4:2:2',
                     path=str(movies['hevc422-10']))
        refusal_case(app, results, 'h264-pq-refuses', 'open_document',
                     'opens from 10-bit HEVC only', path=str(movies['h264-pq8']))
        # A copy: open_document answers an already-open path with that document.
        copy = folder / 'hevc10-hlg-sdr.mov'
        shutil.copyfile(movies['hevc10-hlg'], copy)
        refusal_case(app, results, 'hlg-as-sdr-refuses', 'open_document',
                     'no SDR or 8-bit rendition', path=str(copy), hdr_photo='sdr')
        # A Live Photo pair (same basename) whose movie is HLG refuses as a pair.
        pair = folder / 'pair'
        pair.mkdir()
        shutil.copyfile(movies['hevc10-hlg'], pair / 'live.mov')
        subprocess.run(['ffmpeg', '-v', 'error', '-i', str(movies['hevc8-hvc1']), '-frames:v', '1',
                        str(pair / 'live.jpg')], check=True)
        refusal_case(app, results, 'hlg-pair-refuses', 'open_document',
                     "This Live Photo's movie is HDR (HLG)", path=str(pair / 'live.jpg'))
        if doc is not None:
            refusal_case(app, results, 'hlg-live-photo-refuses', 'add_live_photo_layer',
                         'This movie is HDR (HLG)', document_id=doc, path=str(movies['hevc10-hlg']))
            refusal_case(app, results, 'pq-place-refuses', 'place_image',
                         'This movie is HDR (PQ)', document_id=doc, path=str(movies['hevc10-pq']))
    failed = [name for name, state in results.items() if state != 'PASS']
    print(json.dumps(results, indent=1))
    print('verify_movie_frames: ' + ('PASS' if not failed else f'FAIL ({", ".join(failed)})'))
    return 1 if failed else 0


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--author':
        target = Path(sys.argv[2])
        target.mkdir(parents=True, exist_ok=True)
        authored = author(target)
        print(json.dumps({n: [str(p), oracle(p, MOVIES[n][1], 'bt2020' if MOVIES[n][2]['primaries']
                                             == 'bt2020' else 'bt709')]
                          for n, p in authored.items()}))
        sys.exit(0)
    sys.exit(main())

