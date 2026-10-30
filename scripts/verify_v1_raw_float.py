#!/usr/bin/env python3
"""MCP drive for the 32-bit float (linear) Camera RAW development:
`raw.output_encoding: linear_float` on `open_document`, `place_image`,
`redevelop_raw` and `edit_raw_development`, on the real NEF.

The script owns its one app instance: the Debug app (or `DR_VERIFY_APP`) on
its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, an 8 GiB working limit and private `DR_*_DIR`s
(`verify_v1_lens.App`). It terminates only that process. The fixture is
`~/devel/doctored-fixtures/camera/camera.NEF`; an absent fixture FAILS the
run. Every document is opened from its own copy of the file.

What is measured. Properties of the development, never decoder bytes (the
system decoder changes between macOS versions): values above the SDR white,
linearity in exposure, agreement with the 16-bit development of the same
recipe where the two are meant to agree, and that the layer stays a
re-editable RAW layer through undo, `.rz`, a preset, Sync, a place, locals
and an encoding switch. Samples are `sample_pixel` (`source: layer`) on a
32×20 lattice (640 points).

Bounds, fixed before the first run and never widened. They are measurement
M1's (`scripts/raw-probe/float_edr.py`, whose docstring holds Rulings 1 and 2
of the batch), restated here on this lattice:
- V1: at +2 EV the largest float channel on the lattice is >= 1.5 and >= 20
  points exceed 1.0; the 16-bit development of the same recipe holds no code
  above 65535 (it cannot) and >= 20 points with a channel AT 65535.
- V2: over the points whose largest float channel at 0 EV is in 0.02…0.6 and
  whose 0 EV 16-bit codes are all < 60000, the per-channel ratios f(+1)/f(0)
  have median in 1.98…2.02 with >= 95 % in 1.9…2.1, and f(+2)/f(0) median in
  3.92…4.08 with >= 95 % in 3.6…4.4. A channel whose 0 EV float value is not
  positive counts as outside both ranges.
- The shoulder code is S = 65535 × sRGB-encode(0.25) = 35198.77: the 16-bit
  development compresses highlights above about 0.3 linear and a scene-linear
  development does not. A point is BELOW the shoulder when every one of its
  16-bit codes is under S, and AT OR ABOVE it when some code is >= S.
- V3a (0 EV): over the points with every 16-bit code in 256…65279, per
  channel |sRGB-encode(clamp01(f)) − code/65535| has median <= 0.002 and
  >= 95 % <= 0.01.
- V3b (+1 EV and +2 EV, each against the 16-bit development of its own
  exposure): the same bound over the points with every code in 256…65279 that
  are below the shoulder.
- V3c (0, +1 and +2 EV): for every point at or above the shoulder, the
  LARGEST channel of the float development, clipped to 0…1 and encoded, is
  not below the largest channel of the 16-bit development by more than 0.01.
  The per-channel differences are reported and not judged: where a channel
  exceeds white the 16-bit development moves the colour toward white, which
  raises its lower channels, so the clipped float view differs in hue too.
  For V3 this drive samples as the probe does, so its margin is the probe's:
  each point is the 5×5 square about the lattice pixel, the float value the
  MEAN OF THE PER-PIXEL ENCODINGS of the 25 clamped samples, the 16-bit value
  the mean of the 25 codes (`mean_rgba_codes`, `sample_size` 5).
- MARGIN = 0.02 l (l = CIE L*/100 of the linear base, through the sRGB
  primaries' matrix this file derives, D50, L* clamped to 0…100): a point
  more than MARGIN outside a hard luminance band is EXACTLY the base's; a
  point more than MARGIN inside is HALVED by `exposure_offset` −1: each
  positive channel's ratio to the base is in 0.5 ± 0.05 for >= 95 % of the
  channels judged.
- at least MIN_CLASS = 20 lattice points in each class a case names.
The band is chosen from the fixture, which is not a tolerance: the 35th…65th
percentile of the lattice's l, rounded to three places.

Cases: open, highlights (V1), doubling (V2), sdr agreement (V3a, V3b, V3c),
undo (`redevelop_raw`, then one `undo` restores the lattice exactly), rz
(`.rz` reopen is identical and redevelops again), preset and sync (both keep
depth 32), place (into the float document: depth 32; `sdr16`: 16;
`tone_curve: 0.5` with no encoding: 32, the curve kept), range local, radial
local, encoding switch (on the layer with its locals: `sdr16` and back, the
locals kept), p3 (a Display P3 float open carries the P3 profile), narrowing
(`convert_precision` preserve to 16 bits and a layers copy/paste into a
16-bit and an 8-bit document refuse by name while the layer is float and
succeed after the switch to `sdr16`), refusals (by name:
`precision: rgba8` with `linear_float`; `linear_float` on a 16-bit
document's layer; `output_encoding` in `apply_raw_settings` and in
`save_raw_preset`'s `raw`; a tone curve applies at float, and grain
since #4, and `scripts/verify_v1x_raw_hdr.py` drives both), presence (nonzero Texture, Clarity or Dehaze
refused with `RawPresenceSettings.floatRefusal` by `open_document`,
`redevelop_raw`, `edit_raw_development` and `apply_raw_settings`; all three
at 0 opens and redevelops), local tone (a local `tone` refused with
`RawLocalTone.floatRefusal` by `edit_raw_development` on the float layer, and
a 16-bit development holding a tone local refuses `redevelop_raw` to
`linear_float`; the layers unchanged).

Profile Corrections. camera.NEF (NIKON D70s, Tamron AF 18-250mm A18 at 35 mm)
matches a Lensfun profile and the decoder has none of its own, so a new
development turns Profile Corrections on by default. That pincushion correction
leaves the same transparent strips at the left and right edges and the corners
in both developments: 48 of the 640 lattice windows are not opaque in the float
development and the same 48 in the 16-bit one, with no disagreement. And the
16-bit stage resamples codes where the float one resamples linear light, so the
two disagree at edges (V3b at +2 EV: 94.65 % of the differences <= 0.01). The
agreement, linearity, undo and range cases, which are about the float encoding
and not about lenses, therefore open with `lens_correction: false` (the six
opens of `open`, `work` and `ranged`, one equality chain). `radial local` keeps
the default, where a local over a corrected development must still leave the
outside exact.

Not exercised: the Develop window's rows, and the recorded place step's
missing default.

Usage: python3 scripts/verify_v1_raw_float.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import json
import time
import os
from pathlib import Path
import shutil
import statistics
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_v1_lens import App, SlowClient  # noqa: E402
from verify_v1_raw_output import SRGB_PRIMARIES, mat_vec, to_pcs  # noqa: E402

FIXTURE = Path.home() / 'devel/doctored-fixtures/camera/camera.NEF'
LIMIT = 8 * 1024 ** 3

LATTICE = (32, 20)
WINDOW = 5
EXPOSURES = (0, 1, 2)
SHOULDER_CODE = 65535 * (1.055 * 0.25 ** (1 / 2.4) - 0.055)
MARGIN = 0.02
MIN_CLASS = 20
HALF = (0.45, 0.55)
SHARE = 0.95

WHITE = (0.96422, 1.0, 0.82521)
SRGB_TO_PCS = to_pcs(SRGB_PRIMARIES, 'D65')
NARROW_REFUSAL = ('holds a 32-bit float Camera RAW development; redevelop it as a 16-bit SDR '
                  'master or rasterize it first')
FLOAT_POLICY = 'srgb-rgba32f-linear-v1'
# `RawPresenceSettings.floatRefusal`, word for word.
PRESENCE_REFUSAL = ('Texture, Clarity and Dehaze are defined on display-referred values and are '
                    'not available in a 32-bit float RAW development.')
# `RawLocalTone.floatRefusal`, the core's `FLOAT_TONE_REFUSAL`, word for word.
LOCAL_TONE_REFUSAL = 'RAW local tone controls are not available for a 32-bit float development.'


# ------------------------------------------------------------- the oracle --

def encode(linear):
    """sRGB encoding (IEC 61966-2.1) of a linear value clamped to 0…1."""
    value = min(max(linear, 0.0), 1.0)
    return value * 12.92 if value <= 0.0031308 else 1.055 * value ** (1 / 2.4) - 0.055


def lightness(linear):
    """l = L*/100 of one LINEAR sRGB sample, L* clamped to 0…100."""
    y = mat_vec(SRGB_TO_PCS, list(linear[:3]))[1] / WHITE[1]
    f = y ** (1 / 3) if y > 216 / 24389 else (24389 / 27 * y + 16) / 116
    return min(100.0, max(0.0, 116 * f - 16)) / 100


def within(values, low, high):
    return sum(1 for value in values if low <= value <= high) / len(values) if values else 0.0


def band_classes(values, low, high):
    """Lattice indices clear of a hard band's bounds by MARGIN: outside and
    inside. A point within MARGIN of a bound is in neither."""
    outside, inside = [], []
    for index, value in enumerate(values):
        if value < low - MARGIN or value > high + MARGIN:
            outside.append(index)
        elif low + MARGIN < value < high - MARGIN:
            inside.append(index)
    return outside, inside


def luminance(low, high):
    return dict(luminance=dict(low=low, high=high, low_falloff=0, high_falloff=0, invert=False))


def halved(lattice, base, indices, label):
    """The share of positive base channels of `indices` whose ratio to the
    base is in HALF, and how many were judged."""
    ratios = []
    for index in indices:
        for channel in range(3):
            if base[index][channel] > 1e-4:
                ratios.append(lattice[index][channel] / base[index][channel])
    share = within(ratios, *HALF)
    assert ratios and share >= SHARE, (label, len(ratios), share,
                                       statistics.median(ratios) if ratios else None)
    return dict(channels=len(ratios), share=share, median=statistics.median(ratios))


def assert_exact(lattice, base, indices, label):
    for index in indices:
        assert lattice[index] == base[index], (label, index, lattice[index], base[index])


# --------------------------------------------------------------- plumbing --

BUSY = 'Another background operation is running.'


class Run:
    """The shared state of one drive: the app, its folder and what the cases
    hand each other."""

    def __init__(self, app, folder):
        self.app = app
        self.folder = Path(folder)
        self.serial = 0
        self.size = None
        self.points = None
        self.floats = {}
        self.sdr = {}
        self.float_points = {}
        self.sdr_points = {}
        self.work = None
        self.work_lattice = None
        self.reopened = None
        self.ranged = None

    def copy(self, tag):
        self.serial += 1
        path = self.folder / f'{self.serial:02d}-{tag}.NEF'
        shutil.copyfile(FIXTURE, path)
        return path

    def open(self, tag, precision=None, **development):
        arguments = dict(path=str(self.copy(tag)), raw=development)
        if precision:
            arguments['precision'] = precision
        document = self.app.call('open_document', **arguments)['document']
        if self.size is None:
            self.size = (document['width'], document['height'])
            columns, rows = LATTICE
            self.points = [(int((i + 0.5) * self.size[0] / columns),
                            int((j + 0.5) * self.size[1] / rows))
                           for j in range(rows) for i in range(columns)]
        assert (document['width'], document['height']) == self.size, document
        return document

    def open_float(self, tag, **development):
        document = self.open(tag, output_encoding='linear_float', **development)
        assert document['precision'] == 'rgba32_float', document
        return document['id']

    def close(self, document):
        self.app.call('close_document', document_id=document, discard_changes=True)

    def source(self, document, layer=0):
        return self.app.call('get_raw_source', document_id=document, layer=layer)['source']

    def locals(self, document):
        return self.app.call('get_raw_locals', document_id=document, layer=0)

    def sample(self, document, x, y, layer=0, size=1):
        # A reply can return while the document's own background step (the
        # placed layer's projection) still runs; a read then answers BUSY. Wait
        # it out, as the other drives do, with a ceiling that only a stuck job
        # reaches.
        deadline = time.monotonic() + 60
        while True:
            try:
                return self.app.call('sample_pixel', document_id=document, source='layer',
                                     layer=layer, x=x, y=y, sample_size=size)
            except AssertionError as error:
                if BUSY not in str(error) or time.monotonic() >= deadline:
                    raise
                time.sleep(0.05)

    def float_lattice(self, document, layer=0):
        """The centre sample of every lattice point: linear RGBA."""
        return [self.sample(document, x, y, layer)['center_rgba32'] for x, y in self.points]

    def float_windows(self, document):
        """Per lattice point, over its 5×5 square: the mean linear sample and
        the mean of the per-pixel encodings of the clamped samples (what a
        16-bit code averages), as the M1 probe computes them."""
        reach = WINDOW // 2
        points = []
        for x, y in self.points:
            linear, encoded = [0.0] * 3, [0.0] * 3
            for dy in range(-reach, reach + 1):
                for dx in range(-reach, reach + 1):
                    value = self.sample(document, x + dx, y + dy)['center_rgba32']
                    assert value[3] == 1, value
                    for channel in range(3):
                        linear[channel] += value[channel]
                        encoded[channel] += encode(value[channel])
            count = WINDOW * WINDOW
            points.append(dict(linear=[v / count for v in linear],
                               encoded=[v / count for v in encoded]))
        return points

    def sdr_windows(self, document):
        """Per lattice point: the mean 16-bit code of its 5×5 square and the
        centre pixel's codes."""
        points = []
        for x, y in self.points:
            reply = self.sample(document, x, y, size=WINDOW)
            assert reply['sample_count'] == WINDOW * WINDOW, reply
            points.append(dict(mean=reply['mean_rgba_codes'][:3],
                               center=reply['center_rgba_codes'][:3]))
        return points

    def is_float_master(self, document, layer=0):
        source = self.source(document, layer)
        recipe = source['raw']['recipe']
        assert source['valid'] is True, source
        if source['master_depth'] == 32:
            assert source['output_encoding'] == 'linear_float', source
            # Tone curve 0 keeps the -v1 policy; a curve above 0 writes -v2.
            version = '-v1' if recipe['values']['toneCurve'] == 0 else '-v2'
            assert recipe['outputSpace'].endswith('-rgba32f-linear' + version), (
                recipe['outputSpace'], recipe['values'])
            return True
        assert source['master_depth'] == 16 and source['output_encoding'] == 'sdr16', source
        assert recipe['outputSpace'].endswith('-rgba16-normalized-v1'), recipe['outputSpace']
        return False


# ----------------------------------------------------------------- cases --

def open_case(run):
    for exposure in EXPOSURES:
        run.floats[exposure] = run.open_float(f'float{exposure}', exposure=exposure,
                                              lens_correction=False)
        document = run.open(f'sdr{exposure}', exposure=exposure, tone_curve=0,
                            lens_correction=False)
        assert document['precision'] == 'rgba16', document
        run.sdr[exposure] = document['id']
        source = run.source(run.floats[exposure])
        assert run.is_float_master(run.floats[exposure]), source
        assert source['depth'] == 32 and source['fallback_depth'] == 32, source
        assert source['raw']['recipe']['outputSpace'] == FLOAT_POLICY, source['raw']['recipe']
        assert source['raw']['recipe']['values']['exposure'] == exposure
        assert source['original_embedded'] is True, source
        assert not run.is_float_master(run.sdr[exposure])
        assert run.source(run.sdr[exposure])['raw']['recipe']['values']['toneCurve'] == 0
    for exposure in EXPOSURES:
        run.float_points[exposure] = run.float_windows(run.floats[exposure])
        run.sdr_points[exposure] = run.sdr_windows(run.sdr[exposure])
    return dict(size=list(run.size), lattice=len(run.points),
                edr=run.source(run.floats[0])['raw']['recipe']['extendedDynamicRange'])


def highlights_case(run):
    plus_two = [point['linear'] for point in run.float_points[2]]
    largest = max(max(point) for point in plus_two)
    above = sum(1 for point in plus_two if max(point) > 1.0)
    assert largest >= 1.5 and above >= MIN_CLASS, (largest, above)
    centers = [point['center'] for point in run.sdr_points[2]]
    assert all(code <= 65535 for point in centers for code in point)
    clipped = sum(1 for point in centers if max(point) == 65535)
    assert clipped >= MIN_CLASS, clipped
    return dict(float_largest=largest, float_points_above_one=above, sdr_points_at_65535=clipped)


def doubling_case(run):
    base = [point['linear'] for point in run.float_points[0]]
    codes = [point['mean'] for point in run.sdr_points[0]]
    chosen = [index for index, point in enumerate(base)
              if 0.02 <= max(point) <= 0.6 and all(code < 60000 for code in codes[index])]
    assert len(chosen) >= MIN_CLASS, len(chosen)
    detail = dict(points=len(chosen))
    for exposure, median_range, share_range in ((1, (1.98, 2.02), (1.9, 2.1)),
                                                (2, (3.92, 4.08), (3.6, 4.4))):
        ratios = []
        for index in chosen:
            for channel in range(3):
                low = base[index][channel]
                high = run.float_points[exposure][index]['linear'][channel]
                ratios.append(high / low if low > 0 else float('inf'))
        median = statistics.median(ratios)
        share = within(ratios, *share_range)
        assert median_range[0] <= median <= median_range[1] and share >= SHARE, (
            exposure, median, share)
        detail[f'plus_{exposure}'] = dict(median=median, share=share, ratios=len(ratios))
    return detail


def agreement_case(run):
    """V3a, V3b and V3c on the probe's own 5×5 means (module docstring)."""
    detail = {}
    for exposure in EXPOSURES:
        differences, largest_channel, per_channel = [], [], []
        for index, reference in enumerate(run.sdr_points[exposure]):
            codes = reference['mean']
            encoded = run.float_points[exposure][index]['encoded']
            signed = [encoded[channel] - codes[channel] / 65535 for channel in range(3)]
            below = all(code < SHOULDER_CODE for code in codes)
            if not below:
                per_channel.extend(signed)
                largest_channel.append(max(encoded) - max(codes) / 65535)
            if all(256 <= code <= 65279 for code in codes) and (below or exposure == 0):
                differences.extend(abs(value) for value in signed)
        median = statistics.median(differences) if differences else float('nan')
        share = within(differences, 0.0, 0.01)
        name = 'V3a' if exposure == 0 else 'V3b'
        assert len(differences) >= 3 * MIN_CLASS and median <= 0.002 and share >= SHARE, (
            name, exposure, len(differences), median, share)
        smallest = min(largest_channel) if largest_channel else 0.0
        assert all(value >= -0.01 for value in largest_channel), ('V3c', exposure, smallest)
        detail[f'plus_{exposure}'] = dict(
            statement=name, differences=len(differences), median=median, share=share,
            above_shoulder_points=len(largest_channel), v3c_smallest_largest_channel=smallest,
            per_channel_below=sum(1 for value in per_channel if value < -0.01),
            per_channel_smallest=min(per_channel) if per_channel else None)
    assert detail['plus_2']['above_shoulder_points'] >= MIN_CLASS, detail['plus_2']
    return detail


def undo_case(run):
    run.work = run.open_float('work', lens_correction=False)
    run.work_lattice = run.float_lattice(run.work)
    assert run.work_lattice == run.float_lattice(run.floats[0]), 'two float developments differ'
    reply = run.app.call('redevelop_raw', document_id=run.work, layer=0, raw=dict(exposure=0.5))
    assert reply['ok'] is True, reply
    assert run.is_float_master(run.work)
    changed = run.float_lattice(run.work)
    moved = sum(1 for a, b in zip(changed, run.work_lattice) if a != b)
    assert moved >= len(run.points) // 2, moved
    run.app.call('undo', document_id=run.work)
    assert run.float_lattice(run.work) == run.work_lattice, 'one undo did not restore the lattice'
    assert run.source(run.work)['raw']['recipe']['values']['exposure'] == 0
    run.app.call('redo', document_id=run.work)
    assert run.float_lattice(run.work) == changed, 'redo did not restore the redevelopment'
    return dict(points_changed=moved)


def rz_case(run):
    before = run.float_lattice(run.work)
    source = run.source(run.work)
    path = run.folder / 'float.rz'
    run.app.call('save_copy', document_id=run.work, path=str(path))
    run.close(run.work)
    document = run.app.call('open_document', path=str(path))['document']
    assert document['precision'] == 'rgba32_float', document
    run.reopened = document['id']
    assert run.is_float_master(run.reopened)
    reopened = run.source(run.reopened)
    for key in ('master_depth', 'fallback_depth', 'width', 'height', 'profile', 'storage'):
        assert reopened[key] == source[key], (key, reopened[key], source[key])
    assert reopened['raw'] == source['raw'], 'the recipe or the camera file changed'
    assert run.float_lattice(run.reopened) == before, 'the reopened lattice differs'
    reply = run.app.call('redevelop_raw', document_id=run.reopened, layer=0,
                         raw=dict(exposure=0.25))
    assert reply['ok'] is True and run.is_float_master(run.reopened), reply
    assert run.float_lattice(run.reopened) != before
    return dict(bytes=path.stat().st_size)


def preset_sync_case(run):
    name = f'v1-raw-float-{os.getpid()}'
    # 0.375 is exact in binary32, the precision the recipe records a decoder value at.
    run.app.call('save_raw_preset', name=name, raw=dict(exposure=0.375), replace=True)
    before = run.float_lattice(run.reopened)
    run.app.call('apply_raw_preset', name=name, document_id=run.reopened, layer=0)
    assert run.is_float_master(run.reopened)
    assert run.source(run.reopened)['raw']['recipe']['values']['exposure'] == 0.375
    assert run.float_lattice(run.reopened) != before
    run.app.call('delete_raw_preset', name=name)
    # Sync from the +1 EV float document: its exposure, never its encoding.
    run.app.call('sync_raw_settings', document_id=run.floats[1], layer=0, fields=['exposure'],
                 targets=[dict(document_id=run.reopened, layer=0)])
    assert run.is_float_master(run.reopened)
    assert run.source(run.reopened)['raw']['recipe']['values']['exposure'] == 1
    synced = run.float_lattice(run.reopened)
    assert synced != before, 'the sync changed nothing'
    # From a 16-bit source too: the target stays a float development.
    run.app.call('sync_raw_settings', document_id=run.sdr[2], layer=0, fields=['exposure'],
                 targets=[dict(document_id=run.reopened, layer=0)])
    assert run.is_float_master(run.reopened)
    assert run.source(run.reopened)['raw']['recipe']['values']['exposure'] == 2
    return dict(preset=name)


def place_case(run):
    target = run.reopened
    depths = {}
    for tag, raw, expected in (('default', dict(), True),
                               ('sdr16', dict(output_encoding='sdr16'), False),
                               ('tone', dict(tone_curve=0.5), True),
                               ('float', dict(output_encoding='linear_float', exposure=2), True)):
        reply = run.app.call('place_image', document_id=target, path=str(run.copy(f'place-{tag}')),
                             name=f'placed-{tag}', raw=raw)
        layer = reply['layer']
        assert reply['document']['precision'] == 'rgba32_float', reply
        assert run.is_float_master(target, layer) is expected, (tag, run.source(target, layer))
        depths[tag] = run.source(target, layer)['master_depth']
        if tag == 'tone':
            assert run.source(target, layer)['raw']['recipe']['values']['toneCurve'] == 0.5
        if tag == 'float':
            values = run.float_lattice(target, layer)
            above = sum(1 for point in values if max(point[:3]) > 1.0)
            assert above >= MIN_CLASS, above
            depths['float_points_above_one'] = above
        if tag == 'sdr16':
            # A 16-bit master rendered into linear light never exceeds white.
            values = run.float_lattice(target, layer)
            assert all(max(point[:3]) <= 1.0 + 1e-6 for point in values)
    assert run.is_float_master(target, 0), 'a place changed the first layer'
    # Into a 16-bit document the float development is refused by name.
    text = run.app.refused('place_image', 'raw.output_encoding linear_float', '32-bit float',
                           document_id=run.sdr[0], path=str(run.copy('place-refused')),
                           raw=dict(output_encoding='linear_float'))
    return dict(master_depths=depths, refusal=text)


def range_case(run):
    run.ranged = run.open_float('ranged', lens_correction=False)
    base = run.float_lattice(run.ranged)
    assert base == run.work_lattice, 'two float developments differ'
    values = [lightness(point) for point in base]
    ordered = sorted(values)
    low = round(ordered[int(0.35 * (len(ordered) - 1))], 3)
    high = round(ordered[int(0.65 * (len(ordered) - 1))], 3)
    assert 0 <= low < high <= 1, (low, high)
    reply = run.app.call('edit_raw_development', document_id=run.ranged, layer=0, operations=[
        dict(op='create', id='range1', mask=dict(op='full'), exposure_offset=-1,
             range=luminance(low, high))])
    assert reply['ok'] is True and reply['unchanged'] is False, reply
    assert run.is_float_master(run.ranged)
    nodes = run.locals(run.ranged)['nodes']
    assert len(nodes) == 1 and nodes[0]['range'] == luminance(low, high), nodes
    lattice = run.float_lattice(run.ranged)
    outside, inside = band_classes(values, low, high)
    assert len(outside) >= MIN_CLASS and len(inside) >= MIN_CLASS, (len(outside), len(inside))
    assert_exact(lattice, base, outside, 'outside the band')
    detail = halved(lattice, base, inside, 'inside the band')
    # A point above white is inside `high: 1`: L* clamps to 100. At +3 EV, where
    # hundreds of lattice points have Y >= 1 (at +2 EV only a handful do).
    run.app.call('undo', document_id=run.ranged)
    assert run.float_lattice(run.ranged) == base, 'undo did not restore the base'
    bright = run.open_float('bright', exposure=3)
    bright_base = run.float_lattice(bright)
    run.app.call('edit_raw_development', document_id=bright, layer=0, operations=[
        dict(op='create', id='high1', mask=dict(op='full'), exposure_offset=-1,
             range=luminance(0.9, 1.0))])
    bright_lattice = run.float_lattice(bright)
    over = [index for index, point in enumerate(bright_base)
            if lightness(point) >= 1.0 and max(point[:3]) > 1.0]
    assert len(over) >= MIN_CLASS, len(over)
    changed = sum(1 for index in over if bright_lattice[index] != bright_base[index])
    assert changed == len(over), (changed, len(over))
    run.close(bright)
    run.app.call('redo', document_id=run.ranged)
    assert run.float_lattice(run.ranged) == lattice, 'redo did not restore the local'
    return dict(band=[low, high], outside_exact=len(outside), inside=len(inside),
                above_white_inside=len(over), **detail)


def radial_case(run):
    document = run.open_float('radial')
    base = run.float_lattice(document)
    width, height = run.size
    center = [width / 2, height / 2]
    radius = [width * 0.3, height * 0.3]
    hardness = 0.8
    reply = run.app.call('edit_raw_development', document_id=document, layer=0, operations=[
        dict(op='create', id='radial1', exposure_offset=-1,
             mask=dict(op='radial', center=center, radius=radius, hardness=hardness))])
    assert reply['unchanged'] is False and run.is_float_master(document), reply
    lattice = run.float_lattice(document)
    beyond, core = [], []
    for index, (x, y) in enumerate(run.points):
        d = (((x + 0.5 - center[0]) / radius[0]) ** 2
             + ((y + 0.5 - center[1]) / radius[1]) ** 2) ** 0.5
        if d > 1.01:
            beyond.append(index)
        elif d < hardness - 0.01:
            core.append(index)
    assert len(beyond) >= MIN_CLASS and len(core) >= MIN_CLASS, (len(beyond), len(core))
    assert_exact(lattice, base, beyond, 'outside the ellipse')
    detail = halved(lattice, base, core, 'in the ellipse core')
    run.close(document)
    return dict(outside_exact=len(beyond), core=len(core), **detail)


def switch_case(run):
    """On the layer with its range local: sdr16 and back, the locals kept."""
    locals_before = run.locals(run.ranged)['nodes']
    float_lattice = run.float_lattice(run.ranged)
    reply = run.app.call('redevelop_raw', document_id=run.ranged, layer=0,
                         raw=dict(output_encoding='sdr16'))
    assert reply['ok'] is True, reply
    assert not run.is_float_master(run.ranged)
    assert reply['document']['precision'] == 'rgba32_float', reply
    nodes = run.locals(run.ranged)['nodes']
    assert [node['id'] for node in nodes] == ['range1'], nodes
    assert nodes[0]['range'] == locals_before[0]['range'], nodes
    sdr_lattice = run.float_lattice(run.ranged)
    assert all(max(point[:3]) <= 1.0 + 1e-6 for point in sdr_lattice)
    reply = run.app.call('redevelop_raw', document_id=run.ranged, layer=0,
                         raw=dict(output_encoding='linear_float'))
    assert reply['ok'] is True and run.is_float_master(run.ranged), reply
    nodes = run.locals(run.ranged)['nodes']
    assert [node['id'] for node in nodes] == ['range1'], nodes
    assert nodes[0]['range'] == locals_before[0]['range'], nodes
    assert run.float_lattice(run.ranged) == float_lattice, 'the float development did not return'
    return dict(locals=len(nodes))


def p3_case(run):
    document = run.open_float('p3', output_space='display_p3')
    source = run.source(document)
    assert run.is_float_master(document)
    assert source['raw']['recipe']['outputSpace'] == 'display-p3-rgba32f-linear-v1', source['raw']
    assert 'P3' in source['profile'], source['profile']
    profile = run.app.call('get_color_profile', document_id=document)
    assert 'P3' in json.dumps(profile), profile
    srgb = run.app.call('get_color_profile', document_id=run.floats[0])
    assert 'P3' not in json.dumps(srgb), srgb
    # The same scene in wider primaries: the numbers differ from the sRGB open's.
    assert run.float_lattice(document) != run.work_lattice
    run.close(document)
    return dict(profile=source['profile'])


def narrowing_case(run):
    document = run.open_float('narrow')
    sixteen = run.open('paste16', tone_curve=0)['id']
    eight = run.open('paste8', precision='rgba8', tone_curve=0)
    assert eight['precision'] == 'rgba8', eight
    eight = eight['id']
    refusals = [run.app.refused('convert_precision', NARROW_REFUSAL, document_id=document,
                                mode='rgba16', structure='preserve')]
    run.app.call('copy', document_id=document, mode='layers', layers=[0])
    for target in (sixteen, eight):
        count = run.app.call('get_document', document_id=target)['layer_count']
        refusals.append(run.app.refused('paste', NARROW_REFUSAL, document_id=target,
                                        mode='layers'))
        assert run.app.call('get_document', document_id=target)['layer_count'] == count
    assert run.is_float_master(document), 'a refusal changed the layer'
    run.app.call('redevelop_raw', document_id=document, layer=0,
                 raw=dict(output_encoding='sdr16'))
    assert not run.is_float_master(document)
    run.app.call('copy', document_id=document, mode='layers', layers=[0])
    for target in (sixteen, eight):
        count = run.app.call('get_document', document_id=target)['layer_count']
        run.app.call('paste', document_id=target, mode='layers')
        after = run.app.call('get_document', document_id=target)
        assert after['layer_count'] == count + 1, after['layer_count']
        assert run.source(target, after['active_layer'])['master_depth'] == 16
    converted = run.app.call('convert_precision', document_id=document, mode='rgba16',
                             structure='preserve')
    assert converted['document']['precision'] == 'rgba16', converted
    assert run.source(document)['master_depth'] == 16
    for target in (document, sixteen, eight):
        run.close(target)
    return dict(refusals=refusals)


def refusal_case(run):
    texts = []
    for needle, arguments in (
            ('precision rgba8', dict(raw=dict(output_encoding='linear_float'),
                                     precision='rgba8')),
            ('raw.output_encoding must be', dict(raw=dict(output_encoding='hdr')))):
        before = len(run.app.call('list_documents')['documents'])
        texts.append(run.app.refused('open_document', needle,
                                     path=str(run.copy('refused')), **arguments))
        assert len(run.app.call('list_documents')['documents']) == before, 'a refusal opened a document'
    source = run.source(run.sdr[0])
    texts.append(run.app.refused('redevelop_raw', 'raw.output_encoding linear_float',
                                 '32-bit float document', document_id=run.sdr[0], layer=0,
                                 raw=dict(output_encoding='linear_float')))
    texts.append(run.app.refused('edit_raw_development', 'raw.output_encoding linear_float',
                                 document_id=run.sdr[0], layer=0, operations=[],
                                 raw=dict(output_encoding='linear_float')))
    assert run.source(run.sdr[0]) == source, 'a refusal changed the 16-bit layer'
    float_source = run.source(run.floats[0])
    needle = 'raw.output_encoding is not a preset or Sync control'
    texts.append(run.app.refused('apply_raw_settings', needle, document_id=run.floats[0],
                                 layer=0, raw=dict(output_encoding='sdr16', exposure=0.5)))
    texts.append(run.app.refused('save_raw_preset', needle, name=f'v1-raw-float-no-{os.getpid()}',
                                 raw=dict(output_encoding='linear_float', exposure=0.5)))
    assert run.source(run.floats[0]) == float_source, 'a refusal changed the float layer'
    return dict(refusals=texts)


def presence_case(run):
    """Nonzero Texture, Clarity or Dehaze is refused at the float development
    by `RawPresenceSettings.floatRefusal` on every MCP path; all three at 0
    develops."""
    texts = []
    before = len(run.app.call('list_documents')['documents'])
    texts.append(run.app.refused('open_document', PRESENCE_REFUSAL,
                                 path=str(run.copy('presence-refused')),
                                 raw=dict(output_encoding='linear_float', clarity=20)))
    assert len(run.app.call('list_documents')['documents']) == before, 'a refusal opened a document'
    float_source = run.source(run.floats[0])
    texts.append(run.app.refused('redevelop_raw', PRESENCE_REFUSAL, document_id=run.floats[0],
                                 layer=0, raw=dict(clarity=20)))
    texts.append(run.app.refused('edit_raw_development', PRESENCE_REFUSAL,
                                 document_id=run.floats[0], layer=0, operations=[],
                                 raw=dict(texture=20)))
    texts.append(run.app.refused('apply_raw_settings', PRESENCE_REFUSAL, document_id=run.floats[0],
                                 layer=0, raw=dict(dehaze=20)))
    assert run.source(run.floats[0]) == float_source, 'a refusal changed the float layer'
    document = run.open_float('presence-zero', texture=0, clarity=0, dehaze=0)
    assert run.is_float_master(document)
    reply = run.app.call('redevelop_raw', document_id=document, layer=0,
                         raw=dict(exposure=0.5, texture=0, clarity=0, dehaze=0))
    assert reply['ok'] is True, reply
    assert run.is_float_master(document)
    recipe = run.source(document)['raw']['recipe']
    assert recipe['values']['exposure'] == 0.5, recipe['values']
    assert not (recipe.get('enhancements') or {}).get('presence'), recipe.get('enhancements')
    run.close(document)
    return dict(refusals=texts)


def local_tone_case(run):
    """A local tone refuses at the float development by name: requested on the
    float layer, and saved on a 16-bit development switched to linear_float."""
    width, height = run.size
    radial = dict(op='radial', center=[width / 2, height / 2],
                  radius=[width * 0.3, height * 0.3], hardness=0.8)
    float_source = run.source(run.floats[0])
    texts = [run.app.refused('edit_raw_development', LOCAL_TONE_REFUSAL,
                             document_id=run.floats[0], layer=0, operations=[
                                 dict(op='create', id='tone1', tone=dict(exposure=1),
                                      mask=radial)])]
    assert run.source(run.floats[0]) == float_source, 'a refusal changed the float layer'
    document = run.open_float('local-tone')
    reply = run.app.call('redevelop_raw', document_id=document, layer=0,
                         raw=dict(output_encoding='sdr16'))
    assert reply['ok'] is True and not run.is_float_master(document), reply
    reply = run.app.call('edit_raw_development', document_id=document, layer=0, operations=[
        dict(op='create', id='tone1', tone=dict(exposure=1), mask=radial)])
    assert reply['unchanged'] is False, reply
    sdr_source = run.source(document)
    texts.append(run.app.refused('redevelop_raw', LOCAL_TONE_REFUSAL, document_id=document,
                                 layer=0, raw=dict(output_encoding='linear_float')))
    assert run.source(document) == sdr_source, 'a refusal changed the 16-bit layer'
    run.close(document)
    return dict(refusals=texts)


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, the copies and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-raw-float-'))
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
            run_case('open', lambda: open_case(run))
            run_case('highlights', lambda: highlights_case(run))
            run_case('doubling', lambda: doubling_case(run))
            run_case('sdr agreement', lambda: agreement_case(run))
            run_case('undo', lambda: undo_case(run))
            run_case('rz', lambda: rz_case(run))
            run_case('preset and sync', lambda: preset_sync_case(run))
            run_case('place', lambda: place_case(run))
            run_case('range local', lambda: range_case(run))
            run_case('radial local', lambda: radial_case(run))
            run_case('encoding switch', lambda: switch_case(run))
            run_case('p3', lambda: p3_case(run))
            run_case('narrowing', lambda: narrowing_case(run))
            run_case('refusals', lambda: refusal_case(run))
            run_case('presence', lambda: presence_case(run))
            run_case('local tone', lambda: local_tone_case(run))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
