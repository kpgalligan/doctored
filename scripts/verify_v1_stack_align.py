#!/usr/bin/env python3
"""MCP drive for the v1 stacks batch: `load_files_into_stack` (File >
Load Files into Stack...) and `auto_align_layers` (Edit > Auto-Align Layers...)
end to end.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its
own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that process.
Every expected number is computed here, never read back from the app.

THE PICTURE. `Picture` is multi-octave value noise defined over the whole
plane: five octaves with lattice cells of 64, 32, 16, 8 and 4 pixels, weights
1, 1/2, ... 1/5, smoothstep interpolation, contrast 1.8 about mid-grey, the
lattice an integer hash of (ix, iy, octave). A is that picture sampled at pixel
centres; B(q) = A(K^-1 q) for a known 3x3 matrix K, sampled from the SAME plane,
so both are opaque everywhere. With A below B, the map from a point of B to its
place in A is K^-1, which is what `auto_align_layers` reports as `homography`.

BOUNDS, fixed before measuring and never widened:
- recovered alignment: max |G(q) - K^-1(q)| <= 2.0 px (Euclidean, canvas
  pixels) over the 5x5 grid q = (i W/4, j H/4), corners included;
- pixels, at 8 and 16 bits: mean |layer - A| over the 5x5 grid of cell centres
  ((i + 1/2) W/5, (j + 1/2) H/5) falls below half its value before the align
  (cell centres, because a warp leaves the canvas edge uncovered);
- the canvas after an align (phase 1, take1 R2): EXACTLY the union of the old
  canvas (at the reply's `canvas.origin`) and the content boxes of the aligned
  layers, read back from `get_document`; no `canvas` key means that union is
  the old canvas. `homography` and `quad` stay in the OLD canvas's
  coordinates, so every rectangle and sample after the align is shifted by
  the origin.

CASES
- STACK at 8 bits, at 16 (the stack, then `convert_precision`) and at 32-bit
  float (`paths` = [float_11f/scene.tiff, A, B]): layer names are the file
  names without extension, bottom-first in `paths` order; the canvas is the
  largest width and height; every layer sits at the top-left at its own size;
  `converted_depth` / `converted_profile`; `working_memory_plan_bytes` is the
  plan this file computes; the history is one "Initial State"; the document
  is unsaved with its top layer active.
- STACK MIXED SIZES: 200x150, 640x480, 700x100 and the first path again give a
  700x480 canvas and four layers.
- ALIGN for three K at 640x480 — translation (14, -9); rotation 2 degrees
  about the centre with scale 1.02 and shift (6.5, -4.25); a perspective,
  identity plus m[6] = 6e-5, m[7] = -4e-5 and shift (5, 3) — at 8, 16 and
  float: the alignment bound; `quad` = G(layer corners); the layer's new
  rectangle is the quad's outward-rounded box at the origin; the canvas is the
  union; the reference is unchanged but for the origin; the history cursor
  moves by one; `undo` restores the layer's rectangle and the canvas; and (8,
  16) the pixel bound.
- SCALED ANALYSIS (8 bits): the same picture at 2560x1920, which Vision sees at
  0.8 (2048x1536), for translation (40, -25) and the rotation with shift
  (26, -17): the same bound in canvas pixels, `method` homography for the
  rotation. A scale applied the wrong way round leaves the translation
  14.4 px off.
- LARGE MOVES (8 bits, 640x480; the registration is two passes, a translation
  and then a homography on the layer shifted by it): a translation of a tenth
  of the width, (64, -40), and the 2 degree rotation with that shift, each
  within the same bound; the rotation's `method` is homography.
- ROTATION ALONE (8 bits, 2048x1536, which Vision sees at its own size): a
  rotation of 5 degrees about the centre with scale 1.02 and NO shift, the
  trial the homographic request alone was kept for (`register`'s step 4). On
  this very picture the standalone probe measured the translational request
  1,907 px off and the two-pass result with it, and the single homographic
  request within 0.22 px. `registration: "translation"` must leave the layer
  `unregistered` with no history step (the translation alone is not vouched
  for); the default registration must then align it within the same 2.0 px
  bound with `method` homography. The reply does not say which step answered:
  that it is step 4 and not the two-pass result rests on the probe's reading
  of the translational request for this picture. No case reaches step 3 (the
  translation alone after the two-pass result is set aside).
- PAST THE REACH (8 bits): a translation of 92.5% of the width, (592, -140),
  so the layers share 48 x 340 px, 5% of the frame, under the agreement
  score's minimum share (`AutoAlign.agreementMinimumShare`, 12.5%): no
  estimate can be vouched for, Vision's or the feature step's. The layer is
  `unregistered`, `changed` is false, the history does not
  move, and the layer's rectangle and its 25 sampled pixels are what they
  were.
- IDENTICAL LAYERS (8 bits): the same file twice, for A and for a featureless
  colour ramp (the admission fixtures' scene, where a smooth ramp looks the
  same under any shift): the second layer is `unchanged`, nothing moves and
  there is no history step.
- REAL PHOTOGRAPH (8 bits): A is the 400x400 window at (56, 56) of the 512x512
  astronaut photograph and B(q) = photograph(K^-1 q) for the 2 degree rotation
  with scale 1.02 and shift (32, -10), sampled bilinearly from the SAME
  photograph, so both are opaque. The same alignment bound and pixel bound.
  The photograph is `DR_STACK_ALIGN_PHOTO`, else
  ~/devel/doctored-fixtures/phase6/photos/portrait.png (NASA, public domain);
  when the file is absent the case prints SKIP.
- TRANSLATION ONLY: `registration: "translation"` recovers the 640x480
  translation within the bound with `method: "translation"`.
- PROJECTIONS (phase 1, 8 bits, 640x480): `projection: "collage"` on a 3
  degree rotation with scale 1.03 and shift (8, -5) answers `method`
  similarity with a similarity matrix (no shear, no perspective) within the
  bound; `"reposition"` recovers the translation with `method` translation;
  `"perspective"` and `"auto"` on the perspective K answer the same map
  (within 0.01 px over the grid) and the same method; every reply names its
  `projection`.
- BOTTOM REFERENCE AND UNION CANVAS (8 and 16 bits): three 960x800 crops of
  one picture shifted 0, +115 and +230 px (12% and 24% of the width), stacked
  bottom-first: `reference` is the bottom layer, the two others align to
  +115 and +230 within the bound, the canvas is the union (about 1190 x 800)
  at the reported origin, the bottom layer's 25 samples are bit-identical at
  the origin, one history step, and one undo restores the canvas and every
  rectangle.
- TOO BIG: a transparent 9990 x 10000 canvas with two 2048x2048 coarse
  value-noise layers at its right edge: the upper one flush right, showing
  what belongs 32 px right of where it sits, the reference 32 px left of flush
  right (512 px layers, 105 px in Vision's frame, registered as the identity),
  so the aligned layer passes the right edge and the union is 10022 x 10000,
  past 100 MP: the refusal is `AutoAlign.tooBigMessage`, the history and the
  canvas do not move. It runs in its own instance at an 8 GiB working limit:
  the 100 MP canvas and its placed layers plan past the 2 GiB pin.
- PROJECTION ARGUMENTS: the older `registration: "auto"` still aligns and
  replies `projection` auto; projection and registration together is refused
  by its sentence with no step. Cylindrical and Spherical are
  `scripts/verify_v1_stack_projection.py`'s.
- CMYK: Auto-Align and Reduce Noise on a CMYK stack are `scripts/verify_cmyk_mode.py` G9's.
- STACK CAMERA RAW: `paths` = [a camera RAW, A]. The RAW takes its default
  development and opens at 16 bits, so the stack is 16-bit, the canvas is the
  RAW's size and A is converted. The RAW is `DR_STACK_ALIGN_RAW`, else
  ~/devel/doctored-fixtures/camera/camera.NEF (3008x2000); SKIP when absent.
- STACK PSD: `paths` = [core/tests/fixtures/psd_10b/layers_basic/source.psd,
  A]. The layered 8x6 PSD lands as ONE layer, its composite, at the top-left
  of a 640x480 8-bit stack.
- STACK HEIC: `paths` = [A, core/tests/fixtures/host_stills_opaque/heic8.heic]
  and the reverse order. Each reply layer's width and height are its
  `get_document` row's, the canvas is their maximum, and the history is one
  "Initial State". That file is 64x48 with no orientation, so its header and
  its decode agree.
- STACK TURNED: the drive writes an 800x200 uncompressed RGB TIFF whose
  Orientation (tag 274) is 6 and converts it to `turned.heic` with
  `/usr/bin/sips -s format heic`. A host still's header reports the STORED
  size, so the stack is planned on an 800x480 canvas; the file decodes
  200x800, and [A, turned] and [turned, A] must both come out on a 640x800
  canvas with the turned layer 200x800 in the reply and in its row, one
  "Initial State". SKIP only when `sips` is absent, cannot write the HEIC, or
  the conversion did not keep the orientation (the HEIC carries no `irot`
  turn, or the layer loads at 800x200, so header and decode agree and nothing
  is exercised).
- EMPTY LAYER ABOVE THE REFERENCE: an empty `new_layer` in `layers` is
  `unregistered` while the other layer aligns.
- LOCKED REFERENCE: a position-locked layer becomes `reference`.
- STACK TINY CANVASES: two 128x128 files at 8 bits, two at
  16 (uncompressed RGB TIFFs written here), then two 16x16 at 8 bits: each
  reply is ok at its depth with 2 layers on the files' canvas. Before the fix
  the place and the rasterize shared ONE job, held to the place's plan, and a
  canvas this small refused ("operation needs up to 5 MiB ...").
- REFUSALS, each with no history step and no new document: an empty layer as
  the reference (lowest index, and again named by `reference`), refused with
  its layer name; two locked layers; a linked layer; a group; one layer; a
  `.rz` path.

Fixtures are written once per output folder (`<output>/fixtures`): pure-Python
noise takes about a minute for the 2560x1920 and 2048x1536 pictures.

Usage: python3 scripts/verify_v1_stack_align.py --output DIR
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import itertools
import json
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import Client, png  # noqa: E402

_INSTANCES = itertools.count()  # one pasteboard name per owned instance

ROOT = Path(__file__).resolve().parents[1]
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
FLOAT_SIZE = (48, 32)
PSD_FIXTURE = ROOT / 'core/tests/fixtures/psd_10b/layers_basic/source.psd'
PSD_SIZE = (8, 6)
HEIC_FIXTURE = ROOT / 'core/tests/fixtures/host_stills_opaque/heic8.heic'
HEIC_SIZE = (64, 48)
# The turned still: what its header stores and what it decodes to.
TURNED_STORED = (800, 200)
TURNED_SIZE = (200, 800)
EXTERNAL = Path.home() / 'devel/doctored-fixtures'
PHOTO = Path(os.environ.get('DR_STACK_ALIGN_PHOTO', EXTERNAL / 'phase6/photos/portrait.png'))
PHOTO_SIZE = (512, 512)
PHOTO_WINDOW = (56, 56, 400, 400)
RAW = Path(os.environ.get('DR_STACK_ALIGN_RAW', EXTERNAL / 'camera/camera.NEF'))
RAW_SIZE = (3008, 2000)
SMALL = (640, 480)
LARGE = (2560, 1920)
FRAME = (2048, 1536)
ANALYSIS_MAX_SIDE = 2048
BOUND_PX = 2.0
MIB = 1024 * 1024


class SlowClient(Client):
    """`Client` with a long timeout: a Debug build aligns a 5 MP stack in one call."""

    timeout = 600


class Skipped(Exception):
    """A case whose external fixture is absent."""


class App:
    """One owned instance, on its own scratch socket."""

    def __init__(self, folder, limit):
        self.folder = Path(folder)
        self.socket = None  # this instance's scratch DR_AGENT_SOCKET, made at __enter__
        self.limit = limit
        self.process = None
        self.log = None
        self.client = None

    def __enter__(self):
        executable = app_under_test.default_app_executable()
        app_under_test.require_fresh_app(executable)
        self.folder.mkdir(parents=True, exist_ok=True)
        self.socket = scratch_socket()
        environment = dict(
            os.environ, DR_BACKGROUND='1', DR_AGENT_SOCKET=str(self.socket), ANTHROPIC_API_KEY='dummy',
            DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=str(self.limit),
            DR_ACTIONS_DIR=str(self.folder / 'actions'),
            DR_RECOVERY_DIR=str(self.folder / 'recovery'),
            DR_PASTEBOARD_NAME=f'rz-verify-v1-stack-align-{os.getpid()}-{next(_INSTANCES)}',
            **preset_environment(self.folder))
        self.log = (self.folder / 'app.log').open('w')
        self.process = subprocess.Popen(
            [str(executable), '-ApplePersistenceIgnoreState', 'YES',
             '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
            env=environment, stdout=self.log, stderr=subprocess.STDOUT)
        self.client = SlowClient(self.socket)
        try:
            wait_ready(self.client, self.process, 30, self.log.name)
            return self
        except BaseException:
            self.__exit__()
            raise

    def __exit__(self, *_):
        # Only the Popen process this instance launched is ever signalled.
        if self.process is not None and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        if self.log is not None:
            self.log.close()
            self.log = None
        remove_scratch(self.socket)

    def call(self, tool, **arguments):
        return self.client.call(tool, **arguments)

    def refused(self, tool, *needles, **arguments):
        text = self.client.call(tool, fail=True, **arguments)
        for needle in needles:
            assert needle in text, (tool, needle, text)
        return text


# ------------------------------------------------------------ the picture --

CELLS = (64, 32, 16, 8, 4)
WEIGHTS = tuple(1 / (index + 1) for index in range(len(CELLS)))
WEIGHT_SUM = sum(WEIGHTS)
CONTRAST = 1.8


def lattice(ix, iy, octave):
    """The noise lattice: an integer hash of the cell, in 0...1."""
    h = (ix * 374761393 + iy * 668265263 + (octave + 1) * 1274126177) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    h ^= h >> 16
    return (h & 0xFFFF) / 65535.0


class Picture:
    """The value noise over the whole plane. The lattice is a pure function of
    the cell; the tables only cache it for the rectangle a render reads."""

    def __init__(self, x0, y0, x1, y1):
        self.bounds = (x0, y0, x1, y1)
        self.octaves = []
        for octave, cell in enumerate(CELLS):
            ox, oy = math.floor(x0 / cell), math.floor(y0 / cell)
            nx, ny = math.floor(x1 / cell) + 2 - ox, math.floor(y1 / cell) + 2 - oy
            table = [[lattice(ox + i, oy + j, octave) for i in range(nx)] for j in range(ny)]
            self.octaves.append((cell, WEIGHTS[octave], table, ox, oy))

    def covers(self, x, y):
        x0, y0, x1, y1 = self.bounds
        return x0 <= x <= x1 and y0 <= y <= y1

    def value(self, sx, sy):
        """The picture at the continuous point (sx, sy), 0...1."""
        total = 0.0
        floor = math.floor
        for cell, weight, table, ox, oy in self.octaves:
            fx, fy = sx / cell, sy / cell
            ix, iy = floor(fx), floor(fy)
            tx, ty = fx - ix, fy - iy
            ux, uy = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
            row, below = table[iy - oy], table[iy - oy + 1]
            i = ix - ox
            top = row[i] + (row[i + 1] - row[i]) * ux
            bottom = below[i] + (below[i + 1] - below[i]) * ux
            total += weight * (top + (bottom - top) * uy)
        v = (total / WEIGHT_SUM - 0.5) * CONTRAST + 0.5
        return 0.0 if v < 0 else 1.0 if v > 1 else v

    def level(self, sx, sy):
        """The 8-bit level a render stores for the point."""
        return int(self.value(sx, sy) * 255 + 0.5)

    def rgb(self, sx, sy):
        grey = self.level(sx, sy)
        return grey, grey, grey


def read_png(path):
    """An 8-bit RGB or RGBA non-interlaced PNG as (width, height, channels, rows)."""
    data = Path(path).read_bytes()
    assert data[:8] == b'\x89PNG\r\n\x1a\n', path
    position, packed, header = 8, b'', None
    while position < len(data):
        length, kind = struct.unpack('>I4s', data[position:position + 8])
        body = data[position + 8:position + 8 + length]
        position += 12 + length
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            packed += body
    width, height, bits, colour, _, _, interlace = header
    assert bits == 8 and colour in (2, 6) and interlace == 0, header
    channels = 3 if colour == 2 else 4
    raw, stride = zlib.decompress(packed), width * channels
    rows, previous, position = [], bytearray(stride), 0
    for _ in range(height):
        kind = raw[position]
        line = bytearray(raw[position + 1:position + 1 + stride])
        position += 1 + stride
        for i in range(stride):
            left = line[i - channels] if i >= channels else 0
            up = previous[i]
            if kind == 1:
                line[i] = (line[i] + left) & 255
            elif kind == 2:
                line[i] = (line[i] + up) & 255
            elif kind == 3:
                line[i] = (line[i] + ((left + up) >> 1)) & 255
            elif kind == 4:
                corner = previous[i - channels] if i >= channels else 0
                estimate = left + up - corner
                a, b, c = abs(estimate - left), abs(estimate - up), abs(estimate - corner)
                line[i] = (line[i] + (left if a <= b and a <= c else up if b <= c else corner)) & 255
        rows.append(line)
        previous = line
    return width, height, channels, rows


class Photograph:
    """A decoded photograph as a picture over the part of the plane it covers,
    bilinear between pixel centres, with its pixel (ox, oy) at the plane's
    origin: the point (x + 1/2, y + 1/2) is exactly pixel (ox + x, oy + y)."""

    def __init__(self, path, origin):
        self.width, self.height, self.channels, self.rows = read_png(path)
        self.origin = origin

    def covers(self, x, y):
        px, py = x + self.origin[0], y + self.origin[1]
        return 0.5 <= px <= self.width - 0.5 and 0.5 <= py <= self.height - 0.5

    def rgb(self, sx, sy):
        fx = min(max(sx + self.origin[0] - 0.5, 0), self.width - 1)
        fy = min(max(sy + self.origin[1] - 0.5, 0), self.height - 1)
        x0, y0 = min(int(fx), self.width - 2), min(int(fy), self.height - 2)
        tx, ty = fx - x0, fy - y0
        top, bottom, n = self.rows[y0], self.rows[y0 + 1], self.channels
        out = []
        for channel in range(3):
            a, b = top[x0 * n + channel], top[(x0 + 1) * n + channel]
            c, d = bottom[x0 * n + channel], bottom[(x0 + 1) * n + channel]
            upper, lower = a + (b - a) * tx, c + (d - c) * tx
            out.append(int(upper + (lower - upper) * ty + 0.5))
        return tuple(out)

    def level(self, sx, sy):
        """The red channel: what `mean_difference` compares."""
        return self.rgb(sx, sy)[0]


# ------------------------------------------------------------ the matrices --

IDENTITY = [1, 0, 0, 0, 1, 0, 0, 0, 1]


def apply(m, x, y):
    """Row-major 3x3 on (x, y): pixels, origin top-left, y down."""
    w = m[6] * x + m[7] * y + m[8]
    return (m[0] * x + m[1] * y + m[2]) / w, (m[3] * x + m[4] * y + m[5]) / w


def inverse(m):
    """The inverse by cofactors."""
    a = [m[4] * m[8] - m[5] * m[7], m[2] * m[7] - m[1] * m[8], m[1] * m[5] - m[2] * m[4],
         m[5] * m[6] - m[3] * m[8], m[0] * m[8] - m[2] * m[6], m[2] * m[3] - m[0] * m[5],
         m[3] * m[7] - m[4] * m[6], m[1] * m[6] - m[0] * m[7], m[0] * m[4] - m[1] * m[3]]
    det = m[0] * a[0] + m[1] * a[3] + m[2] * a[6]
    return [v / det for v in a]


def translation(dx, dy):
    return [1, 0, dx, 0, 1, dy, 0, 0, 1]


def rotation(size, dx, dy, degrees=2):
    """`degrees` (2 unless given) about the picture's centre, scale 1.02, then a shift."""
    angle = math.radians(degrees)
    a, b = 1.02 * math.cos(angle), 1.02 * math.sin(angle)
    cx, cy = size[0] / 2, size[1] / 2
    return [a, -b, cx - a * cx + b * cy + dx, b, a, cy - b * cx - a * cy + dy, 0, 0, 1]


def perspective():
    return [1, 0, 5, 0, 1, 3, 6e-5, -4e-5, 1]


SMALL_K = {
    'translation': translation(14, -9),
    'rotation': rotation(SMALL, 6.5, -4.25),
    'perspective': perspective(),
}
LARGE_K = {
    'translation': translation(40, -25),
    'rotation': rotation(LARGE, 26, -17),
}
# A tenth of the width, alone and with the rotation; and 92.5% of the width,
# which leaves 5% of the frame shared, under the agreement score's minimum.
MOVE_K = {
    'far-translation': translation(64, -40),
    'far-rotation': rotation(SMALL, 64, -40),
    'past-reach': translation(592, -140),
}
PHOTO_K = rotation(PHOTO_WINDOW[2:], 32, -10)


def similarity(size, dx, dy, degrees, scale):
    """`degrees` about the picture's centre at `scale`, then a shift."""
    angle = math.radians(degrees)
    a, b = scale * math.cos(angle), scale * math.sin(angle)
    cx, cy = size[0] / 2, size[1] / 2
    return [a, -b, cx - a * cx + b * cy + dx, b, a, cy - b * cx - a * cy + dy, 0, 0, 1]


COLLAGE_K = similarity(SMALL, 8, -5, 3, 1.03)
# Three crops of one picture, each shifted right by 12% of the width more.
CROP = (960, 800)
CROP_SHIFTS = (0, 115, 230)
# Too big: a 9990x10000 canvas and two 2048x2048 layers at the right edge,
# drawn from the picture at an eighth of its scale so the structure survives
# Vision's 0.2 analysis scale. The upper one is flush right and shows what
# belongs TILE_SHIFT px right of where it sits; the reference sits TILE_GAP px
# left of flush right. 512x512 layers (105 px in the 2048 analysis frame)
# registered as the identity, flush right or not (`unchanged`), so the
# layers are 2048 px (419 frame px).
HUGE = (9990, 10000)
TILE = (2048, 2048)
TILE_SHIFT = 32
TILE_GAP = 32
# 5 degrees with no shift, at the analysis size itself: the translational
# request does not follow it, so only the homographic request alone answers.
ALONE_K = rotation(FRAME, 0, 0, degrees=5)


def corners(rect):
    x, y, w, h = rect
    return [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]


def corner_grid(size):
    """5x5 over the whole canvas, corners included."""
    return [(i * size[0] / 4, j * size[1] / 4) for j in range(5) for i in range(5)]


def centre_grid(size):
    """5x5 cell centres, as whole pixels."""
    return [(int((i + 0.5) * size[0] / 5), int((j + 0.5) * size[1] / 5))
            for j in range(5) for i in range(5)]


def outward_box(quad):
    """The quad's axis-aligned box rounded outward, a corner within 1e-9 of an
    integer snapped onto it first: (x, y, width, height)."""
    def snap(v):
        return round(v) if abs(v - round(v)) <= 1e-9 else v
    xs, ys = [snap(p[0]) for p in quad], [snap(p[1]) for p in quad]
    left, top = math.floor(min(xs)), math.floor(min(ys))
    return left, top, math.ceil(max(xs)) - left, math.ceil(max(ys)) - top


# --------------------------------------------------------------- fixtures --


def render(picture, size, source_map):
    """`size` RGBA bytes: pixel (x, y) is the picture at source_map(x + 1/2, y + 1/2)."""
    width, height = size
    for x, y in corners((0, 0, width, height)):
        sx, sy = apply(source_map, x, y)
        assert picture.covers(sx - 8, sy - 8) and picture.covers(sx + 8, sy + 8), (x, y, sx, sy)
    m0, m1, m2, m3, m4, m5, m6, m7, m8 = source_map
    rgb = picture.rgb
    out = bytearray(b'\xff' * (width * height * 4))
    for y in range(height):
        qy = y + 0.5
        ax, ay, aw = m1 * qy + m2, m4 * qy + m5, m7 * qy + m8
        base = y * width * 4
        for x in range(width):
            qx = x + 0.5
            w = m6 * qx + aw
            p = base + 4 * x
            out[p], out[p + 1], out[p + 2] = rgb((m0 * qx + ax) / w, (m3 * qx + ay) / w)
    return bytes(out)

def fixture(folder, name, picture, size, source_map):
    """Writes `<folder>/<name>.png` once per output folder."""
    path = Path(folder) / f'{name}.png'
    if not path.is_file():
        started = time.monotonic()
        staged = path.with_suffix('.partial')
        png(staged, size[0], size[1], render(picture, size, source_map))
        staged.rename(path)
        print(f'fixture {path.name}: {time.monotonic() - started:.1f} s', flush=True)
    return path


def ramp(folder, name, size):
    """A featureless colour ramp, the admission fixtures' scene without its
    noise: R = 40 + 160 x/(w-1), G = 30 + 170 y/(h-1), B = 200 - 150 x/(w-1)."""
    path = Path(folder) / f'{name}.png'
    if not path.is_file():
        width, height = size
        out = bytearray(b'\xff' * (width * height * 4))
        for y in range(height):
            green = round(30 + 170 * y / (height - 1))
            for x in range(width):
                u, p = x / (width - 1), (y * width + x) * 4
                out[p], out[p + 1], out[p + 2] = round(40 + 160 * u), green, round(200 - 150 * u)
        png(path, width, height, bytes(out))
    return path


def flat(folder, name, size, rgba):
    path = Path(folder) / f'{name}.png'
    if not path.is_file():
        png(path, size[0], size[1], bytes(rgba) * (size[0] * size[1]))
    return path


# --------------------------------------------------------------- plumbing --


def load(app, paths, **extra):
    return app.call('load_files_into_stack', paths=[str(path) for path in paths], **extra)


def close(app, document):
    app.call('close_document', document_id=document, discard_changes=True)


def history(app, document):
    return app.call('list_history', document_id=document)

def cursor(app, document):
    return history(app, document)['cursor']


def rows(app, document):
    return app.call('get_document', document_id=document)['layers']


def rect(row):
    return row['offset_x'], row['offset_y'], row['width'], row['height']


def documents(app):
    return sorted(entry['id'] for entry in app.call('list_documents')['documents'])


def plan_bytes(entries, bytes_per_pixel):
    """`StackLoad.planBytes`, restated: every layer at the stack's depth, the
    stack's charge of that depth per layer pixel (10 bytes at 8 bits, the
    8-bit place charge; 3 at 16 and 5 at float, the measured typed-layer
    charges), the largest single decode charge (3 x encoded + a x pixels +
    16 MiB, a = 10 for an 8-bit PNG and 60 for a float TIFF), and the canvas's
    open at 34 bytes a pixel scaled by bytes-per-pixel / 4. `entries` is
    [(path, (w, h), a)]."""
    width = max(size[0] for _, size, _ in entries)
    height = max(size[1] for _, size, _ in entries)
    per_pixel = bytes_per_pixel + {4: 10, 8: 3, 16: 5}[bytes_per_pixel]
    layers = sum(size[0] * size[1] * per_pixel for _, size, _ in entries)
    transient = max(3 * Path(path).stat().st_size + a * size[0] * size[1] + 16 * MIB
                    for path, size, a in entries)
    return layers + transient + (bytes_per_pixel // 4) * 34 * width * height


def check_stack(app, reply, entries, depth_name, bytes_per_pixel, plan=True):
    """What every load must report, and what the new document must be.
    `plan` false skips the plan's own arithmetic (a RAW's or a PSD's decode
    charge is not restated here) and holds the plan to the layers' bytes."""
    paths = [Path(path) for path, _, _ in entries]
    sizes = [size for _, size, _ in entries]
    canvas = (max(size[0] for size in sizes), max(size[1] for size in sizes))
    assert reply['ok'] is True, reply
    assert reply['depth'] == depth_name, reply['depth']
    assert reply['canvas'] == dict(width=canvas[0], height=canvas[1]), reply['canvas']
    assert [layer['index'] for layer in reply['layers']] == list(range(len(paths)))
    assert [layer['name'] for layer in reply['layers']] == [path.stem for path in paths]
    assert [layer['path'] for layer in reply['layers']] == [str(path) for path in paths]
    assert [(layer['width'], layer['height']) for layer in reply['layers']] == sizes
    if plan:
        assert reply['working_memory_plan_bytes'] == plan_bytes(entries, bytes_per_pixel), (
            reply['working_memory_plan_bytes'], plan_bytes(entries, bytes_per_pixel))
    else:
        own = sum(size[0] * size[1] * bytes_per_pixel for size in sizes)
        assert reply['working_memory_plan_bytes'] > own, (reply['working_memory_plan_bytes'], own)
    summary = reply['document']
    document = summary['id']
    assert (summary['width'], summary['height']) == canvas, summary
    assert summary['layer_count'] == len(paths), summary
    assert summary['active_layer'] == len(paths) - 1, 'the top layer is active'
    assert summary['unsaved_changes'] is True and summary['path'] is None, summary
    assert summary['precision'] == depth_name, summary
    listed = rows(app, document)
    assert [row['name'] for row in listed] == [path.stem for path in paths], listed
    for row, size in zip(listed, sizes):
        assert rect(row) == (0, 0, size[0], size[1]), (row['name'], rect(row))
    states = history(app, document)
    assert [state['name'] for state in states['states']] == ['Initial State'], states['states']
    assert states['cursor'] == 0
    return document


def pair(app, a, b):
    """A below B, as a stack; the document id."""
    return load(app, [a, b])['document']['id']


def mean_difference(app, document, layer, picture, size, depth, origin=(0, 0)):
    """mean |layer - A| over the cell-centre grid, in 8-bit levels; `origin`
    is where the old canvas's top-left sits after a grown canvas."""
    total = 0.0
    points = centre_grid(size)
    for x, y in points:
        codes = app.call('sample_pixel', document_id=document, source='layer', layer=layer,
                         x=x + origin[0], y=y + origin[1])['center_rgba_codes']
        got = codes[0] / 257 if depth == 16 else codes[0]
        total += abs(got - picture.level(x + 0.5, y + 0.5))
    return total / len(points)


def canvas_after(app, document, reply, size, layers):
    """The canvas an align left, (ox, oy, width, height) with (ox, oy) where
    the old canvas's top-left sits in the new one: the reply's `canvas` key,
    held to the summary and to the union of the old canvas and the content
    boxes of `layers` in the new document."""
    summary = reply['document']
    width, height = summary['width'], summary['height']
    grown = reply.get('canvas')
    if grown is None:
        assert (width, height) == tuple(size), ('no canvas key, so the canvas is unchanged',
                                                summary)
        ox, oy = 0, 0
    else:
        assert (grown['width'], grown['height']) == (width, height), (grown, summary)
        ox, oy = grown['origin']['x'], grown['origin']['y']
        assert ox >= 0 and oy >= 0 and (width, height) != tuple(size), grown
    listed = rows(app, document)
    x0, y0, x1, y1 = ox, oy, ox + size[0], oy + size[1]
    for index in layers:
        row = listed[index]
        if 'content_x' not in row:
            continue
        x0, y0 = min(x0, row['content_x']), min(y0, row['content_y'])
        x1 = max(x1, row['content_x'] + row['content_width'])
        y1 = max(y1, row['content_y'] + row['content_height'])
    assert (x0, y0, x1, y1) == (0, 0, width, height), (
        'the canvas is the union of the old canvas and the layers', (x0, y0, x1, y1),
        (width, height), (ox, oy))
    return ox, oy, width, height


def shifted(box, ox, oy):
    return box[0] + ox, box[1] + oy, box[2], box[3]


def canvas_size(app, document):
    summary = app.call('get_document', document_id=document)
    return summary['width'], summary['height']


def alignment_error(g, wanted, size):
    assert len(g) == 9 and all(isinstance(v, (int, float)) for v in g), g
    worst = 0.0
    for x, y in corner_grid(size):
        gx, gy = apply(g, x, y)
        wx, wy = apply(wanted, x, y)
        worst = max(worst, math.hypot(gx - wx, gy - wy))
    return worst


# The feature step's sizes (masters in core/src/feature_points.rs,
# feature_register.rs and align_distortion.rs): the feature cap, a feature (x, y
# f64 and 64 f32), a corner candidate (three f32), ANMS's candidates a feature
# and a radius (f64), a match pair (four f64), the lens test's match record (a
# u32 and four [f64; 2], 8-aligned) and the host's inlier buffer (four f64 a pair).
MAX_FEATURES, FEATURE, CANDIDATE, CANDIDATES_PER_FEATURE = 2000, 16 + 4 * 64, 12, 4
PAIR, LENS_MATCH = 32, 72
MATCHING = 16 + 16 + 2 * PAIR + 8  # a row, a column, a pair twice, an inlier index


def feature_scratch(width, height):
    """`AutoAlign.featureScratch`: two grey copies, the inlier buffer and the
    core's peak (`feature_register::scratch_bytes`): one detection (six f32
    planes of the finest level, the corner candidates of a quarter of its
    pixels twice over, ANMS and the features; a frame whose longer side is
    under 512 px is detected on its 2x upsampled level first, four times the
    pixels, with level 0's plane beside it), the other frame's
    features, and the larger of the matching and the lens test (the kept
    pairs and `pair_coefficient_bytes`)."""
    pixels = width * height
    finest, beside = (4 * pixels, pixels) if max(width, height) < 512 else (pixels, 0)
    detect = ((finest * 6 + beside) * 4 + (finest // 4) * 2 * CANDIDATE
              + MAX_FEATURES * CANDIDATES_PER_FEATURE * 16 + MAX_FEATURES * FEATURE)
    lens = MAX_FEATURES * PAIR + MAX_FEATURES * (LENS_MATCH + PAIR)
    core = detect + MAX_FEATURES * FEATURE + max(MAX_FEATURES * MATCHING, lens)
    return 2 * pixels + 4 * MAX_FEATURES * 8 + core


def registration_plan(size, depth, largest=None):
    """Two analysis frames, each 8 bytes a pixel plus 32 MiB, the feature
    step's working bytes at the analysis frame (`feature_scratch`), and for a
    16-bit or float document the thumbnail transient of the largest layer
    analysed: 19 bytes a layer pixel at 16 bits (the 16-bit place charge) and
    16 at float (the layer's own bytes)."""
    scale = min(1, ANALYSIS_MAX_SIDE / max(size))
    width, height = max(1, round(scale * size[0])), max(1, round(scale * size[1]))
    layer = largest or size
    transient = layer[0] * layer[1] * {8: 0, 16: 19, 'float': 16}[depth]
    return 2 * (width * height * 8 + 32 * MIB) + feature_scratch(width, height) + transient


# ----------------------------------------------------------------- cases --

def stack_case(app, fixtures, depth):
    a, b = fixtures['a'], fixtures['b-translation']
    if depth == 'float':
        entries = [(fixtures['scene'], FLOAT_SIZE, 60), (a, SMALL, 10), (b, SMALL, 10)]
        reply = load(app, [path for path, _, _ in entries])
        document = check_stack(app, reply, entries, 'rgba32_float', 16)
        scene, first, second = reply['layers']
        assert scene['source_depth'] == 32 and scene['converted_depth'] is False, scene
        for layer in (first, second):
            assert layer['source_depth'] == 8 and layer['converted_depth'] is True, layer
            assert isinstance(layer['converted_profile'], bool), layer
        assert isinstance(reply.get('note'), str) and first['name'] in reply['note'], reply.get('note')
        assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    else:
        entries = [(a, SMALL, 10), (b, SMALL, 10)]
        reply = load(app, [path for path, _, _ in entries])
        document = check_stack(app, reply, entries, 'rgba8', 4)
        for layer in reply['layers']:
            assert layer['source_depth'] == 8, layer
            assert layer['converted_depth'] is False and layer['converted_profile'] is False, layer
            assert 'disclosures' not in layer, layer
        assert 'note' not in reply, reply.get('note')
        if depth == 16:
            app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
            assert app.call('get_precision', document_id=document)['bits_per_channel'] == 16
            listed = rows(app, document)
            assert [row['name'] for row in listed] == [Path(a).stem, Path(b).stem], listed
            assert [rect(row) for row in listed] == [(0, 0) + SMALL] * 2, listed
    detail = dict(plan_bytes=reply['working_memory_plan_bytes'], profile=reply['profile'])
    close(app, document)
    return detail


def stack_mixed(app, fixtures):
    small, wide, a = fixtures['small'], fixtures['wide'], fixtures['a']
    entries = [(small, (200, 150), 10), (a, SMALL, 10), (wide, (700, 100), 10),
               (small, (200, 150), 10)]
    reply = load(app, [path for path, _, _ in entries], name='Mixed Stack')
    document = check_stack(app, reply, entries, 'rgba8', 4)
    assert reply['canvas'] == dict(width=700, height=480)
    assert reply['document']['title'] == 'Mixed Stack', reply['document']
    close(app, document)
    return dict(canvas=reply['canvas'])


def align_case(app, fixtures, picture, key, k, depth, size, registration=None, method=None,
               projection=None):
    """Load A below B(K), align B to A and hold the reply to K^-1."""
    a, b = fixtures[f'{key[0]}'], fixtures[f'{key[1]}']
    if depth == 'float':
        document = load(app, [fixtures['scene'], a, b])['document']['id']
        reference, moving = 1, 2
    else:
        document = pair(app, a, b)
        reference, moving = 0, 1
        if depth == 16:
            app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    listed = rows(app, document)
    start_rect, reference_rect = rect(listed[moving]), rect(listed[reference])
    assert start_rect == (0, 0) + size and reference_rect == (0, 0) + size, listed
    start = cursor(app, document)
    before = mean_difference(app, document, moving, picture, size, depth) if depth != 'float' else None
    arguments = dict(document_id=document, layers=[reference, moving])
    if registration:
        arguments['registration'] = registration
    if projection:
        arguments['projection'] = projection
    reply = app.call('auto_align_layers', **arguments)
    assert reply['ok'] is True and reply['changed'] is True, reply
    expected = projection or ('reposition' if registration == 'translation' else 'auto')
    assert reply['projection'] == expected, (reply['projection'], expected)
    assert reply['reference'] == reference, reply
    assert [entry['layer'] for entry in reply['aligned']] == [moving], reply
    assert reply['unchanged'] == [] and reply['unregistered'] == [], reply
    assert reply['registration_plan_bytes'] == registration_plan(size, depth), reply
    assert reply['working_memory_plan_bytes'] >= reply['registration_plan_bytes'], reply
    entry = reply['aligned'][0]
    assert entry['name'] == Path(b).stem, entry
    if method:
        assert entry['method'] == method, entry
    g = entry['homography']
    worst = alignment_error(g, inverse(k), size)
    assert worst <= BOUND_PX, (f'max |G(q) - K^-1(q)| = {worst:.3f} px over the 5x5 grid, '
                               f'bound {BOUND_PX}; method {entry["method"]}; G = {g}')
    quad = [tuple(point) for point in entry['quad']]
    assert len(quad) == 4, quad
    for got, corner in zip(quad, corners(start_rect)):
        want = apply(g, corner[0], corner[1])
        assert math.hypot(got[0] - want[0], got[1] - want[1]) <= 1e-6, (got, want)
    ox, oy, width, height = canvas_after(app, document, reply, size, [reference, moving])
    assert cursor(app, document) == start + 1, 'one history step'
    listed = rows(app, document)
    assert rect(listed[reference]) == shifted(reference_rect, ox, oy), 'the reference is unchanged'
    assert rect(listed[moving]) == shifted(outward_box(quad), ox, oy), (
        rect(listed[moving]), outward_box(quad), (ox, oy))
    assert [row['name'] for row in listed][reference:moving + 1] == [Path(a).stem, Path(b).stem]
    detail = dict(worst_px=round(worst, 3), method=entry['method'], homography=g,
                  canvas=(width, height), origin=(ox, oy))
    if before is not None:
        after = mean_difference(app, document, moving, picture, size, depth, origin=(ox, oy))
        assert after < before / 2, (f'mean |layer - A| was {before:.2f} levels and is '
                                    f'{after:.2f}; it must fall below half')
        detail.update(mean_before=round(before, 2), mean_after=round(after, 2))
    app.call('undo', document_id=document)
    assert cursor(app, document) == start, 'undo walks the one step back'
    assert rect(rows(app, document)[moving]) == start_rect, 'undo restores the layer rectangle'
    assert canvas_size(app, document) == tuple(size), 'undo restores the canvas'
    close(app, document)
    return detail


def rotation_alone(app, fixtures, picture):
    """5 degrees with no shift: the translation alone is not vouched for, and
    the default registration aligns the layer with a homography."""
    a, b = fixtures['frame-a'], fixtures['frame-b-alone']
    document = pair(app, a, b)
    start, start_rect = cursor(app, document), rect(rows(app, document)[1])
    reply = app.call('auto_align_layers', document_id=document, layers=[0, 1],
                     registration='translation')
    assert reply['ok'] is True and reply['changed'] is False, reply
    assert reply['aligned'] == [] and reply['unchanged'] == [], reply
    assert [(entry['layer'], entry['name']) for entry in reply['unregistered']] == [
        (1, Path(b).stem)], reply
    assert 'does not match the reference' in reply['unregistered'][0]['reason'], reply
    assert cursor(app, document) == start, 'no history step'
    assert rect(rows(app, document)[1]) == start_rect, 'the layer did not move'
    close(app, document)
    return align_case(app, fixtures, picture, ('frame-a', 'frame-b-alone'), ALONE_K, 8, FRAME,
                      method='homography')


def past_reach(app, fixtures):
    """A shift Vision does not follow: the layer is named as not registered
    and nothing about it changes."""
    a, b = fixtures['a'], fixtures['b-past-reach']
    document = pair(app, a, b)
    start, start_rect = cursor(app, document), rect(rows(app, document)[1])

    def sampled():
        return [app.call('sample_pixel', document_id=document, source='layer', layer=1,
                         x=x, y=y)['center_rgba_codes'] for x, y in centre_grid(SMALL)]

    before = sampled()
    reply = app.call('auto_align_layers', document_id=document, layers=[0, 1])
    assert reply['ok'] is True and reply['changed'] is False, reply
    assert reply['aligned'] == [] and reply['unchanged'] == [], reply
    assert [(entry['layer'], entry['name']) for entry in reply['unregistered']] == [
        (1, Path(b).stem)], reply
    assert 'does not match the reference' in reply['unregistered'][0]['reason'], reply
    assert cursor(app, document) == start, 'no history step'
    assert rect(rows(app, document)[1]) == start_rect, 'the layer did not move'
    assert sampled() == before, 'the layer pixels are unchanged'
    close(app, document)
    return dict(unregistered=reply['unregistered'])


def identical(app, fixtures, key):
    """The same file twice: the second layer is already where it belongs.
    For the ramp this is the case a translation fitted to nothing must not
    move: a smooth ramp looks the same under any shift."""
    document = pair(app, fixtures[key], fixtures[key])
    start, start_rect = cursor(app, document), rect(rows(app, document)[1])
    reply = app.call('auto_align_layers', document_id=document, layers=[0, 1])
    assert reply['ok'] is True and reply['changed'] is False, reply
    assert reply['aligned'] == [] and reply['unregistered'] == [], reply
    assert [entry['layer'] for entry in reply['unchanged']] == [1], reply
    assert cursor(app, document) == start, 'no history step'
    assert rect(rows(app, document)[1]) == start_rect, 'the layer did not move'
    close(app, document)
    return dict(unchanged=reply['unchanged'])


def stack_raw(app, fixtures):
    """A camera RAW first: a 16-bit stack on the RAW's canvas."""
    if not RAW.is_file():
        raise Skipped(f'{RAW} not found')
    a = fixtures['a']
    entries = [(RAW, RAW_SIZE, None), (a, SMALL, None)]
    reply = load(app, [RAW, a])
    document = check_stack(app, reply, entries, 'rgba16', 8, plan=False)
    raw, frame = reply['layers']
    assert raw['source_depth'] == 16 and raw['converted_depth'] is False, raw
    assert frame['source_depth'] == 8 and frame['converted_depth'] is True, frame
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 16
    assert isinstance(reply.get('note'), str) and frame['name'] in reply['note'], reply.get('note')
    detail = dict(canvas=reply['canvas'], plan_bytes=reply['working_memory_plan_bytes'],
                  profile=reply['profile'], note=reply.get('note'))
    close(app, document)
    return detail


def stack_psd(app, fixtures):
    """A layered PSD first: it lands as one layer, its composite."""
    a = fixtures['a']
    entries = [(PSD_FIXTURE, PSD_SIZE, None), (a, SMALL, None)]
    reply = load(app, [PSD_FIXTURE, a])
    document = check_stack(app, reply, entries, 'rgba8', 4, plan=False)
    psd, frame = reply['layers']
    assert psd['source_depth'] == 8 and psd['converted_depth'] is False, psd
    assert frame['source_depth'] == 8 and frame['converted_depth'] is False, frame
    detail = dict(canvas=reply['canvas'], plan_bytes=reply['working_memory_plan_bytes'],
                  profile=reply['profile'], note=reply.get('note'))
    close(app, document)
    return detail


def stack_heic(app, fixtures):
    """A host still beside a PNG, in both orders: each layer's reported size
    is its row's, and the canvas is their maximum."""
    a = fixtures['a']
    seen = {}
    for name, entries in (('png first', [(a, SMALL, None), (HEIC_FIXTURE, HEIC_SIZE, None)]),
                          ('heic first', [(HEIC_FIXTURE, HEIC_SIZE, None), (a, SMALL, None)])):
        reply = load(app, [path for path, _, _ in entries])
        document = check_stack(app, reply, entries, 'rgba8', 4, plan=False)
        listed = rows(app, document)
        for layer, row in zip(reply['layers'], listed):
            assert (layer['width'], layer['height']) == (row['width'], row['height']), (layer, row)
        seen[name] = dict(canvas=reply['canvas'], plan_bytes=reply['working_memory_plan_bytes'])
        close(app, document)
    return seen


def turned_tiff(path):
    """An 800x200 uncompressed 8-bit RGB TIFF, little-endian, whose
    Orientation (tag 274) is 6: the stored rows are the picture turned, so it
    displays 200 wide and 800 tall."""
    width, height = TURNED_STORED
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            pixels += bytes(((x * 255) // (width - 1), (y * 255) // (height - 1), (x * 7 + y * 3) % 256))
    entries = [
        (256, 4, 1, width), (257, 4, 1, height), (258, 3, 3, None), (259, 3, 1, 1),
        (262, 3, 1, 2), (273, 4, 1, None), (274, 3, 1, 6), (277, 3, 1, 3),
        (278, 4, 1, height), (279, 4, 1, len(pixels)), (284, 3, 1, 1),
    ]
    directory = 8
    bits_at = directory + 2 + 12 * len(entries) + 4
    data_at = bits_at + 6
    out = bytearray(b'II*\x00' + struct.pack('<I', directory) + struct.pack('<H', len(entries)))
    for tag, kind, count, value in entries:
        if tag == 258:
            value = bits_at
        elif tag == 273:
            value = data_at
        out += struct.pack('<HHI', tag, kind, count)
        out += struct.pack('<HH', value, 0) if kind == 3 and count == 1 else struct.pack('<I', value)
    out += struct.pack('<I', 0) + struct.pack('<HHH', 8, 8, 8) + pixels
    Path(path).write_bytes(bytes(out))


def tiff16(path, size, seed):
    """A `size` uncompressed 16-bit RGB TIFF, little-endian, one strip."""
    width, height = size
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            pixels += struct.pack('<HHH', (x * 997 + seed * 4099) % 65536,
                                  (y * 1499 + seed) % 65536, (x * 31 + y * 17) % 65536)
    entries = [
        (256, 4, 1, width), (257, 4, 1, height), (258, 3, 3, None), (259, 3, 1, 1),
        (262, 3, 1, 2), (273, 4, 1, None), (277, 3, 1, 3),
        (278, 4, 1, height), (279, 4, 1, len(pixels)), (284, 3, 1, 1),
    ]
    directory = 8
    bits_at = directory + 2 + 12 * len(entries) + 4
    data_at = bits_at + 6
    out = bytearray(b'II*\x00' + struct.pack('<I', directory) + struct.pack('<H', len(entries)))
    for tag, kind, count, value in entries:
        if tag == 258:
            value = bits_at
        elif tag == 273:
            value = data_at
        out += struct.pack('<HHI', tag, kind, count)
        out += struct.pack('<HH', value, 0) if kind == 3 and count == 1 else struct.pack('<I', value)
    out += struct.pack('<I', 0) + struct.pack('<HHH', 16, 16, 16) + pixels
    Path(path).write_bytes(bytes(out))
    return Path(path)


def tiny_canvases(app, folder):
    """Two-file stacks of 128 px canvases at 8 and 16 bits and of 16 px ones."""
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    detail = {}
    for label, size, bits in (('128-8', (128, 128), 8), ('128-16', (128, 128), 16),
                              ('16-8', (16, 16), 8)):
        if bits == 8:
            paths = [flat(folder, f'tiny-{label}-{k}', size, (40 + 60 * k, 90, 200 - 50 * k, 255))
                     for k in range(2)]
        else:
            paths = [tiff16(folder / f'tiny-{label}-{k}.tif', size, k) for k in range(2)]
        reply = load(app, paths)
        assert reply['ok'] is True, reply
        depth = 'rgba8' if bits == 8 else 'rgba16'
        assert reply['depth'] == depth, reply['depth']
        assert reply['canvas'] == dict(width=size[0], height=size[1]), reply['canvas']
        summary = reply['document']
        try:
            assert summary['layer_count'] == 2, summary
            listed = rows(app, summary['id'])
            assert [rect(row) for row in listed] == [(0, 0) + size] * 2, listed
        finally:
            close(app, summary['id'])
        detail[label] = reply['working_memory_plan_bytes']
    return detail


def stack_turned(app, fixtures, folder):
    """A host still stored turned (orientation 6): its header says 800x200 and
    it decodes 200x800, so the stack is planned on the wrong canvas (800x480)
    and must come out on the real one, 640x800, in both orders."""
    sips = '/usr/bin/sips'
    if not Path(sips).is_file():
        raise Skipped('sips is absent')
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    source, turned = folder / 'turned.tiff', folder / 'turned.heic'
    turned_tiff(source)
    converted = subprocess.run([sips, '-s', 'format', 'heic', str(source), '--out', str(turned)],
                               capture_output=True, text=True, check=False)
    if converted.returncode != 0 or not turned.is_file() or turned.stat().st_size == 0:
        raise Skipped(f'sips could not write the HEIC: {converted.stderr.strip()[:200]}')
    # The HEIC keeps a turn as an `irot` box (one byte after the type: the
    # angle in quarter turns). A conversion that turned the pixels instead
    # writes none, or a zero one: the file then stores 200x800, header and
    # decode agree, and nothing is exercised.
    data = turned.read_bytes()
    at = data.find(b'irot')
    quarter_turns = data[at + 4] & 3 if 0 <= at < len(data) - 4 else 0
    a = fixtures['a']
    seen = dict(irot_quarter_turns=quarter_turns)
    for name, order in (('png first', [a, turned]), ('turned first', [turned, a])):
        reply = load(app, order)
        landed = next(layer for layer in reply['layers'] if layer['name'] == turned.stem)
        if (landed['width'], landed['height']) == TURNED_STORED or quarter_turns == 0:
            close(app, reply['document']['id'])
            raise Skipped(
                f'the orientation did not survive the conversion (irot quarter turns '
                f'{quarter_turns}, the layer loaded at {landed["width"]}x{landed["height"]}), so '
                'header and decode agree')
        entries = [(path, TURNED_SIZE if path == turned else SMALL, None) for path in order]
        document = check_stack(app, reply, entries, 'rgba8', 4, plan=False)
        assert reply['canvas'] == dict(width=640, height=800), reply['canvas']
        row = next(row for row in rows(app, document) if row['name'] == turned.stem)
        assert rect(row) == (0, 0) + TURNED_SIZE, rect(row)
        assert (landed['width'], landed['height']) == TURNED_SIZE, landed
        seen[name] = dict(canvas=reply['canvas'], plan_bytes=reply['working_memory_plan_bytes'])
        close(app, document)
    return seen


def empty_above(app, fixtures):
    document = pair(app, fixtures['a'], fixtures['b-rotation'])
    app.call('new_layer', document_id=document, name='Empty')
    listed = rows(app, document)
    assert [row['name'] for row in listed][2] == 'Empty', listed
    start = cursor(app, document)
    reply = app.call('auto_align_layers', document_id=document, layers=[0, 1, 2])
    assert reply['changed'] is True and reply['reference'] == 0, reply
    assert [entry['layer'] for entry in reply['aligned']] == [1], reply
    assert [(entry['layer'], entry['name']) for entry in reply['unregistered']] == [(2, 'Empty')]
    assert 'no pixels' in reply['unregistered'][0]['reason'], reply['unregistered']
    assert cursor(app, document) == start + 1
    worst = alignment_error(reply['aligned'][0]['homography'], inverse(SMALL_K['rotation']), SMALL)
    assert worst <= BOUND_PX, worst
    close(app, document)
    return dict(unregistered=reply['unregistered'], worst_px=round(worst, 3))


def locked_reference(app, fixtures):
    document = pair(app, fixtures['a'], fixtures['b-rotation'])
    app.call('set_layer_lock', document_id=document, layer=1, locks=['position'])
    start = cursor(app, document)
    reply = app.call('auto_align_layers', document_id=document, layers=[0, 1])
    assert reply['reference'] == 1, reply
    assert [entry['layer'] for entry in reply['aligned']] == [0], reply
    assert cursor(app, document) == start + 1
    # A is the floating layer now, so its points belong at K of themselves.
    worst = alignment_error(reply['aligned'][0]['homography'], SMALL_K['rotation'], SMALL)
    assert worst <= BOUND_PX, worst
    ox, oy, _, _ = canvas_after(app, document, reply, SMALL, [0, 1])
    assert rect(rows(app, document)[1]) == (ox, oy) + SMALL, 'the locked layer did not move'
    close(app, document)
    return dict(reference=reply['reference'], worst_px=round(worst, 3))


def collage_case(app, fixtures, picture):
    """Collage: a similarity, answered as one."""
    detail = align_case(app, fixtures, picture, ('a', 'b-collage'), COLLAGE_K, 8, SMALL,
                        projection='collage', method='similarity')
    g = detail['homography']
    assert abs(g[0] - g[4]) <= 1e-9 and abs(g[1] + g[3]) <= 1e-9, ('a similarity', g)
    assert g[6] == 0 and g[7] == 0 and g[8] == 1, ('no perspective', g)
    return detail


def perspective_is_auto(app, fixtures, picture):
    """Perspective and Auto answer the same registration."""
    k = SMALL_K['perspective']
    seen = {}
    for projection in ('perspective', 'auto'):
        seen[projection] = align_case(app, fixtures, picture, ('a', 'b-perspective'), k, 8, SMALL,
                                      projection=projection)
    apart = alignment_error(seen['perspective']['homography'], seen['auto']['homography'], SMALL)
    assert apart <= 0.01, (f'perspective and auto differ by {apart:.4f} px over the grid', seen)
    assert seen['perspective']['method'] == seen['auto']['method'], seen
    return dict(apart_px=apart, method=seen['auto']['method'])


def bottom_union(app, fixtures, picture, depth):
    """Three crops shifted right by 0, 115 and 230 px: the bottom layer is the
    reference and stays bit-identical, the canvas grows to the union."""
    paths = [fixtures[f'crop-{index}'] for index in range(3)]
    document = load(app, paths)['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    start, before_rows = cursor(app, document), [rect(row) for row in rows(app, document)]
    assert before_rows == [(0, 0) + CROP] * 3, before_rows

    def bottom(origin):
        return [app.call('sample_pixel', document_id=document, source='layer', layer=0,
                         x=x + origin[0], y=y + origin[1])['center_rgba_codes']
                for x, y in centre_grid(CROP)]

    held = bottom((0, 0))
    reply = app.call('auto_align_layers', document_id=document, layers=[0, 1, 2])
    assert reply['ok'] is True and reply['changed'] is True, reply
    assert reply['reference'] == 0 and reply['projection'] == 'auto', reply
    assert [entry['layer'] for entry in reply['aligned']] == [1, 2], reply
    assert reply['unregistered'] == [] and reply['unchanged'] == [], reply
    worst = {}
    for entry, dx in zip(reply['aligned'], CROP_SHIFTS[1:]):
        # Layer i's point q shows the picture at q + (dx, 0): it belongs there.
        worst[entry['layer']] = alignment_error(entry['homography'], translation(dx, 0), CROP)
        assert worst[entry['layer']] <= BOUND_PX, (entry['layer'], worst, entry['homography'])
    assert 'canvas' in reply, 'the canvas grew'
    ox, oy, width, height = canvas_after(app, document, reply, CROP, [0, 1, 2])
    assert abs(width - (CROP[0] + CROP_SHIFTS[2])) <= 2 and abs(height - CROP[1]) <= 2, (
        width, height)
    assert rect(rows(app, document)[0]) == (ox, oy) + CROP, 'the bottom layer did not move'
    assert bottom((ox, oy)) == held, 'the bottom layer is bit-identical'
    assert cursor(app, document) == start + 1, 'one history step'
    app.call('undo', document_id=document)
    assert cursor(app, document) == start, 'undo walks the one step back'
    assert canvas_size(app, document) == CROP, 'undo restores the canvas'
    assert [rect(row) for row in rows(app, document)] == before_rows, 'undo restores the layers'
    close(app, document)
    return dict(canvas=(width, height), origin=(ox, oy),
                worst_px={k: round(v, 3) for k, v in worst.items()})


def too_big(app, fixtures):
    """A union past 100 MP is refused by `tooBigMessage`, and nothing moves."""
    made = app.call('new_document', width=HUGE[0], height=HUGE[1], background='transparent')
    document = made['document']['id']
    try:
        x, y = HUGE[0] - TILE[0], (HUGE[1] - TILE[1]) // 2
        for name, at in (('tile-a', x - TILE_GAP), ('tile-b', x)):
            app.call('place_image', document_id=document, path=str(fixtures[name]), name=name,
                     x=at, y=y)
        listed = rows(app, document)
        assert [row['name'] for row in listed][1:] == ['tile-a', 'tile-b'], listed
        assert [rect(row) for row in listed][1:] == [(x - TILE_GAP, y) + TILE, (x, y) + TILE], listed
        start = cursor(app, document)
        text = app.refused(
            'auto_align_layers',
            'The aligned layers would need a ',
            '-pixel canvas, past the 100-megapixel limit. Nothing was changed.',
            document_id=document, layers=[1, 2])
        assert cursor(app, document) == start, 'a refusal leaves no history step'
        assert canvas_size(app, document) == HUGE, 'the canvas did not change'
        assert [rect(row) for row in rows(app, document)] == [rect(row) for row in listed]
        return dict(refusal=text[:300])
    finally:
        close(app, document)


def projection_arguments(app, fixtures):
    """The older registration still aligns; projection and registration together refuse."""
    document = pair(app, fixtures['a'], fixtures['b-translation'])
    start, seen = cursor(app, document), {}
    seen['both'] = app.refused('auto_align_layers', 'Pass projection or registration, not both.',
                               document_id=document, layers=[0, 1], projection='auto',
                               registration='auto')
    assert cursor(app, document) == start, 'a refusal leaves no history step'
    reply = app.call('auto_align_layers', document_id=document, layers=[0, 1], registration='auto')
    assert reply['changed'] is True and reply['projection'] == 'auto', reply
    worst = alignment_error(reply['aligned'][0]['homography'], inverse(SMALL_K['translation']),
                            SMALL)
    assert worst <= BOUND_PX, worst
    close(app, document)
    seen['legacy worst_px'] = round(worst, 3)
    return seen


def refusals(app, fixtures, folder):
    a, b = fixtures['a'], fixtures['b-rotation']
    seen = {}
    opened = []

    def stack():
        document = pair(app, a, b)
        opened.append(document)
        return document

    def refuse(name, document, *needles, **arguments):
        held, listed = cursor(app, document), documents(app)
        seen[name] = app.refused('auto_align_layers', *needles, document_id=document, **arguments)
        assert cursor(app, document) == held, f'{name}: a refusal leaves no history step'
        assert documents(app) == listed, f'{name}: a refusal opens no document'

    # An empty layer as the reference: the lowest index of the three...
    document = stack()
    app.call('new_layer', document_id=document, name='Empty Reference')
    app.call('reorder_layer', document_id=document, **{'from': 2, 'to': 0})
    assert rows(app, document)[0]['name'] == 'Empty Reference'
    refuse('empty reference, lowest index', document, 'Empty Reference', 'no pixels',
           layers=[0, 1, 2])
    # ...and named by `reference` from the top of the stack.
    document = stack()
    app.call('new_layer', document_id=document, name='Empty On Top')
    assert rows(app, document)[2]['name'] == 'Empty On Top'
    refuse('empty reference, named', document, 'Empty On Top', 'no pixels',
           layers=[0, 1, 2], reference=2)

    document = stack()
    app.call('set_layer_lock', document_id=document, layers=[0, 1], locks=['position'])
    refuse('two locked layers', document, 'position-locked', Path(a).stem, Path(b).stem,
           layers=[0, 1])

    document = stack()
    app.call('link_layers', document_id=document, layers=[0, 1])
    refuse('a linked layer', document, 'linked', layers=[0, 1])

    document = stack()
    app.call('group_layers', document_id=document, layers=[1], name='Holder')
    group = next(row['index'] for row in rows(app, document) if row['name'] == 'Holder')
    refuse('a group', document, 'Holder', 'is a group', layers=[0, group])

    document = stack()
    refuse('one layer', document, 'at least 2', layers=[1])

    native = Path(folder) / 'native-stack.rz'
    app.call('save_copy', document_id=document, path=str(native))
    assert native.is_file()
    listed = documents(app)
    seen['a .rz path'] = app.refused('load_files_into_stack', native.name, 'Doctored document',
                                     paths=[str(a), str(native)])
    assert documents(app) == listed, 'a refused load opens no document'
    seen['one path'] = app.refused('load_files_into_stack', 'at least 2', paths=[str(a)])
    seen['a missing file'] = app.refused('load_files_into_stack', 'absent.png',
                                         paths=[str(a), str(Path(folder) / 'absent.png')])
    assert documents(app) == listed, 'a refused load opens no document'
    for document in opened:
        close(app, document)
    return seen


# ----------------------------------------------------------------- main --

def author(folder):
    """Every fixture, written once per output folder."""
    folder.mkdir(parents=True, exist_ok=True)
    # The cache reaches 640 px left of the frame for the past-reach shift (592, -140);
    # the lattice is a pure function, so no other fixture changes.
    small = Picture(-640, -256, SMALL[0] + 256, SMALL[1] + 256)
    fixtures = {'a': fixture(folder, 'frame-a', small, SMALL, IDENTITY)}
    for name, k in list(SMALL_K.items()) + list(MOVE_K.items()):
        fixtures[f'b-{name}'] = fixture(folder, f'frame-b-{name}', small, SMALL, inverse(k))
    fixtures['b-collage'] = fixture(folder, 'frame-b-collage', small, SMALL, inverse(COLLAGE_K))
    wide = Picture(-256, -256, CROP[0] + CROP_SHIFTS[-1] + 256, CROP[1] + 256)
    for index, dx in enumerate(CROP_SHIFTS):
        fixtures[f'crop-{index}'] = fixture(folder, f'crop-{index}', wide, CROP, translation(dx, 0))
    coarse = Picture(-64, -64, TILE[0] // 8 + 64, TILE[1] // 8 + 64)
    for name, dx in (('tile-a', 0), ('tile-b', TILE_GAP + TILE_SHIFT)):
        fixtures[name] = fixture(folder, name, coarse, TILE, [1 / 8, 0, dx / 8, 0, 1 / 8, 0, 0, 0, 1])
    photo = None
    if PHOTO.is_file():
        photo = Photograph(PHOTO, PHOTO_WINDOW[:2])
        assert (photo.width, photo.height) == PHOTO_SIZE, (photo.width, photo.height)
        window = PHOTO_WINDOW[2:]
        fixtures['photo-a'] = fixture(folder, 'photo-a', photo, window, IDENTITY)
        fixtures['photo-b'] = fixture(folder, 'photo-b', photo, window, inverse(PHOTO_K))
    large = Picture(-512, -512, LARGE[0] + 512, LARGE[1] + 512)
    fixtures['large-a'] = fixture(folder, 'large-a', large, LARGE, IDENTITY)
    for name, k in LARGE_K.items():
        fixtures[f'large-b-{name}'] = fixture(folder, f'large-b-{name}', large, LARGE, inverse(k))
    # `large` covers the 2048x1536 frame and its rotated corners too.
    fixtures['frame-a'] = fixture(folder, 'analysis-a', large, FRAME, IDENTITY)
    fixtures['frame-b-alone'] = fixture(folder, 'analysis-b-alone', large, FRAME, inverse(ALONE_K))
    fixtures['ramp'] = ramp(folder, 'ramp', SMALL)
    fixtures['small'] = flat(folder, 'small', (200, 150), (200, 60, 40, 255))
    fixtures['wide'] = flat(folder, 'wide', (700, 100), (40, 90, 200, 255))
    scene = folder / 'scene.tiff'
    if not scene.is_file():
        shutil.copyfile(FLOAT_FIXTURE, scene)
    fixtures['scene'] = scene
    return fixtures, small, large, photo


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-stack-align-'))
    output.mkdir(parents=True, exist_ok=True)
    assert FLOAT_FIXTURE.is_file(), f'missing float fixture {FLOAT_FIXTURE}'
    assert PSD_FIXTURE.is_file(), f'missing PSD fixture {PSD_FIXTURE}'
    fixtures, small, large, photo = author(output / 'fixtures')

    results, failed, skipped = {}, [], []

    def photograph_case():
        if photo is None:
            raise Skipped(f'{PHOTO} not found')
        return align_case(app, fixtures, photo, ('photo-a', 'photo-b'), PHOTO_K, 8,
                          PHOTO_WINDOW[2:], method='homography')

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except Skipped as reason:
            results[name] = dict(skipped=True, reason=str(reason))
            skipped.append(name)
            print(f'SKIP {name}: {reason}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', 2 * 1024 ** 3) as app:
        for depth in (8, 16, 'float'):
            label = f'{depth}-bit' if depth != 'float' else 'float'
            run(f'stack {label}', lambda: stack_case(app, fixtures, depth))
            for name, k in SMALL_K.items():
                run(f'align {name} {label}',
                    lambda: align_case(app, fixtures, small, ('a', f'b-{name}'), k, depth, SMALL))
        run('stack mixed sizes', lambda: stack_mixed(app, fixtures))
        run('scaled analysis translation',
            lambda: align_case(app, fixtures, large, ('large-a', 'large-b-translation'),
                               LARGE_K['translation'], 8, LARGE))
        run('scaled analysis rotation',
            lambda: align_case(app, fixtures, large, ('large-a', 'large-b-rotation'),
                               LARGE_K['rotation'], 8, LARGE, method='homography'))
        run('large translation',
            lambda: align_case(app, fixtures, small, ('a', 'b-far-translation'),
                               MOVE_K['far-translation'], 8, SMALL))
        run('large translation with a rotation',
            lambda: align_case(app, fixtures, small, ('a', 'b-far-rotation'),
                               MOVE_K['far-rotation'], 8, SMALL, method='homography'))
        run('rotation alone', lambda: rotation_alone(app, fixtures, large))
        run('past the reach', lambda: past_reach(app, fixtures))
        run('identical layers', lambda: identical(app, fixtures, 'a'))
        run('identical featureless layers', lambda: identical(app, fixtures, 'ramp'))
        run('real photograph', photograph_case)
        run('translation only',
            lambda: align_case(app, fixtures, small, ('a', 'b-translation'),
                               SMALL_K['translation'], 8, SMALL, registration='translation',
                               method='translation'))
        run('stack camera RAW', lambda: stack_raw(app, fixtures))
        run('stack PSD', lambda: stack_psd(app, fixtures))
        run('stack heic', lambda: stack_heic(app, fixtures))
        run('stack turned', lambda: stack_turned(app, fixtures, output / 'turned'))
        run('collage', lambda: collage_case(app, fixtures, small))
        run('reposition',
            lambda: align_case(app, fixtures, small, ('a', 'b-translation'),
                               SMALL_K['translation'], 8, SMALL, projection='reposition',
                               method='translation'))
        run('perspective is auto', lambda: perspective_is_auto(app, fixtures, small))
        for depth in (8, 16):
            run(f'bottom reference and union canvas {depth}-bit',
                lambda: bottom_union(app, fixtures, None, depth))
        run('projection arguments', lambda: projection_arguments(app, fixtures))
        run('empty layer above the reference', lambda: empty_above(app, fixtures))
        run('locked reference', lambda: locked_reference(app, fixtures))
        run('stack tiny canvases', lambda: tiny_canvases(app, output / 'tiny'))
        run('refusals', lambda: refusals(app, fixtures, output))
    # A 100 MP canvas and its placed layers plan past the 2 GiB pin, so the
    # too-big case runs in its own instance at 8 GiB (a 16 GB Mac's limit).
    with App(output / 'instance-huge', 8 * 1024 ** 3) as huge:
        run('too big', lambda: too_big(huge, fixtures))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS' + (f' ({len(skipped)} skipped: {", ".join(skipped)})' if skipped else ''))


if __name__ == '__main__':
    main()
