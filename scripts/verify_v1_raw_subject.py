#!/usr/bin/env python3
"""MCP drive for Select Subject, Select People and Sky masks on Camera RAW locals:
`edit_raw_development` operations whose mask is `{op: subject}`,
`{op: people}` or `{op: sky}`, and the Recompute form `{op: mask, command:
{op: subject}}`.

The script owns its one app instance: the Debug app (or `DR_VERIFY_APP`) on
its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, an 8 GiB working limit and private `DR_*_DIR`s
(`verify_v1_lens.App`). It terminates only that process. An absent fixture
FAILS the run. Every document is opened from its own copy of its file, at 16
bits.

Fixtures:
- `~/devel/doctored-fixtures/raw-pixls/nikon-z6/Z6.NEF`: toys on a table.
- a portrait DNG built here: `phase3_fixtures.synthetic_dng(orientation=1,
  samples=…)`, the samples mosaicked from
  `~/devel/doctored-fixtures/phase6/photos/portrait.png` (512×512; `sips -s
  format bmp` into the output folder, each sRGB code decoded to linear by IEC
  61966-2.1, CFA value = round(64 + 959·linear), RGGB).
- a flat DNG built here: `samples` all 300 (nothing for Vision to find).
- a sky DNG built here as the portrait one is, from
  `~/devel/doctored-fixtures/phase6/photos/sky.jpg` (CC0) scaled by `sips -z
  682 1024` first.

What is measured. A generated mask is stored as ordinary native coverage, so it
is read back exactly: `get_raw_local_mask`'s `.drmask` export is the 16-byte
RZLM header (`RZLM`, version 1, width, height, u32 LE) and width·height u16 LE
samples, row 0 top. The picture is compared with a PLAIN development of the
same file through `sample_pixel` (`source: layer`) on a lattice: where the
stored coverage is 0 the local must not have touched the pixel at all, and
where it is nearly full the local's `exposure_offset` must have moved it.

Bounds, fixed before the first run and never widened:
- the lattice is 32×20 (640 points);
- INSIDE: coverage ≥ 58982 (90 % of 65535);
- OUTSIDE: coverage = 0; such a lattice point is code-identical to the plain
  development;
- CORNER: coverage ≤ 655 (1 %) at a named background point;
- CENTRE: coverage ≥ 32768 at a named point on a subject;
- an UNCLIPPED INSIDE point (every plain code in 256…65279) moves at least
  MOVED = 64 codes in some channel under `exposure_offset` 1;
- at least MIN_CLASS = 20 lattice points in each of the OUTSIDE and the moved
  INSIDE class.

Cases:
- SUBJECT (Z6, `create` with `mask: {op: subject}`, `exposure_offset` 1):
  `get_raw_locals` reports `generated: subject`; the plane is the source size;
  CENTRE at 2 or more of the three object centres (0.52, 0.60), (0.72, 0.62),
  (0.31, 0.56) of the frame; CORNER at (0.02, 0.02), (0.98, 0.02) and
  (0.98, 0.98) (the bottom-left corner holds out-of-focus clutter and is not
  asserted); OUTSIDE lattice points equal the plain development exactly and
  unclipped INSIDE points moved; MIN_CLASS points in each class.
- STORED: `redevelop_raw {exposure: -0.25}`, then `save_raw_preset` /
  `apply_raw_preset` of an exposure: `get_raw_locals` (the mask `sha256` in
  it) is equal before and after each and the re-exported `.drmask` bytes are
  identical. `undo` / `redo` restore each exposure with the same locals.
- RZ: `save_copy` to `.rz`, close, reopen: `get_raw_locals` equal, `.drmask`
  bytes identical, the lattice identical, and a further `update` succeeds.
- ON TOP: a brush `add` dab at a corner, `invert`, `update` with density 0.5
  and feather 8, and `update` with a luminance range each succeed;
  `generated` stays `subject`; the mask `sha256` changes for the brush and the
  invert only.
- RECOMPUTE: `mask` with `command: {op: subject}`: the three CORNER points
  and the CENTRE rule hold again and `generated` is `subject`. On a radial
  local the same op makes it `generated`; a `radial` command afterwards clears
  `generated`.
- PEOPLE (portrait DNG): `generated` is `people`; CENTRE at the face
  (0.44, 0.22); CORNER at (0.02, 0.02) and (0.98, 0.02).
- NOTHING (flat DNG): `create` subject and `create` people are refused
  in-band with exactly `No subject was found in the image.` and `No people
  were found in this image.`; Recompute on an existing radial local there is
  refused the same way; `get_raw_locals` and `get_raw_source` are unchanged
  after each.
- REFUSAL: `mask: {op: subject, radius: 3}` (on a create and on a Recompute)
  and an `update` carrying `generated` are refused in-band and change nothing.
- NO SKY MODEL (the instance's own empty `DR_MODELS_DIR`): `create` with
  `mask: {op: sky}` on the flat DNG is refused in-band with exactly `the sky
  model is not installed; download it in Settings ▸ Models` and changes
  nothing.
- SKY (a second instance whose `DR_MODELS_DIR` is the read-only fixture store
  ~/devel/doctored-fixtures/models; the sky DNG, `create` with `mask: {op:
  sky}`, `exposure_offset` 1): `generated` is `sky`; the plane is the source
  size; CENTRE at the sky points (0.5, 0.1) and (0.2, 0.2); CORNER at the
  ground points (0.5, 0.95) and (0.1, 0.95); OUTSIDE lattice points equal the
  plain development exactly and MIN_CLASS of them exist.
- ALERTS: neither instance's log holds an `[AutomationAlerts]` line.

Usage: python3 scripts/verify_v1_raw_subject.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time
from contextlib import contextmanager

sys.dont_write_bytecode = True
from phase3_fixtures import synthetic_dng  # noqa: E402
import verify_v1_lens  # noqa: E402
from verify_v1_lens import App, SlowClient  # noqa: E402

FIXTURES = Path.home() / 'devel/doctored-fixtures'
Z6 = FIXTURES / 'raw-pixls/nikon-z6/Z6.NEF'
PORTRAIT = FIXTURES / 'phase6/photos/portrait.png'
SKY = FIXTURES / 'phase6/photos/sky.jpg'
MODEL_STORE = FIXTURES / 'models'
LIMIT = 8 * 1024 ** 3
BUSY = 'Another background operation is running.'

LATTICE = (32, 20)
INSIDE = 58982
OUTSIDE = 0
CORNER = 655
CENTRE = 32768
MOVED = 64
MIN_CLASS = 20

OBJECTS = [(0.52, 0.60), (0.72, 0.62), (0.31, 0.56)]
BACKGROUND = [(0.02, 0.02), (0.98, 0.02), (0.98, 0.98)]
FACE = (0.44, 0.22)
PORTRAIT_BACKGROUND = [(0.02, 0.02), (0.98, 0.02)]

NO_SUBJECT = 'No subject was found in the image.'
NO_PEOPLE = 'No people were found in this image.'
NO_SKY_MODEL = 'the sky model is not installed; download it in Settings ▸ Models'
SKY_POINTS = [(0.5, 0.1), (0.2, 0.2)]
GROUND_POINTS = [(0.5, 0.95), (0.1, 0.95)]


# ------------------------------------------------------------- fixtures --

def bmp_rgb(path):
    """(width, height, rows) of an uncompressed 24- or 32-bit BMP; rows are
    top-first lists of (r, g, b) byte triples."""
    data = path.read_bytes()
    assert data[:2] == b'BM', 'not a BMP'
    start, = struct.unpack_from('<I', data, 10)
    width, height = struct.unpack_from('<ii', data, 18)
    bits, = struct.unpack_from('<H', data, 28)
    compression, = struct.unpack_from('<I', data, 30)
    assert bits in (24, 32) and compression in (0, 3), (bits, compression)
    shifts = (16, 8, 0)
    if compression == 3:
        masks = struct.unpack_from('<III', data, 54)
        assert all(mask in (0xFF, 0xFF00, 0xFF0000, 0xFF000000) for mask in masks), masks
        shifts = tuple((mask & -mask).bit_length() - 1 for mask in masks)
    step = bits // 8
    stride = (bits * width + 31) // 32 * 4
    rows = []
    for line in range(abs(height)):
        # A positive height stores the bottom row first.
        stored = abs(height) - 1 - line if height > 0 else line
        at = start + stored * stride
        row = []
        for x in range(width):
            pixel = int.from_bytes(data[at + x * step:at + x * step + step], 'little')
            row.append(tuple((pixel >> shift) & 0xFF for shift in shifts))
        rows.append(row)
    return width, abs(height), rows


def iec_linear(code):
    value = code / 255
    return value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4


def portrait_dng(folder):
    """The portrait as a camera file: its sRGB pixels decoded to linear and
    mosaicked RGGB between the DNG's black (64) and white (1023)."""
    return mosaicked_dng(folder, PORTRAIT, 'portrait', (512, 512))


def sky_dng(folder):
    """sky.jpg at 1024 x 682 as a camera file, mosaicked as the portrait is."""
    return mosaicked_dng(folder, SKY, 'sky', (1024, 682), ['-z', '682', '1024'])


def mosaicked_dng(folder, picture, name, size, resize=()):
    bmp = folder / f'{name}.bmp'
    subprocess.run(['sips', *resize, '-s', 'format', 'bmp', str(picture), '--out', str(bmp)],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    width, height, rows = bmp_rgb(bmp)
    assert (width, height) == size, (width, height)
    table = [round(64 + 959 * iec_linear(code)) for code in range(256)]
    samples = bytearray()
    for y, row in enumerate(rows):
        # RGGB: red and green on even rows, green and blue on odd ones.
        values = [table[pixel[(y % 2) + (x % 2)]] for x, pixel in enumerate(row)]
        samples.extend(struct.pack('<' + 'H' * width, *values))
    path = folder / f'{name}.dng'
    synthetic_dng(path, width, height, orientation=1, samples=bytes(samples))
    return path


@contextmanager
def models_at(store):
    """`verify_v1_lens.App` launches with `store` as `DR_MODELS_DIR` inside
    this block (it reads the model store from `preset_environment`)."""
    original = verify_v1_lens.preset_environment

    def environment(folder):
        values = original(folder)
        values['DR_MODELS_DIR'] = str(store)
        return values

    verify_v1_lens.preset_environment = environment
    try:
        yield
    finally:
        verify_v1_lens.preset_environment = original


def flat_dng(folder):
    path = folder / 'flat.dng'
    width, height = 512, 384
    synthetic_dng(path, width, height, orientation=1,
                  samples=struct.pack('<H', 300) * (width * height))
    return path


# --------------------------------------------------------------- plumbing --

class Mask:
    """One exported `.drmask`: the RZLM header and its u16 coverage."""

    def __init__(self, data):
        assert data[:4] == b'RZLM', data[:16]
        version, self.width, self.height = struct.unpack_from('<III', data, 4)
        assert version == 1, version
        assert len(data) == 16 + self.width * self.height * 2, (len(data), self.width, self.height)
        self.data = data

    def at(self, x, y):
        return struct.unpack_from('<H', self.data, 16 + (y * self.width + x) * 2)[0]

    def fraction(self, fx, fy):
        return self.at(min(self.width - 1, int(fx * self.width)),
                       min(self.height - 1, int(fy * self.height)))


class Run:
    """The shared state of one drive: the app, its folder and what the cases
    hand each other."""

    def __init__(self, app, folder):
        self.app = app
        self.folder = Path(folder)
        self.serial = 0
        self.subject = None
        self.flat = None
        self.points = None
        self.plain_lattice = None

    def open(self, fixture, tag):
        self.serial += 1
        path = self.folder / f'{self.serial:02d}-{tag}{fixture.suffix}'
        shutil.copyfile(fixture, path)
        document = self.app.call('open_document', path=str(path), precision='preserve')['document']
        return document['id'], (document['width'], document['height'])

    def close(self, document):
        self.app.call('close_document', document_id=document, discard_changes=True)

    def source(self, document):
        return self.waiting('get_raw_source', document_id=document, layer=0)['source']

    def exposure(self, document):
        return self.source(document)['raw']['recipe']['values']['exposure']

    def locals(self, document):
        return self.waiting('get_raw_locals', document_id=document, layer=0)

    def node(self, document, identifier):
        return next(node for node in self.locals(document)['nodes'] if node['id'] == identifier)

    def mask(self, document, identifier):
        self.serial += 1
        path = self.folder / f'{self.serial:02d}-{identifier}.drmask'
        reply = self.waiting('get_raw_local_mask', document_id=document, layer=0, id=identifier,
                             path=str(path))
        mask = Mask(path.read_bytes())
        assert (reply['width'], reply['height']) == (mask.width, mask.height), reply
        return mask

    def waiting(self, tool, **arguments):
        # A reply can return while the document's own background step (the
        # development's projection) still runs; the next call then answers
        # BUSY without doing anything. Wait it out, as verify_v1_raw_float does,
        # with a ceiling that only a stuck job reaches.
        deadline = time.monotonic() + 60
        while True:
            try:
                return self.app.call(tool, **arguments)
            except AssertionError as error:
                if BUSY not in str(error) or time.monotonic() >= deadline:
                    raise
                time.sleep(0.05)

    def lattice(self, document):
        return [self.waiting('sample_pixel', document_id=document, source='layer', layer=0,
                             x=x, y=y)['center_rgba_codes'] for x, y in self.points]

    def edit(self, document, *operations):
        reply = self.waiting('edit_raw_development', document_id=document, layer=0,
                             operations=list(operations))
        assert reply['ok'] is True, reply
        return reply

    def refused(self, document, needle, *operations):
        return self.app.refused('edit_raw_development', needle, document_id=document, layer=0,
                                operations=list(operations))


def unclipped(codes):
    return all(256 <= c <= 65279 for c in codes[:3])


def moved(a, b):
    return max(abs(p - q) for p, q in zip(a[:3], b[:3]))


def subject_rule(mask):
    """The CORNER points and the CENTRE rule of the Z6 frame; returns the
    coverage read at each."""
    corners = [mask.fraction(*point) for point in BACKGROUND]
    centres = [mask.fraction(*point) for point in OBJECTS]
    assert all(value <= CORNER for value in corners), ('corner coverage', corners)
    assert sum(value >= CENTRE for value in centres) >= 2, ('object coverage', centres)
    return dict(corners=corners, centres=centres)


# ----------------------------------------------------------------- cases --

def subject_case(run):
    plain, size = run.open(Z6, 'plain')
    columns, rows = LATTICE
    run.points = [(int((i + 0.5) * size[0] / columns), int((j + 0.5) * size[1] / rows))
                  for j in range(rows) for i in range(columns)]
    run.plain_lattice = run.lattice(plain)
    run.close(plain)

    run.subject, opened = run.open(Z6, 'subject')
    assert opened == size, (opened, size)
    assert run.source(run.subject)['depth'] == 16
    assert run.lattice(run.subject) == run.plain_lattice, 'two plain developments differ'
    reply = run.edit(run.subject, dict(op='create', id='s1', mask=dict(op='subject'),
                                       exposure_offset=1))
    assert reply['unchanged'] is False, reply
    listing = run.locals(run.subject)
    assert (listing['source_width'], listing['source_height']) == size, listing
    node = listing['nodes'][0]
    assert node['id'] == 's1' and node['generated'] == 'subject', node
    assert node['exposure_offset'] == 1 and node['density'] == 1 and node['feather'] == 0, node
    mask = run.mask(run.subject, 's1')
    assert (mask.width, mask.height) == size, (mask.width, mask.height, size)
    assert (node['mask']['width'], node['mask']['height']) == size, node['mask']
    detail = subject_rule(mask)

    lattice = run.lattice(run.subject)
    outside = inside = 0
    for index, (x, y) in enumerate(run.points):
        coverage = mask.at(x, y)
        if coverage == OUTSIDE:
            assert lattice[index] == run.plain_lattice[index], (
                'outside the subject', index, lattice[index], run.plain_lattice[index])
            outside += 1
        elif coverage >= INSIDE and unclipped(run.plain_lattice[index]):
            assert moved(lattice[index], run.plain_lattice[index]) >= MOVED, (
                'inside the subject', index, lattice[index], run.plain_lattice[index])
            inside += 1
    assert outside >= MIN_CLASS and inside >= MIN_CLASS, (outside, inside)
    return dict(size=list(size), outside_exact=outside, inside_moved=inside,
                sha256=node['mask']['sha256'], **detail)


def stored_case(run):
    document = run.subject
    before = run.locals(document)
    stored = run.mask(document, 's1').data
    original = run.exposure(document)

    def same(label):
        assert run.locals(document) == before, f'{label} changed the locals'
        assert run.mask(document, 's1').data == stored, f'{label} changed the stored coverage'

    reply = run.app.call('redevelop_raw', document_id=document, layer=0, raw=dict(exposure=-0.25))
    assert reply['ok'] is True, reply
    assert abs(run.exposure(document) + 0.25) < 1e-6, run.exposure(document)
    same('redevelop_raw')
    name = 'v1 subject exposure'
    saved = run.app.call('save_raw_preset', name=name, raw=dict(exposure=0.3), replace=True)
    assert saved['ok'] is True, saved
    applied = run.app.call('apply_raw_preset', name=name, document_id=document, layer=0)
    assert applied['ok'] is True, applied
    assert abs(run.exposure(document) - 0.3) < 1e-6, run.exposure(document)
    same('apply_raw_preset')
    run.app.call('undo', document_id=document)
    assert abs(run.exposure(document) + 0.25) < 1e-6, run.exposure(document)
    same('undo of the preset')
    run.app.call('undo', document_id=document)
    assert abs(run.exposure(document) - original) < 1e-6, run.exposure(document)
    same('undo of redevelop_raw')
    run.app.call('redo', document_id=document)
    run.app.call('redo', document_id=document)
    assert abs(run.exposure(document) - 0.3) < 1e-6, run.exposure(document)
    same('redo')
    return dict(sha256=before['nodes'][0]['mask']['sha256'], bytes=len(stored))


def rz_case(run):
    before = run.locals(run.subject)
    stored = run.mask(run.subject, 's1').data
    lattice = run.lattice(run.subject)
    path = run.folder / 'subject.rz'
    run.app.call('save_copy', document_id=run.subject, path=str(path))
    run.close(run.subject)
    run.subject = None
    reopened = run.app.call('open_document', path=str(path))['document']['id']
    run.subject = reopened
    assert run.locals(reopened) == before, 'get_raw_locals differs after the reopen'
    assert run.mask(reopened, 's1').data == stored, 'the stored coverage differs after the reopen'
    assert run.lattice(reopened) == lattice, 'the lattice codes differ after the reopen'
    reply = run.edit(reopened, dict(op='update', id='s1', exposure_offset=0.5))
    assert reply['unchanged'] is False, reply
    node = run.node(reopened, 's1')
    assert node['exposure_offset'] == 0.5 and node['generated'] == 'subject', node
    assert node['mask'] == before['nodes'][0]['mask'], node['mask']
    return dict(bytes=path.stat().st_size)


def on_top_case(run):
    document = run.subject
    width, height = (run.node(document, 's1')['mask'][key] for key in ('width', 'height'))

    def sha():
        node = run.node(document, 's1')
        assert node['generated'] == 'subject', node
        return node['mask']['sha256']

    start = sha()
    corner = [width * 0.02, height * 0.02]
    run.edit(document, dict(op='mask', id='s1', command=dict(
        op='brush', mode='add',
        dabs=[dict(center=corner, radius=40, hardness=1, opacity=1)])))
    brushed = sha()
    assert brushed != start, 'the brush did not change the coverage'
    painted = run.mask(document, 's1').fraction(0.02, 0.02)
    assert painted >= INSIDE, painted
    run.edit(document, dict(op='mask', id='s1', command=dict(op='invert')))
    inverted = sha()
    assert inverted not in (start, brushed), 'invert did not change the coverage'
    run.edit(document, dict(op='update', id='s1', density=0.5, feather=8))
    assert sha() == inverted, 'density and feather changed the stored coverage'
    band = dict(luminance=dict(low=0.2, high=0.8, low_falloff=0, high_falloff=0, invert=False))
    run.edit(document, dict(op='update', id='s1', range=band))
    assert sha() == inverted, 'a range changed the stored coverage'
    node = run.node(document, 's1')
    assert node['density'] == 0.5 and node['feather'] == 8 and node['range'] == band, node
    return dict(start=start, brushed=brushed, inverted=inverted, painted_corner=painted)


def recompute_case(run):
    document = run.subject
    before = run.node(document, 's1')['mask']['sha256']
    reply = run.edit(document, dict(op='mask', id='s1', command=dict(op='subject')))
    assert reply['unchanged'] is False, reply
    node = run.node(document, 's1')
    assert node['generated'] == 'subject' and node['mask']['sha256'] != before, node
    detail = subject_rule(run.mask(document, 's1'))

    width, height = node['mask']['width'], node['mask']['height']
    radial = dict(op='radial', center=[width / 2, height / 2], radius=[width / 8, height / 8],
                  hardness=0.5)
    run.edit(document, dict(op='create', id='r1', mask=radial))
    drawn = run.node(document, 'r1')
    assert 'generated' not in drawn, drawn
    run.edit(document, dict(op='mask', id='r1', command=dict(op='subject')))
    made = run.node(document, 'r1')
    assert made['generated'] == 'subject' and made['mask']['sha256'] != drawn['mask']['sha256'], made
    subject_rule(run.mask(document, 'r1'))
    run.edit(document, dict(op='mask', id='r1', command=radial))
    cleared = run.node(document, 'r1')
    assert 'generated' not in cleared, cleared
    assert cleared['mask']['sha256'] == drawn['mask']['sha256'], (cleared['mask'], drawn['mask'])
    assert run.node(document, 's1')['generated'] == 'subject'
    run.close(document)
    run.subject = None
    return detail


def people_case(run):
    document, size = run.open(portrait_dng(run.folder), 'portrait')
    assert size == (512, 512), size
    reply = run.edit(document, dict(op='create', id='p1', mask=dict(op='people'),
                                    exposure_offset=1))
    assert reply['unchanged'] is False, reply
    node = run.node(document, 'p1')
    assert node['generated'] == 'people', node
    mask = run.mask(document, 'p1')
    assert (mask.width, mask.height) == size, (mask.width, mask.height)
    face = mask.fraction(*FACE)
    corners = [mask.fraction(*point) for point in PORTRAIT_BACKGROUND]
    assert face >= CENTRE, ('face coverage', face)
    assert all(value <= CORNER for value in corners), ('corner coverage', corners)
    run.close(document)
    return dict(face=face, corners=corners, sha256=node['mask']['sha256'])


def nothing_case(run):
    document, size = run.open(flat_dng(run.folder), 'flat')
    texts = []

    def unchanged(label, locals_before, source_before):
        assert run.locals(document) == locals_before, f'{label} changed the locals'
        assert run.source(document) == source_before, f'{label} changed the source'

    locals_before, source_before = run.locals(document), run.source(document)
    assert locals_before['nodes'] == [], locals_before
    for kind, sentence in (('subject', NO_SUBJECT), ('people', NO_PEOPLE)):
        text = run.refused(document, sentence, dict(op='create', id='n1', mask=dict(op=kind)))
        assert text == sentence, (kind, text)
        unchanged(f'a refused {kind} create', locals_before, source_before)
        texts.append(text)

    run.edit(document, dict(op='create', id='r1', mask=dict(
        op='radial', center=[size[0] / 2, size[1] / 2], radius=[60, 60], hardness=0.5)))
    locals_before, source_before = run.locals(document), run.source(document)
    assert len(locals_before['nodes']) == 1 and 'generated' not in locals_before['nodes'][0]
    for kind, sentence in (('subject', NO_SUBJECT), ('people', NO_PEOPLE)):
        text = run.refused(document, sentence,
                           dict(op='mask', id='r1', command=dict(op=kind)))
        assert text == sentence, (kind, text)
        unchanged(f'a refused {kind} recompute', locals_before, source_before)
        texts.append(text)
    run.flat = document
    return dict(refusals=texts)


def refusal_case(run):
    """Malformed generating operations, on the flat document and its radial
    local."""
    document = run.flat
    locals_before, source_before = run.locals(document), run.source(document)
    operations = [
        ('no other field', dict(op='create', id='n2', mask=dict(op='subject', radius=3))),
        ('no other field', dict(op='mask', id='r1', command=dict(op='people', radius=3))),
        ('Unknown RAW local field', dict(op='update', id='r1', generated='subject')),
    ]
    texts = []
    for needle, operation in operations:
        texts.append(run.refused(document, needle, operation))
        assert run.locals(document) == locals_before, ('a refusal changed the locals', operation)
        assert run.source(document) == source_before, ('a refusal changed the source', operation)
    return dict(refusals=texts)


def no_sky_model_case(run):
    document = run.flat
    locals_before, source_before = run.locals(document), run.source(document)
    text = run.refused(document, NO_SKY_MODEL, dict(op='create', id='k1', mask=dict(op='sky')))
    assert text == NO_SKY_MODEL, text
    assert run.locals(document) == locals_before, 'the refusal changed the locals'
    assert run.source(document) == source_before, 'the refusal changed the source'
    run.close(document)
    run.flat = None
    return dict(refusal=text)


def sky_case(run):
    picture = sky_dng(run.folder)
    plain, size = run.open(picture, 'sky-plain')
    columns, rows = LATTICE
    points = [(int((i + 0.5) * size[0] / columns), int((j + 0.5) * size[1] / rows))
              for j in range(rows) for i in range(columns)]
    run.points = points
    plain_lattice = run.lattice(plain)
    run.close(plain)
    document, opened = run.open(picture, 'sky')
    assert opened == size, (opened, size)
    reply = run.edit(document, dict(op='create', id='k1', mask=dict(op='sky'), exposure_offset=1))
    assert reply['unchanged'] is False, reply
    node = run.node(document, 'k1')
    assert node['generated'] == 'sky', node
    mask = run.mask(document, 'k1')
    assert (mask.width, mask.height) == size, (mask.width, mask.height, size)
    sky = [mask.fraction(*point) for point in SKY_POINTS]
    ground = [mask.fraction(*point) for point in GROUND_POINTS]
    assert all(value >= CENTRE for value in sky), ('sky coverage', sky)
    assert all(value <= CORNER for value in ground), ('ground coverage', ground)
    lattice = run.lattice(document)
    outside = 0
    for index, (x, y) in enumerate(points):
        if mask.at(x, y) == OUTSIDE:
            assert lattice[index] == plain_lattice[index], (
                'outside the sky', index, lattice[index], plain_lattice[index])
            outside += 1
    assert outside >= MIN_CLASS, outside
    run.close(document)
    return dict(size=list(size), sky=sky, ground=ground, outside_exact=outside,
                sha256=node['mask']['sha256'])


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, the copies and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-raw-subject-'))
    output.mkdir(parents=True, exist_ok=True)

    results, failed = {}, []

    def run_case(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    missing = [path for path in (Z6, PORTRAIT, SKY, MODEL_STORE) if not path.exists()]
    if missing:
        results['fixture'] = dict(passed=False, error=f'missing {missing}')
        failed.append('fixture')
        print(f'FAIL fixture: missing {missing}', flush=True)
    else:
        with App(output / 'instance', LIMIT, SlowClient) as app:
            run = Run(app, output)
            run_case('subject', lambda: subject_case(run))
            run_case('stored', lambda: stored_case(run))
            run_case('rz', lambda: rz_case(run))
            run_case('on top', lambda: on_top_case(run))
            run_case('recompute', lambda: recompute_case(run))
            run_case('people', lambda: people_case(run))
            run_case('nothing', lambda: nothing_case(run))
            run_case('refusal', lambda: refusal_case(run))
            run_case('no sky model', lambda: no_sky_model_case(run))
        with models_at(MODEL_STORE), App(output / 'instance-models', LIMIT, SlowClient) as app:
            run_case('sky', lambda: sky_case(Run(app, output)))

        def alerts():
            lines = [line for log in ('instance/app.log', 'instance-models/app.log')
                     for line in (output / log).read_text(errors='replace').splitlines()
                     if '[AutomationAlerts]' in line]
            assert not lines, lines[:5]
            return dict(lines=0)

        run_case('alerts', alerts)

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
