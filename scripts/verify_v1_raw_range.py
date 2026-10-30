#!/usr/bin/env python3
"""MCP drive for Luminance Range and Colour Range masks on Camera RAW locals:
`edit_raw_development` operations carrying `range`, on the real NEF.

The script owns its one app instance: the Debug app (or `DR_VERIFY_APP`) on
its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, an 8 GiB working limit and private `DR_*_DIR`s
(`verify_v1_lens.App`). It terminates only that process. The fixture is
`~/devel/doctored-fixtures/camera/camera.NEF`; an absent fixture FAILS the
run. Every document is opened from its own copy of the file, at 16 bits.

What is measured. A local's range is evaluated on the BASE development (the
global render before any local), so the oracle reads a PLAIN development of the
same file: `sample_pixel` (`source: layer`) on a 32×20 lattice. Its sRGB codes
are decoded by IEC 61966-2.1, taken to PCS XYZ by the matrix this file derives
from the sRGB primaries (D65, Bradford-adapted to D50; `verify_v1_raw_output`'s
`to_pcs`), and to CIE L*a*b* against D50 = (0.96422, 1, 0.82521) with L*
clamped to 0…100. l = L*/100. A ranged document is then compared with the plain
one point by point: a point the range excludes must hold the plain codes
exactly, and a point it includes must have moved.

Bounds, fixed before the first run and never widened:
- the lattice is 32×20 (640 points);
- MARGIN = 0.02 l: a point more than MARGIN outside a hard luminance range is
  code-identical to the plain development;
- an UNCLIPPED point (every plain code in 256…65279) more than MARGIN inside
  moves at least MOVED = 64 codes in some channel under `exposure_offset` −1;
- at least MIN_CLASS = 20 lattice points in each class a case names;
- COLOUR_MARGIN = 5 ΔE: a point whose oracle ΔE from the reported colour is
  above amount + falloff + COLOUR_MARGIN is code-identical to the plain
  development;
- SAMPLE_BOUND = 2 ΔE: the `lab` a `sample` resolves to, against the oracle's
  mean Lab of the 5×5 plain pixels about the sampled one.
The ranges themselves are chosen from the fixture, which is not a tolerance:
the luminance band is the 35th…65th percentile of the lattice's l (20th…80th
inside the radial), rounded to three places, and the colour is sampled at one
of the five unclipped lattice points about the median l: the one whose own
colour is nearest the mean of its 5×5 pixels (a flat patch).

Cases:
- LUMINANCE: `create` with `mask: {op: full}`, `exposure_offset` −1 and a hard
  band. Outside points exact, unclipped inside points moved; `get_raw_locals`
  reports the range with every field.
- OFFSET: `update` to `exposure_offset` −2 leaves every outside point exact
  (the local's own exposure does not move its selection) and the inside moved.
- UNDO: `undo` twice returns the plain lattice and no locals; `redo` twice
  returns the OFFSET lattice.
- VERSION: the recipe's enhancement `version` is 1 with a local that has no
  range, 2 once it has one, and 1 again after `range: null`.
- REDEVELOP: `redevelop_raw {exposure}` keeps the range, the points outside it
  (by a plain copy opened at that exposure) equal that plain copy, and the
  unclipped points inside it moved (see `REDEVELOP_EXPOSURE`).
- PRESET: `save_raw_preset` / `apply_raw_preset` of an exposure keep the range
  and the enhancement version.
- RZ: `save_copy` to `.rz`, close, reopen: `get_raw_locals` equal, the lattice
  identical, and a further `edit_raw_development` succeeds with the range kept.
- COLOUR: `create` with `color: {sample: [x, y], amount, falloff}`.
  `get_raw_locals` reports `lab` and no `sample`, within SAMPLE_BOUND of the
  oracle; points beyond amount + falloff + COLOUR_MARGIN exact; the sampled
  point itself moved.
- RADIAL: a band inside a radial shape. Points outside the ellipse exact,
  points inside it and outside the band exact, unclipped points in the
  ellipse's hard core and inside the band moved.
- REFUSAL: low > high, `lab` with `sample`, an unknown key and a sample point
  off the frame are refused in-band; `get_raw_locals` and `get_raw_source` are
  unchanged.

Usage: python3 scripts/verify_v1_raw_range.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import json
from pathlib import Path
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_v1_lens import App, SlowClient  # noqa: E402
from verify_v1_raw_output import SRGB_PRIMARIES, iec_decode, mat_vec, to_pcs  # noqa: E402

FIXTURE = Path.home() / 'devel/doctored-fixtures/camera/camera.NEF'
LIMIT = 8 * 1024 ** 3

LATTICE = (32, 20)
MARGIN = 0.02
MOVED = 64
MIN_CLASS = 20
COLOUR_MARGIN = 5.0
SAMPLE_BOUND = 2.0

# The REDEVELOP case's global exposure. The band is the LUMINANCE case's, taken
# from the as-shot lattice, so the exposure has to leave lattice points inside
# it. The first run used +0.5: every one of its 542 outside points was exact,
# but the brighter picture left only 10 unclipped points inside the band,
# under MIN_CLASS. At −0.25 the second run had 346 outside and 113 inside. No
# bound moved.
REDEVELOP_EXPOSURE = -0.25

WHITE = (0.96422, 1.0, 0.82521)
SRGB_TO_PCS = to_pcs(SRGB_PRIMARIES, 'D65')


# ------------------------------------------------------------- the oracle --

def lab_of(codes):
    """CIE L*a*b* (D50) of one sRGB pixel's codes, L* clamped to 0…100."""
    xyz = mat_vec(SRGB_TO_PCS, [iec_decode(c / 65535) for c in codes[:3]])

    def f(t):
        return t ** (1 / 3) if t > 216 / 24389 else (24389 / 27 * t + 16) / 116

    fx, fy, fz = (f(max(v, 0.0) / w) for v, w in zip(xyz, WHITE))
    return [min(100.0, max(0.0, 116 * fy - 16)), 500 * (fx - fy), 200 * (fy - fz)]


def delta_e(a, b):
    return sum((p - q) ** 2 for p, q in zip(a, b)) ** 0.5


def unclipped(codes):
    return all(256 <= c <= 65279 for c in codes[:3])


def moved(a, b):
    return max(abs(p - q) for p, q in zip(a[:3], b[:3]))


def band_of(labs, low_fraction, high_fraction):
    """A luminance band holding the middle of the lattice's own l values."""
    values = sorted(lab[0] / 100 for lab in labs)
    low = round(values[int(low_fraction * (len(values) - 1))], 3)
    high = round(values[int(high_fraction * (len(values) - 1))], 3)
    assert 0 <= low < high <= 1, (low, high)
    return low, high


def band_classes(labs, low, high):
    """Lattice indices clear of a hard band's bounds by MARGIN: outside and
    inside. A point within MARGIN of a bound is in neither."""
    outside, inside = [], []
    for index, lab in enumerate(labs):
        value = lab[0] / 100
        if value < low - MARGIN or value > high + MARGIN:
            outside.append(index)
        elif low + MARGIN < value < high - MARGIN:
            inside.append(index)
    return outside, inside


def luminance(low, high):
    return dict(luminance=dict(low=low, high=high, low_falloff=0, high_falloff=0, invert=False))


# --------------------------------------------------------------- plumbing --

class Run:
    """The shared state of one drive: the app, its folder and what the cases
    hand each other."""

    def __init__(self, app, folder):
        self.app = app
        self.folder = Path(folder)
        self.serial = 0
        self.size = None
        self.points = None
        self.plain = None
        self.plain_lattice = None
        self.plain_labs = None
        self.band = None
        self.ranged = None
        self.luminance_lattice = None
        self.offset_lattice = None
        self.radial = None

    def copy(self, tag):
        self.serial += 1
        path = self.folder / f'{self.serial:02d}-{tag}.NEF'
        shutil.copyfile(FIXTURE, path)
        return path

    def open(self, tag, **development):
        arguments = dict(path=str(self.copy(tag)), precision='preserve')
        if development:
            arguments['raw'] = development
        document = self.app.call('open_document', **arguments)['document']
        if self.size is None:
            self.size = (document['width'], document['height'])
            columns, rows = LATTICE
            self.points = [(int((i + 0.5) * self.size[0] / columns),
                            int((j + 0.5) * self.size[1] / rows))
                           for j in range(rows) for i in range(columns)]
        assert (document['width'], document['height']) == self.size, document
        return document['id']

    def close(self, document):
        self.app.call('close_document', document_id=document, discard_changes=True)

    def source(self, document):
        return self.app.call('get_raw_source', document_id=document, layer=0)['source']

    def locals(self, document):
        return self.app.call('get_raw_locals', document_id=document, layer=0)

    def pixel(self, document, x, y):
        return self.app.call('sample_pixel', document_id=document, source='layer', layer=0,
                             x=x, y=y)['center_rgba_codes']

    def lattice(self, document):
        return [self.pixel(document, x, y) for x, y in self.points]

    def edit(self, document, *operations, **raw):
        arguments = dict(document_id=document, layer=0, operations=list(operations))
        if raw:
            arguments['raw'] = raw
        reply = self.app.call('edit_raw_development', **arguments)
        assert reply['ok'] is True, reply
        return reply


def enhancement_version(source):
    enhancements = source['raw']['recipe'].get('enhancements')
    return None if enhancements is None else enhancements['version']


def assert_exact(lattice, plain, indices, label):
    for index in indices:
        assert lattice[index] == plain[index], (label, index, lattice[index], plain[index])


def assert_moved(lattice, plain, indices, label):
    """Every unclipped point of `indices` moved by MOVED codes; returns how
    many there were."""
    checked = 0
    for index in indices:
        if not unclipped(plain[index]):
            continue
        assert moved(lattice[index], plain[index]) >= MOVED, (
            label, index, lattice[index], plain[index])
        checked += 1
    return checked


# ----------------------------------------------------------------- cases --

def setup(run):
    run.plain = run.open('plain')
    source = run.source(run.plain)
    assert source['depth'] == 16, source
    assert enhancement_version(source) is None, source['raw']['recipe']
    run.plain_lattice = run.lattice(run.plain)
    run.plain_labs = [lab_of(codes) for codes in run.plain_lattice]
    run.band = band_of(run.plain_labs, 0.35, 0.65)
    values = sorted(lab[0] / 100 for lab in run.plain_labs)
    return dict(size=list(run.size), lattice=len(run.plain_lattice), band=list(run.band),
                l_min=values[0], l_median=values[len(values) // 2], l_max=values[-1],
                unclipped=sum(unclipped(codes) for codes in run.plain_lattice))


def luminance_case(run):
    low, high = run.band
    run.ranged = run.open('luminance')
    assert run.lattice(run.ranged) == run.plain_lattice, 'two plain developments differ'
    reply = run.edit(run.ranged, dict(op='create', id='range1', mask=dict(op='full'),
                                      exposure_offset=-1, range=luminance(low, high)))
    assert reply['unchanged'] is False, reply
    nodes = run.locals(run.ranged)['nodes']
    assert len(nodes) == 1 and nodes[0]['id'] == 'range1', nodes
    assert nodes[0]['range'] == luminance(low, high), nodes[0]['range']
    assert nodes[0]['exposure_offset'] == -1, nodes[0]
    run.luminance_lattice = run.lattice(run.ranged)
    outside, inside = band_classes(run.plain_labs, low, high)
    assert_exact(run.luminance_lattice, run.plain_lattice, outside, 'outside the band')
    checked = assert_moved(run.luminance_lattice, run.plain_lattice, inside, 'inside the band')
    assert len(outside) >= MIN_CLASS and checked >= MIN_CLASS, (len(outside), checked)
    return dict(band=[low, high], outside_exact=len(outside), inside_moved=checked)


def offset_case(run):
    low, high = run.band
    reply = run.edit(run.ranged, dict(op='update', id='range1', exposure_offset=-2))
    assert reply['unchanged'] is False, reply
    nodes = run.locals(run.ranged)['nodes']
    assert nodes[0]['exposure_offset'] == -2 and nodes[0]['range'] == luminance(low, high), nodes
    run.offset_lattice = run.lattice(run.ranged)
    outside, inside = band_classes(run.plain_labs, low, high)
    assert_exact(run.offset_lattice, run.plain_lattice, outside, 'outside the band at -2')
    checked = assert_moved(run.offset_lattice, run.plain_lattice, inside, 'inside the band at -2')
    further = sum(1 for index in inside if unclipped(run.plain_lattice[index])
                  and moved(run.offset_lattice[index], run.plain_lattice[index])
                  > moved(run.luminance_lattice[index], run.plain_lattice[index]))
    assert len(outside) >= MIN_CLASS and checked >= MIN_CLASS, (len(outside), checked)
    assert further >= MIN_CLASS, further
    return dict(outside_exact=len(outside), inside_moved=checked, moved_further=further)


def undo_case(run):
    run.app.call('undo', document_id=run.ranged)
    assert run.lattice(run.ranged) == run.luminance_lattice, 'undo did not restore offset -1'
    run.app.call('undo', document_id=run.ranged)
    assert run.locals(run.ranged)['nodes'] == [], 'undo did not remove the local'
    assert run.lattice(run.ranged) == run.plain_lattice, 'undo did not restore the plain lattice'
    run.app.call('redo', document_id=run.ranged)
    run.app.call('redo', document_id=run.ranged)
    assert run.lattice(run.ranged) == run.offset_lattice, 'redo did not restore offset -2'
    nodes = run.locals(run.ranged)['nodes']
    assert nodes[0]['range'] == luminance(*run.band), nodes
    return dict(steps=4)


def version_case(run):
    document = run.open('version')
    run.edit(document, dict(op='create', id='plain1', mask=dict(op='empty')))
    without = enhancement_version(run.source(document))
    assert without == 1, without
    assert 'range' not in run.locals(document)['nodes'][0], run.locals(document)
    run.edit(document, dict(op='update', id='plain1', range=luminance(0.25, 0.75)))
    with_range = enhancement_version(run.source(document))
    assert with_range == 2, with_range
    run.edit(document, dict(op='update', id='plain1', range=None))
    removed = enhancement_version(run.source(document))
    assert removed == 1, removed
    assert 'range' not in run.locals(document)['nodes'][0], run.locals(document)
    assert enhancement_version(run.source(run.ranged)) == 2
    run.close(document)
    return dict(without=without, with_range=with_range, removed=removed)


def redevelop_case(run):
    low, high = run.band
    before = run.locals(run.ranged)
    reply = run.app.call('redevelop_raw', document_id=run.ranged, layer=0,
                         raw=dict(exposure=REDEVELOP_EXPOSURE))
    assert reply['ok'] is True, reply
    source = run.source(run.ranged)
    exposure = source['raw']['recipe']['values']['exposure']
    assert abs(exposure - REDEVELOP_EXPOSURE) < 1e-6, source['raw']['recipe']
    assert enhancement_version(source) == 2, source['raw']['recipe']
    assert run.locals(run.ranged) == before, 'redevelop_raw changed the locals'
    reference = run.open('plain-exposure', exposure=REDEVELOP_EXPOSURE)
    plain = run.lattice(reference)
    run.close(reference)
    assert plain != run.plain_lattice, 'the exposure did not change the plain development'
    outside, inside = band_classes([lab_of(codes) for codes in plain], low, high)
    lattice = run.lattice(run.ranged)
    assert_exact(lattice, plain, outside, 'outside the band after redevelop_raw')
    checked = assert_moved(lattice, plain, inside, 'inside the band after redevelop_raw')
    assert len(outside) >= MIN_CLASS and checked >= MIN_CLASS, (len(outside), checked)
    return dict(outside_exact=len(outside), inside_moved=checked)


def preset_case(run):
    name = 'v1 range exposure'
    saved = run.app.call('save_raw_preset', name=name, raw=dict(exposure=0.3), replace=True)
    assert saved['ok'] is True, saved
    before = run.locals(run.ranged)
    applied = run.app.call('apply_raw_preset', name=name, document_id=run.ranged, layer=0)
    assert applied['ok'] is True, applied
    source = run.source(run.ranged)
    assert abs(source['raw']['recipe']['values']['exposure'] - 0.3) < 1e-6, source['raw']['recipe']
    assert enhancement_version(source) == 2, source['raw']['recipe']
    assert run.locals(run.ranged) == before, 'apply_raw_preset changed the locals'
    return dict(range=before['nodes'][0]['range'])


def rz_case(run):
    before = run.locals(run.ranged)
    lattice = run.lattice(run.ranged)
    path = run.folder / 'ranged.rz'
    run.app.call('save_copy', document_id=run.ranged, path=str(path))
    run.close(run.ranged)
    run.ranged = None
    reopened = run.app.call('open_document', path=str(path))['document']['id']
    assert run.locals(reopened) == before, 'get_raw_locals differs after the reopen'
    assert enhancement_version(run.source(reopened)) == 2
    assert run.lattice(reopened) == lattice, 'the lattice codes differ after the reopen'
    reply = run.edit(reopened, dict(op='update', id='range1', exposure_offset=-1.5))
    assert reply['unchanged'] is False, reply
    nodes = run.locals(reopened)['nodes']
    assert nodes[0]['exposure_offset'] == -1.5 and nodes[0]['range'] == before['nodes'][0]['range']
    run.close(reopened)
    return dict(bytes=path.stat().st_size)


def colour_case(run):
    amount, falloff = 15.0, 10.0
    order = sorted((index for index in range(len(run.plain_lattice))
                    if unclipped(run.plain_lattice[index])),
                   key=lambda index: run.plain_labs[index][0])
    width, height = run.size

    def window_mean(index):
        x, y = run.points[index]
        window = [lab_of(run.pixel(run.plain, px, py))
                  for py in range(max(0, y - 2), min(height - 1, y + 2) + 1)
                  for px in range(max(0, x - 2), min(width - 1, x + 2) + 1)]
        return [sum(lab[k] for lab in window) / len(window) for k in range(3)]

    # Of the five unclipped lattice points about the median l, the one whose
    # own colour is nearest its 5×5 mean: a flat patch, so the sampled colour
    # is the colour of the point it was sampled at.
    middle = len(order) // 2
    means = {index: window_mean(index) for index in order[max(0, middle - 2):middle + 3]}
    chosen = min(means, key=lambda index: delta_e(run.plain_labs[index], means[index]))
    mean = means[chosen]
    x, y = run.points[chosen]

    document = run.open('colour')
    reply = run.edit(document, dict(
        op='create', id='colour1', mask=dict(op='full'), exposure_offset=-1,
        range=dict(color=dict(sample=[x + 0.5, y + 0.5], amount=amount, falloff=falloff))))
    assert reply['unchanged'] is False, reply
    reported = run.locals(document)['nodes'][0]['range']
    assert set(reported) == {'color'}, reported
    colour = reported['color']
    assert set(colour) == {'lab', 'amount', 'falloff', 'invert'}, colour
    assert colour['amount'] == amount and colour['falloff'] == falloff, colour
    assert colour['invert'] is False and len(colour['lab']) == 3, colour
    error = delta_e(colour['lab'], mean)
    assert error <= SAMPLE_BOUND, (colour['lab'], mean, error)
    assert enhancement_version(run.source(document)) == 2

    lattice = run.lattice(document)
    distances = [delta_e(lab, colour['lab']) for lab in run.plain_labs]
    outside = [index for index, d in enumerate(distances)
               if d > amount + falloff + COLOUR_MARGIN]
    inside = [index for index, d in enumerate(distances) if d < amount - COLOUR_MARGIN]
    assert_exact(lattice, run.plain_lattice, outside, 'beyond the colour range')
    checked = assert_moved(lattice, run.plain_lattice, inside, 'inside the colour range')
    assert len(outside) >= MIN_CLASS, len(outside)
    assert chosen in inside and checked >= 1, (distances[chosen], checked)
    run.close(document)
    return dict(sample=[x + 0.5, y + 0.5], lab=colour['lab'], oracle_mean=mean, sample_error=error,
                outside_exact=len(outside), inside_moved=checked)


def radial_case(run):
    low, high = band_of(run.plain_labs, 0.20, 0.80)
    width, height = run.size
    center = [width / 2, height / 2]
    radius = [width * 0.45, height * 0.45]
    hardness = 0.9
    document = run.open('radial')
    reply = run.edit(document, dict(
        op='create', id='radial1', exposure_offset=-1, range=luminance(low, high),
        mask=dict(op='radial', center=center, radius=radius, hardness=hardness)))
    assert reply['unchanged'] is False, reply
    lattice = run.lattice(document)
    beyond, excluded, core = [], [], []
    band_outside, band_inside = (set(indices) for indices in
                                 band_classes(run.plain_labs, low, high))
    for index, (x, y) in enumerate(run.points):
        d = (((x + 0.5 - center[0]) / radius[0]) ** 2
             + ((y + 0.5 - center[1]) / radius[1]) ** 2) ** 0.5
        if d > 1.01:
            beyond.append(index)
        elif index in band_outside:
            excluded.append(index)
        elif d < hardness - 0.01 and index in band_inside:
            core.append(index)
    assert_exact(lattice, run.plain_lattice, beyond, 'outside the ellipse')
    assert_exact(lattice, run.plain_lattice, excluded, 'in the ellipse, outside the band')
    checked = assert_moved(lattice, run.plain_lattice, core, 'in the ellipse core and the band')
    assert len(beyond) >= MIN_CLASS and len(excluded) >= MIN_CLASS and checked >= MIN_CLASS, (
        len(beyond), len(excluded), checked)
    run.radial = document
    return dict(band=[low, high], outside_ellipse_exact=len(beyond),
                outside_band_exact=len(excluded), core_moved=checked)


def refusal_case(run):
    document = run.radial
    locals_before = run.locals(document)
    source_before = run.source(document)
    width, _ = run.size
    cases = [
        ('RAW local range', luminance(0.7, 0.3)),
        ('RAW local range', dict(color=dict(lab=[50, 0, 0], sample=[10, 10], amount=10))),
        ('RAW local range', dict()),
        ('Unknown RAW local field', dict(luminance=dict(low=0.2, high=0.8, feather=1))),
        ('outside the source', dict(color=dict(sample=[width + 10, 5], amount=10))),
    ]
    texts = []
    for needle, value in cases:
        texts.append(run.app.refused('edit_raw_development', needle, document_id=document, layer=0,
                                     operations=[dict(op='update', id='radial1', range=value)]))
        assert run.locals(document) == locals_before, ('a refusal changed the locals', value)
        assert run.source(document) == source_before, ('a refusal changed the source', value)
    run.close(document)
    return dict(refusals=texts)


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, the copies and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-raw-range-'))
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

    if not FIXTURE.is_file():
        results['fixture'] = dict(passed=False, error=f'missing {FIXTURE}')
        failed.append('fixture')
        print(f'FAIL fixture: missing {FIXTURE}', flush=True)
    else:
        with App(output / 'instance', LIMIT, SlowClient) as app:
            run = Run(app, output)
            run_case('plain reference', lambda: setup(run))
            run_case('luminance', lambda: luminance_case(run))
            run_case('offset', lambda: offset_case(run))
            run_case('undo', lambda: undo_case(run))
            run_case('version', lambda: version_case(run))
            run_case('redevelop', lambda: redevelop_case(run))
            run_case('preset', lambda: preset_case(run))
            run_case('rz', lambda: rz_case(run))
            run_case('colour', lambda: colour_case(run))
            run_case('radial', lambda: radial_case(run))
            run_case('refusal', lambda: refusal_case(run))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
