#!/usr/bin/env python3
"""MCP drive for the Camera RAW output spaces Adobe RGB (1998) and ProPhoto RGB:
`open_document`, `redevelop_raw`, the RAW presets and the `.rz` round trip with
`raw.output_space` set to `adobe_rgb` and `prophoto_rgb`.

The script owns its one app instance: the Debug app (or `DR_VERIFY_APP`) on
its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, an 8 GiB working limit and private `DR_*_DIR`s
(`verify_v1_lens.App`). It terminates only that process. The fixture is
`~/devel/doctored-fixtures/camera/camera.NEF`; an absent fixture FAILS the
run. Every document is opened from its own copy of the file.

The oracle is this file's own and reads nothing back from the app but pixel
codes: each space is its published data — xy primaries, native white (D65
through the Bradford adaptation to the PCS D50 for sRGB and Adobe RGB; the PCS
D50 itself for ProPhoto) and tone curve (IEC 61966-2.1; a pure power of
563/256; a pure power of 461/256) — and the wide→sRGB matrix is
`inv(sRGB) · wide`, computed here in floating point from those primaries.

ProPhoto RGB here is the pure 1.8 power with no linear toe, not the ROMM RGB
encoding of ISO 22028-2. The app renders the master into the system's named
ROMM RGB space, which Core Image encodes as a pure 1.8 power, and tags it
with the built-in profile, whose count-1 `curv` holds 461/256. Two earlier
forms failed `oracle prophoto_rgb` at this bound: rendering into the built-in
profile's own space (Core Image added the ROMM toe: 4.9e-4 at a deep shadow),
and a sampled ROMM table in the profile (Core Image approximated the table:
7.4e-4 at a midtone). The bound and the lattice did not change.

Bounds. LINEAR_BOUND, the lattice and MIN_INTERIOR were fixed before the first
run and never widened; MIN_CLAMPED and the CLAMP case's development came later,
with that case (see `CLAMP_DEVELOPMENT`):
- LINEAR_BOUND = 4e-4, absolute, in linear sRGB light, per channel;
- an INTERIOR lattice point has every sRGB code in 256…65279 and every wide
  code in 1…65534; the ORACLE case requires at least MIN_INTERIOR = 200 of
  the 640;
- a CLAMPED lattice point has an sRGB channel at 0 or 65535 and every wide
  code in 1…65534; the CLAMP case requires at least MIN_CLAMPED = 10 channels
  held at 0 and as many held at 65535.

Cases, for each of `adobe_rgb` and `prophoto_rgb`:
- OPEN 16: `open_document` with `precision: preserve`, `raw: {output_space}`.
  `get_raw_source` reports the policy string, the profile's name and depth
  16; `get_color_profile` names the same profile; a `save_copy` PNG's `iCCP`
  chunk, parsed here, holds that `desc` and a count-1 `curv` with the gamma's
  raw value (`0x0233` for Adobe RGB, `0x01CD` for ProPhoto).
- ORACLE: against a second copy opened with `srgb`, `sample_pixel`
  (`source: layer`) on a 32×20 lattice. The wide codes are decoded by the
  profile's own power and taken through the wide→sRGB matrix. At an interior point
  that equals the IEC-decoded sRGB codes within LINEAR_BOUND. At a point where
  an sRGB channel sits at 0 or 65535 and the wide codes are unclipped, the
  oracle is ≤ LINEAR_BOUND or ≥ 1 − LINEAR_BOUND in that channel: the
  per-channel clamp of a colour outside sRGB. The count of each class goes in
  the receipt. The fixture's as-shot development has NO clamped point (all 640
  are interior), so here that second check runs on nothing; CLAMP is the case
  that reaches it.
- CLAMP: the same two opens with `raw: {exposure: 2.0, temperature: 3000}`
  added, a development that pushes the highlights past white and casts the
  picture blue. At least MIN_CLAMPED channels of the clamped points sit at 0
  in sRGB and at least MIN_CLAMPED at 65535, the oracle is ≤ LINEAR_BOUND in
  each of the first and ≥ 1 − LINEAR_BOUND in each of the second, and the
  interior points stay within LINEAR_BOUND. The counts and the two worst
  oracle values go in the receipt.
- OPEN 8: `precision: rgba8`. The source reports depth 16, fallback depth 8
  and the space's profile; `get_color_profile` equals an 8-bit `srgb` open's.
- REDEVELOP: on a 16-bit `srgb` open, `redevelop_raw {output_space}`: the
  recipe and the source profile change, `get_color_profile` does not, the
  interior lattice points stay within LINEAR_BOUND of the sRGB development,
  and `undo` restores the sRGB recipe.
- PRESET: `save_raw_preset` with `fields: [output_space]` from the wide layer;
  `list_raw_presets` shows `output_space` as the raw value; `apply_raw_preset`
  gives an `srgb` layer the policy and the profile; a preset holding only
  `exposure`, applied to a wide layer, leaves its output space alone.
- RZ: `save_copy` to `.rz`, close, reopen: `get_raw_source` is equal, the
  lattice codes are identical, and `redevelop_raw {exposure}` then succeeds
  with the output space kept.
- REFUSAL: `redevelop_raw` with `raw: {output_space: "rec2020"}` is refused
  with "raw.output_space must be srgb, display_p3, adobe_rgb or prophoto_rgb
  (normalized SDR)." and `get_raw_source` is unchanged.

Usage: python3 scripts/verify_v1_raw_output.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import json
from pathlib import Path
import shutil
import struct
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
from verify_v1_lens import App, SlowClient  # noqa: E402

FIXTURE = Path.home() / 'devel/doctored-fixtures/camera/camera.NEF'
LIMIT = 8 * 1024 ** 3

LINEAR_BOUND = 4e-4
MIN_INTERIOR = 200
MIN_CLAMPED = 10
LATTICE = (32, 20)

# The CLAMP case's development: two stops over, and a 3000 K neutral, which
# renders the picture blue. On the fixture it holds sRGB channels at 0 and at
# 65535 at lattice points whose Adobe RGB and ProPhoto codes are unclipped.
# It was picked from six developments tried in one exploration run when the
# case was added: all six pass both clamp checks, this is the one with useful
# counts at both ends in both spaces, and LINEAR_BOUND did not move.
CLAMP_DEVELOPMENT = dict(exposure=2.0, temperature=3000)

REFUSAL = ('raw.output_space must be srgb, display_p3, adobe_rgb or prophoto_rgb '
           '(normalized SDR).')

SRGB_POLICY = 'srgb-rgba16-normalized-v1'
SRGB_NAME = 'sRGB IEC61966-2.1'
SRGB_PRIMARIES = ((0.64, 0.33), (0.30, 0.60), (0.15, 0.06))

SPACES = {
    'adobe_rgb': dict(
        name='Adobe RGB (1998)', policy='adobe-rgb-1998-rgba16-normalized-v1',
        primaries=((0.64, 0.33), (0.21, 0.71), (0.15, 0.06)), white='D65', gamma_raw=0x0233),
    'prophoto_rgb': dict(
        name='ProPhoto RGB', policy='prophoto-rgb-rgba16-normalized-v1',
        primaries=((0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001)), white='D50',
        gamma_raw=0x01CD),
}


# ------------------------------------------------------------- the oracle --

D50 = [0.9642, 1.0, 0.8249]
BRADFORD = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]]


def mat_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def mat_vec(a, v):
    return [sum(a[i][k] * v[k] for k in range(3)) for i in range(3)]


def inv3(m):
    det = (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
           - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
           + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))
    out = [[0.0] * 3 for _ in range(3)]
    for r in range(3):
        for c in range(3):
            r0, r1 = (c + 1) % 3, (c + 2) % 3
            c0, c1 = (r + 1) % 3, (r + 2) % 3
            out[r][c] = (m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]) / det
    return out


def xy_xyz(x, y):
    return [x / y, 1.0, (1 - x - y) / y]


def to_pcs(primaries, white):
    """Linear RGB -> PCS XYZ (D50) from the xy primaries: each primary scaled so
    the three sum to the native white, then Bradford-adapted when that white is
    D65."""
    native_white = xy_xyz(0.3127, 0.3290) if white == 'D65' else D50
    columns = [xy_xyz(x, y) for x, y in primaries]
    basis = [[columns[c][r] for c in range(3)] for r in range(3)]
    scale = mat_vec(inv3(basis), native_white)
    native = [[basis[r][c] * scale[c] for c in range(3)] for r in range(3)]
    if white == 'D50':
        return native
    source, destination = mat_vec(BRADFORD, native_white), mat_vec(BRADFORD, D50)
    gain = [[destination[i] / source[i] if i == j else 0.0 for j in range(3)] for i in range(3)]
    return mat_mul(mat_mul(inv3(BRADFORD), mat_mul(gain, BRADFORD)), native)


def wide_to_srgb(space):
    entry = SPACES[space]
    return mat_mul(inv3(to_pcs(SRGB_PRIMARIES, 'D65')), to_pcs(entry['primaries'], entry['white']))


def iec_decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def srgb_linear(codes):
    return [iec_decode(c / 65535) for c in codes[:3]]


def wide_decode(space, v):
    """The profile's own curve: a pure power of its u8Fixed8 gamma."""
    return v ** (SPACES[space]['gamma_raw'] / 256)


def oracle(space, codes):
    """Linear sRGB of one wide pixel: the space's curve, then the matrix."""
    return mat_vec(wide_to_srgb(space), [wide_decode(space, c / 65535) for c in codes[:3]])


# ------------------------------------------------------------ PNG and ICC --

def png_profile(path):
    """The inflated `iCCP` profile of a PNG, walking its chunks by hand."""
    data = Path(path).read_bytes()
    assert data[:8] == b'\x89PNG\r\n\x1a\n', 'not a PNG'
    at = 8
    while at < len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b'iCCP':
            end = body.index(b'\x00')
            assert body[end + 1] == 0, 'iCCP compression method'
            return zlib.decompress(body[end + 2:])
        at += 12 + length
    raise AssertionError('the PNG carries no iCCP chunk')


def icc_tag(profile, signature):
    count = struct.unpack('>I', profile[128:132])[0]
    for index in range(count):
        entry = 132 + 12 * index
        if profile[entry:entry + 4] == signature:
            offset, size = struct.unpack('>II', profile[entry + 4:entry + 12])
            return profile[offset:offset + size]
    raise AssertionError(f'the profile has no {signature!r} tag')


def icc_facts(profile):
    """The `desc` text and the `curv` entries the three TRC tags share: one
    u8Fixed8 gamma for a count of 1."""
    desc = icc_tag(profile, b'desc')
    assert desc[:4] == b'desc', desc[:4]
    length = struct.unpack('>I', desc[8:12])[0]
    name = desc[12:12 + length - 1].decode('ascii')
    curves = []
    for signature in (b'rTRC', b'gTRC', b'bTRC'):
        curve = icc_tag(profile, signature)
        assert curve[:4] == b'curv', curve[:4]
        count = struct.unpack('>I', curve[8:12])[0]
        assert len(curve) == 12 + 2 * count, (len(curve), count)
        curves.append(list(struct.unpack(f'>{count}H', curve[12:])))
    assert curves[0] == curves[1] == curves[2], 'the three TRC tags differ'
    return name, curves[0]


def expected_curve(space):
    """The `curv` entries the space's profile must hold: the one gamma."""
    return [SPACES[space]['gamma_raw']]


# --------------------------------------------------------------- plumbing --

class Run:
    """The shared state of one drive: the app, its folder and what the cases
    hand each other."""

    def __init__(self, app, folder):
        self.app = app
        self.folder = Path(folder)
        self.serial = 0
        self.srgb16 = None
        self.srgb16_lattice = None
        self.srgb8_profile = None
        self.size = None
        self.wide16 = {}
        self.wide16_lattice = {}
        self.wide8 = {}
        self.interior = {}
        self.clamp_srgb_lattice = None

    def copy(self, tag):
        self.serial += 1
        path = self.folder / f'{self.serial:02d}-{tag}.NEF'
        shutil.copyfile(FIXTURE, path)
        return path

    def open(self, tag, depth, space, **development):
        reply = self.app.call('open_document', path=str(self.copy(tag)),
                              precision='preserve' if depth == 16 else 'rgba8',
                              raw=dict(development, output_space=space))
        document = reply['document']
        if self.size is None:
            self.size = (document['width'], document['height'])
        assert (document['width'], document['height']) == self.size, document
        return document['id']

    def close(self, document):
        self.app.call('close_document', document_id=document, discard_changes=True)

    def source(self, document):
        return self.app.call('get_raw_source', document_id=document, layer=0)['source']

    def profile(self, document):
        return self.app.call('get_color_profile', document_id=document)

    def lattice(self, document):
        width, height = self.size
        columns, rows = LATTICE
        points = [(int((i + 0.5) * width / columns), int((j + 0.5) * height / rows))
                  for j in range(rows) for i in range(columns)]
        return [self.app.call('sample_pixel', document_id=document, source='layer', layer=0,
                              x=x, y=y)['center_rgba_codes'] for x, y in points]


def policy_of(source):
    return source['raw']['recipe']['outputSpace']


def assert_source(source, space, fallback):
    entry = SPACES[space]
    assert policy_of(source) == entry['policy'], policy_of(source)
    assert source['profile'] == entry['name'], source['profile']
    assert source['depth'] == 16 and source['master_depth'] == 16, source
    assert source['fallback_depth'] == fallback, source['fallback_depth']


def classes_of(wide_lattice, srgb_lattice):
    """The lattice points by class: interior, clipped in sRGB with the wide
    codes unclipped, and the rest."""
    interior, clipped, other = [], [], 0
    for index, (wide, srgb) in enumerate(zip(wide_lattice, srgb_lattice)):
        assert wide[3] == 65535 and srgb[3] == 65535, (index, wide, srgb)
        unclipped = all(1 <= c <= 65534 for c in wide[:3])
        if unclipped and all(256 <= c <= 65279 for c in srgb[:3]):
            interior.append(index)
        elif unclipped and any(c in (0, 65535) for c in srgb[:3]):
            clipped.append(index)
        else:
            other += 1
    return dict(interior=interior, clipped=clipped, other=other)


def classify(run, space):
    """`classes_of` the as-shot developments, computed once per space."""
    if space not in run.interior:
        run.interior[space] = classes_of(run.wide16_lattice[space], run.srgb16_lattice)
    return run.interior[space]


def assert_clamped(space, wide_lattice, srgb_lattice, clipped):
    """The per-channel clamp at the clipped points: where sRGB holds a channel
    at 0 the oracle is at or below zero, and where it holds one at 65535 the
    oracle is at or above one, each to LINEAR_BOUND. Returns how many channels
    of each kind were checked and the worst oracle value of each."""
    low = high = 0
    highest_low = lowest_high = None
    for index in clipped:
        srgb = srgb_lattice[index]
        got = oracle(space, wide_lattice[index])
        for channel in range(3):
            if srgb[channel] == 0:
                assert got[channel] <= LINEAR_BOUND, ('clipped low', index, channel, got, srgb)
                low += 1
                highest_low = got[channel] if highest_low is None else max(highest_low,
                                                                           got[channel])
            elif srgb[channel] == 65535:
                assert got[channel] >= 1 - LINEAR_BOUND, ('clipped high', index, channel, got, srgb)
                high += 1
                lowest_high = got[channel] if lowest_high is None else min(lowest_high,
                                                                           got[channel])
    return dict(channels_at_0=low, channels_at_65535=high, highest_oracle_at_0=highest_low,
                lowest_oracle_at_65535=lowest_high)


# ----------------------------------------------------------------- cases --

def setup(run):
    run.srgb16 = run.open('srgb-16', 16, 'srgb')
    source = run.source(run.srgb16)
    assert policy_of(source) == SRGB_POLICY and source['profile'] == SRGB_NAME, source
    assert run.profile(run.srgb16)['name'] == SRGB_NAME
    run.srgb16_lattice = run.lattice(run.srgb16)
    srgb8 = run.open('srgb-8', 8, 'srgb')
    run.srgb8_profile = run.profile(srgb8)
    run.close(srgb8)
    return dict(size=list(run.size), lattice=len(run.srgb16_lattice),
                srgb8_profile=run.srgb8_profile)


def open16(run, space):
    entry = SPACES[space]
    document = run.open(f'{space}-16', 16, space)
    run.wide16[space] = document
    source = run.source(document)
    assert_source(source, space, 16)
    assert run.app.call('get_precision', document_id=document)['bits_per_channel'] == 16
    assert run.profile(document)['name'] == entry['name'], run.profile(document)
    picture = run.folder / f'{space}-16.png'
    run.app.call('save_copy', document_id=document, path=str(picture))
    name, curve = icc_facts(png_profile(picture))
    assert name == entry['name'], name
    want = expected_curve(space)
    assert len(curve) == len(want), (len(curve), len(want))
    assert curve == want, [(i, a, b) for i, (a, b) in enumerate(zip(curve, want)) if a != b][:8]
    return dict(profile=name, curv_entries=len(curve), curv_head=[hex(v) for v in curve[:3]],
                policy=policy_of(source))


def oracle_case(run, space):
    run.wide16_lattice[space] = run.lattice(run.wide16[space])
    classes = classify(run, space)
    worst = 0.0
    for index in classes['interior']:
        want = srgb_linear(run.srgb16_lattice[index])
        got = oracle(space, run.wide16_lattice[space][index])
        for channel in range(3):
            error = abs(got[channel] - want[channel])
            worst = max(worst, error)
            assert error <= LINEAR_BOUND, (
                'interior', index, channel, run.wide16_lattice[space][index],
                run.srgb16_lattice[index], got, want)
    assert_clamped(space, run.wide16_lattice[space], run.srgb16_lattice, classes['clipped'])
    assert len(classes['interior']) >= MIN_INTERIOR, len(classes['interior'])
    return dict(interior=len(classes['interior']), clipped_in_srgb=len(classes['clipped']),
                other=classes['other'], worst_interior_linear=worst, bound=LINEAR_BOUND)


def clamp(run, space):
    if run.clamp_srgb_lattice is None:
        reference = run.open('clamp-srgb', 16, 'srgb', **CLAMP_DEVELOPMENT)
        source = run.source(reference)
        assert policy_of(source) == SRGB_POLICY, source
        assert abs(source['raw']['recipe']['values']['exposure'] - 2.0) < 1e-6, source['raw']
        run.clamp_srgb_lattice = run.lattice(reference)
        run.close(reference)
    document = run.open(f'clamp-{space}', 16, space, **CLAMP_DEVELOPMENT)
    source = run.source(document)
    assert_source(source, space, 16)
    assert abs(source['raw']['recipe']['values']['exposure'] - 2.0) < 1e-6, source['raw']
    wide = run.lattice(document)
    run.close(document)
    classes = classes_of(wide, run.clamp_srgb_lattice)
    worst = 0.0
    for index in classes['interior']:
        want = srgb_linear(run.clamp_srgb_lattice[index])
        got = oracle(space, wide[index])
        for channel in range(3):
            error = abs(got[channel] - want[channel])
            worst = max(worst, error)
            assert error <= LINEAR_BOUND, ('interior', index, channel, wide[index],
                                           run.clamp_srgb_lattice[index], got, want)
    clamped = assert_clamped(space, wide, run.clamp_srgb_lattice, classes['clipped'])
    assert clamped['channels_at_0'] >= MIN_CLAMPED, clamped
    assert clamped['channels_at_65535'] >= MIN_CLAMPED, clamped
    return dict(development=CLAMP_DEVELOPMENT, interior=len(classes['interior']),
                clipped_in_srgb=len(classes['clipped']), other=classes['other'],
                worst_interior_linear=worst, bound=LINEAR_BOUND, **clamped)


def open8(run, space):
    entry = SPACES[space]
    document = run.open(f'{space}-8', 8, space)
    run.wide8[space] = document
    source = run.source(document)
    assert_source(source, space, 8)
    assert run.app.call('get_precision', document_id=document)['bits_per_channel'] == 8
    profile = run.profile(document)
    assert profile == run.srgb8_profile, (profile, run.srgb8_profile)
    assert profile['name'] != entry['name'], profile
    return dict(document_profile=profile['name'], source_profile=source['profile'])


def redevelop(run, space):
    entry = SPACES[space]
    document = run.open(f'redevelop-{space}', 16, 'srgb')
    before_profile = run.profile(document)
    before = run.source(document)
    assert policy_of(before) == SRGB_POLICY, before
    reply = run.app.call('redevelop_raw', document_id=document, layer=0,
                         raw=dict(output_space=space))
    assert reply['ok'] is True, reply
    after = run.source(document)
    assert_source(after, space, 16)
    assert run.profile(document) == before_profile, 'the document profile is unchanged'
    classes = classify(run, space)
    lattice = run.lattice(document)
    worst = 0.0
    for index in classes['interior']:
        want = srgb_linear(run.srgb16_lattice[index])
        got = srgb_linear(lattice[index])
        for channel in range(3):
            error = abs(got[channel] - want[channel])
            worst = max(worst, error)
            assert error <= LINEAR_BOUND, ('redeveloped', index, channel, lattice[index],
                                           run.srgb16_lattice[index])
    run.app.call('undo', document_id=document)
    restored = run.source(document)
    assert policy_of(restored) == SRGB_POLICY and restored['profile'] == SRGB_NAME, restored
    run.close(document)
    return dict(interior=len(classes['interior']), worst_interior_linear=worst,
                bound=LINEAR_BOUND)


def preset(run, space):
    entry = SPACES[space]
    name = f'v1 output {space}'
    saved = run.app.call('save_raw_preset', name=name, fields=['output_space'],
                         document_id=run.wide16[space], layer=0, replace=True)
    assert saved['ok'] is True and saved['preset']['raw'] == dict(output_space=space), saved
    listed = run.app.call('list_raw_presets')
    rows = [row for row in listed['presets'] if row['name'] == name]
    assert len(rows) == 1 and rows[0]['raw'] == dict(output_space=space), listed
    assert listed['broken'] == [], listed['broken']

    target = run.open(f'preset-{space}', 8, 'srgb')
    assert policy_of(run.source(target)) == SRGB_POLICY
    held = run.profile(target)
    applied = run.app.call('apply_raw_preset', name=name, document_id=target, layer=0)
    assert applied['ok'] is True, applied
    assert_source(run.source(target), space, 8)
    assert run.profile(target) == held, 'the document profile is unchanged'
    run.close(target)

    exposure = f'v1 exposure {space}'
    run.app.call('save_raw_preset', name=exposure, raw=dict(exposure=0.3), replace=True)
    wide = run.wide8[space]
    start = run.source(wide)
    run.app.call('apply_raw_preset', name=exposure, document_id=wide, layer=0)
    moved = run.source(wide)
    assert_source(moved, space, 8)
    assert abs(moved['raw']['recipe']['values']['exposure'] - 0.3) < 1e-6, moved['raw']['recipe']
    assert moved['raw']['recipe']['values'] != start['raw']['recipe']['values']
    return dict(preset=rows[0]['raw'], file=rows[0]['file'])


def rz(run, space):
    document = run.wide16[space]
    before = run.source(document)
    path = run.folder / f'{space}-16.rz'
    run.app.call('save_copy', document_id=document, path=str(path))
    run.close(document)
    del run.wide16[space]
    reopened = run.app.call('open_document', path=str(path))['document']['id']
    assert run.source(reopened) == before, 'get_raw_source differs after the reopen'
    assert run.profile(reopened)['name'] == SPACES[space]['name']
    assert run.lattice(reopened) == run.wide16_lattice[space], 'the lattice codes differ'
    reply = run.app.call('redevelop_raw', document_id=reopened, layer=0, raw=dict(exposure=0.25))
    assert reply['ok'] is True, reply
    after = run.source(reopened)
    assert_source(after, space, 16)
    assert abs(after['raw']['recipe']['values']['exposure'] - 0.25) < 1e-6, after['raw']['recipe']
    run.close(reopened)
    return dict(bytes=path.stat().st_size)


def refusal(run, space):
    document = run.wide8[space]
    before = run.source(document)
    text = run.app.refused('redevelop_raw', REFUSAL, document_id=document, layer=0,
                           raw=dict(output_space='rec2020'))
    assert run.source(document) == before, 'a refusal changed the source'
    run.close(document)
    del run.wide8[space]
    return dict(refusal=text)


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, the copies and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-raw-output-'))
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
            run_case('srgb reference', lambda: setup(run))
            for space in SPACES:
                run_case(f'open 16 {space}', lambda: open16(run, space))
                run_case(f'oracle {space}', lambda: oracle_case(run, space))
                run_case(f'clamp {space}', lambda: clamp(run, space))
                run_case(f'open 8 {space}', lambda: open8(run, space))
                run_case(f'redevelop {space}', lambda: redevelop(run, space))
                run_case(f'preset {space}', lambda: preset(run, space))
                run_case(f'rz {space}', lambda: rz(run, space))
                run_case(f'refusal {space}', lambda: refusal(run, space))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
