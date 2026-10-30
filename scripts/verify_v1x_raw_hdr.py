#!/usr/bin/env python3
"""MCP drive for RAW HDR output (#680): the tone curve, Shadows
and grain on the 32-bit float (linear) RAW development, and the Develop
window's HDR preview at Fit and 100 % with Clipping and its float histogram
as `inspect_raw_development`'s `hdr_preview` and `after_histogram`, on the
real NEF.

The script owns its one app instance through `verify_v1_raw_float`'s `Run`
and `verify_v1_lens.App`: the Debug app (or `DR_VERIFY_APP`) on its own
scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, an
8 GiB working limit and private `DR_*_DIR`s. It terminates only that process.
The fixture is `~/devel/doctored-fixtures/camera/camera.NEF`; an absent
fixture FAILS the run. Every document is opened from its own copy.

Samples are `sample_pixel` (`source: layer`) on `Run`'s 32×20 lattice. The
curve and shadows cases read each point as measurement M2 does
(`scripts/raw-probe/float_tone.py`): the float value is the mean of the
per-pixel sRGB encodings of the 25 clamped samples of the 5×5 square, the
16-bit value the mean of its 25 codes. Developments that are compared open
with `lens_correction: false` (`verify_v1_raw_float`'s docstring: the lens
correction leaves transparent strips that no 5×5 window may hold).

Bounds, fixed before the first run and never widened (M2's, restated here):
- highlights: a float open at tone_curve 1, exposure 2: >= 20 lattice points
  have a channel > 1.0 and the largest channel is >= 1.5; the policy is
  `srgb-rgba32f-linear-v2`.
- curve (T2): tone_curve 0.5 and 1 each against 0 (exposure 0): over the
  channels whose 16-bit twin's mean code moves by more than 0.01 (code/65535)
  at points whose every 16-bit code is in 256…65279 in both twins, the
  float's encoded mean moves with the same sign for >= 90 % of those
  channels, and >= 20 points hold such a channel.
- shadows (T3): tone_curve 1 shadows 1 against tone_curve 1 shadows 0, T2's
  statement.
- linear again: `redevelop_raw tone_curve 0` on a tone-curve float layer
  writes `srgb-rgba32f-linear-v1` and its lattice equals a fresh float open's
  exactly.
- locals: a radial local's ids and mask are unchanged through a tone-curve
  redevelop; the layer stays a float development with the curve.
- rz: `save_copy` `.rz` and reopen: the recipe and the lattice are identical;
  a later exposure redevelop keeps the tone curve.
- action: `start_recording` / `redevelop_raw tone_curve 0.7 shadows 0.5` /
  `stop_recording`, then `run_action` on a fresh float document develops the
  same recipe values and the same lattice as the direct redevelop.
- preset and sync: a preset with tone_curve 0.4 applied to a float layer, and
  `sync_raw_settings` of `tone_curve` from it to another float layer: both
  hold tone curve 0.4 (within 1e-6: the decoder value is binary32) and stay
  float developments.
- place: `place_image` `raw: {tone_curve: 0.5}` into a float document gives a
  float master with the curve.
- hdr preview: `inspect_raw_development` on the float layer at +2 EV answers
  `hdr_preview.after_peak` > 1.0 and `after_divisor` = ceil(max(w, h) / 1600)
  of the after master; a 16-bit document's layer answers no `hdr_preview`.
- hdr detail (#680): on the +2 EV float layer, `inspect_raw_development` with a
  256×256 `region` about the brightest lattice point answers `after_divisor`
  1 and `after_peak` > 1.0 (and `after_marked` 0 without clipping); with
  `clipping: highlights` `after_marked` is in 1…65536 and the peak is
  unchanged; `after_histogram.storage` is `rgba32_float` and its
  `above_white_rgba` equals the MCP `histogram` (source layer, stride 1) of
  that layer.
- grain (#680): `redevelop_raw grain {amount 20}` on that layer keeps a float
  master and >= 20 lattice points with a channel > 1.0; every lattice point
  whose centre sample has all channels >= 1.0 is bit-identical to the no-grain
  lattice; some point with all channels < 1.0 changed; an `open_document`
  with the same grain equals the redeveloped lattice; grain amount 0 restores
  the no-grain lattice exactly.

Not exercised: the Develop window itself (no `scripts/*_ui.py`); its HDR view
is the same `RawDevelopExtendedImage` this reply reads.

Usage: python3 scripts/verify_v1x_raw_hdr.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import json
import math
import os
from pathlib import Path
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from verify_v1_lens import App, SlowClient  # noqa: E402
from verify_v1_raw_float import BUSY, FIXTURE, LIMIT, MIN_CLASS, Run  # noqa: E402

SAME_SIGN = 0.90
MOVE = 0.01
DETAIL_SIDE = 256
GRAIN_AMOUNT = 20
V1_POLICY = 'srgb-rgba32f-linear-v1'
V2_POLICY = 'srgb-rgba32f-linear-v2'


def close_to(value, expected):
    return abs(value - expected) <= 1e-6


def same_sign(float_moved, float_base, sdr_moved, sdr_base, label):
    """T2's statement (module docstring) over two float/16-bit pairs."""
    channels = agree = points = 0
    for index in range(len(sdr_base)):
        after, before = sdr_moved[index]['mean'], sdr_base[index]['mean']
        if not all(256 <= code <= 65279 for code in after + before):
            continue
        held = False
        for channel in range(3):
            delta = (after[channel] - before[channel]) / 65535
            if abs(delta) <= MOVE:
                continue
            held = True
            channels += 1
            moved = (float_moved[index]['encoded'][channel]
                     - float_base[index]['encoded'][channel])
            if moved * delta > 0:
                agree += 1
        if held:
            points += 1
    share = agree / channels if channels else 0.0
    assert channels and share >= SAME_SIGN and points >= MIN_CLASS, (
        label, agree, channels, share, points)
    return dict(channels=channels, agree=agree, share=share, points=points)


class HDRRun(Run):
    def __init__(self, app, folder):
        super().__init__(app, folder)
        self.windows = {}
        self.sdr_windows_by = {}
        self.fresh = None
        self.fresh_lattice = None
        self.bright = None
        self.locals_document = None

    def recipe(self, document, layer=0):
        return self.source(document, layer)['raw']['recipe']

    def inspect(self, document, layer=0, **extra):
        deadline = time.monotonic() + 60
        while True:
            try:
                return self.app.call('inspect_raw_development', document_id=document, layer=layer,
                                     **extra)
            except AssertionError as error:
                if BUSY not in str(error) or time.monotonic() >= deadline:
                    raise
                time.sleep(0.05)


# ----------------------------------------------------------------- cases --

def highlights_case(run):
    run.bright = run.open_float('bright', tone_curve=1, exposure=2, lens_correction=False)
    assert run.is_float_master(run.bright)
    recipe = run.recipe(run.bright)
    assert recipe['outputSpace'] == V2_POLICY, recipe['outputSpace']
    assert recipe['values']['toneCurve'] == 1, recipe['values']
    lattice = run.float_lattice(run.bright)
    largest = max(max(point[:3]) for point in lattice)
    above = sum(1 for point in lattice if max(point[:3]) > 1.0)
    assert largest >= 1.5 and above >= MIN_CLASS, (largest, above)
    return dict(largest=largest, points_above_one=above, policy=recipe['outputSpace'])


def curve_case(run):
    for curve, shadows in ((0, 0), (0.5, 0), (1, 0), (1, 1)):
        document = run.open_float(f'float-{curve}-{shadows}', tone_curve=curve, shadows=shadows,
                                  lens_correction=False)
        twin = run.open(f'sdr-{curve}-{shadows}', tone_curve=curve, shadows=shadows,
                        lens_correction=False)
        assert twin['precision'] == 'rgba16', twin
        assert run.is_float_master(document)
        recipe = run.recipe(document)
        assert recipe['outputSpace'] == (V1_POLICY if curve == 0 else V2_POLICY), recipe
        run.windows[(curve, shadows)] = run.float_windows(document)
        run.sdr_windows_by[(curve, shadows)] = run.sdr_windows(twin['id'])
        if curve == 0:
            run.fresh = document
            run.fresh_lattice = run.float_lattice(document)
        else:
            run.close(document)
        run.close(twin['id'])
    detail = {}
    for curve in (0.5, 1):
        detail[f'tone_curve_{curve}'] = same_sign(
            run.windows[(curve, 0)], run.windows[(0, 0)],
            run.sdr_windows_by[(curve, 0)], run.sdr_windows_by[(0, 0)], f'T2 {curve}')
    return detail


def shadows_case(run):
    return same_sign(run.windows[(1, 1)], run.windows[(1, 0)],
                     run.sdr_windows_by[(1, 1)], run.sdr_windows_by[(1, 0)], 'T3')


def linear_again_case(run):
    document = run.open_float('again', tone_curve=0.5, lens_correction=False)
    assert run.recipe(document)['outputSpace'] == V2_POLICY
    reply = run.app.call('redevelop_raw', document_id=document, layer=0, raw=dict(tone_curve=0))
    assert reply['ok'] is True, reply
    recipe = run.recipe(document)
    assert recipe['outputSpace'] == V1_POLICY, recipe['outputSpace']
    assert recipe['values']['toneCurve'] == 0, recipe['values']
    assert run.is_float_master(document)
    assert run.float_lattice(document) == run.fresh_lattice, 'tone curve 0 is not the linear open'
    run.close(document)
    return dict(policy=recipe['outputSpace'])


def locals_case(run):
    document = run.open_float('locals', tone_curve=0.3)
    width, height = run.size
    radial = dict(op='radial', center=[width / 2, height / 2],
                  radius=[width * 0.3, height * 0.3], hardness=0.8)
    reply = run.app.call('edit_raw_development', document_id=document, layer=0, operations=[
        dict(op='create', id='radial1', exposure_offset=-1, mask=radial)])
    assert reply['unchanged'] is False, reply
    before = run.locals(document)['nodes']
    reply = run.app.call('redevelop_raw', document_id=document, layer=0, raw=dict(tone_curve=0.6))
    assert reply['ok'] is True and run.is_float_master(document), reply
    recipe = run.recipe(document)
    assert close_to(recipe['values']['toneCurve'], 0.6), recipe['values']
    assert recipe['outputSpace'] == V2_POLICY, recipe['outputSpace']
    after = run.locals(document)['nodes']
    assert [node['id'] for node in after] == ['radial1'], after
    assert after == before, 'the local changed through a tone-curve redevelop'
    run.locals_document = document
    return dict(nodes=len(after))


def rz_case(run):
    document = run.locals_document
    before = run.float_lattice(document)
    source = run.source(document)
    path = run.folder / 'hdr.rz'
    run.app.call('save_copy', document_id=document, path=str(path))
    run.close(document)
    reopened = run.app.call('open_document', path=str(path))['document']
    assert reopened['precision'] == 'rgba32_float', reopened
    reopened = reopened['id']
    assert run.is_float_master(reopened)
    assert run.source(reopened)['raw'] == source['raw'], 'the recipe or the camera file changed'
    assert run.float_lattice(reopened) == before, 'the reopened lattice differs'
    reply = run.app.call('redevelop_raw', document_id=reopened, layer=0, raw=dict(exposure=0.5))
    assert reply['ok'] is True and run.is_float_master(reopened), reply
    recipe = run.recipe(reopened)
    assert recipe['values']['exposure'] == 0.5, recipe['values']
    assert close_to(recipe['values']['toneCurve'], 0.6), recipe['values']
    assert recipe['outputSpace'] == V2_POLICY, recipe['outputSpace']
    run.close(reopened)
    return dict(bytes=path.stat().st_size)


def action_case(run):
    name = f'v1x-raw-hdr-{os.getpid()}'
    direct = run.open_float('action-direct', lens_correction=False)
    run.app.call('start_recording', discard=True)
    reply = run.app.call('redevelop_raw', document_id=direct, layer=0,
                         raw=dict(tone_curve=0.7, shadows=0.5))
    assert reply['ok'] is True, reply
    stopped = run.app.call('stop_recording', name=name)
    replayed = run.open_float('action-replay', lens_correction=False)
    run.app.call('run_action', document_id=replayed, name=name)
    assert run.is_float_master(replayed)
    a, b = run.recipe(direct), run.recipe(replayed)
    assert close_to(b['values']['toneCurve'], 0.7) and b['values']['shadows'] == 0.5, b['values']
    assert a['values'] == b['values'] and a['outputSpace'] == b['outputSpace'] == V2_POLICY, (
        a['values'], b['values'])
    assert run.float_lattice(replayed) == run.float_lattice(direct), 'the replay differs'
    run.close(direct)
    run.close(replayed)
    return dict(steps=stopped.get('steps'))


def preset_sync_case(run):
    name = f'v1x-raw-hdr-{os.getpid()}'
    run.app.call('save_raw_preset', name=name, raw=dict(tone_curve=0.4), replace=True)
    target = run.open_float('preset')
    run.app.call('apply_raw_preset', name=name, document_id=target, layer=0)
    run.app.call('delete_raw_preset', name=name)
    assert run.is_float_master(target)
    assert close_to(run.recipe(target)['values']['toneCurve'], 0.4), run.recipe(target)['values']
    assert run.recipe(target)['outputSpace'] == V2_POLICY
    synced = run.open_float('sync')
    run.app.call('sync_raw_settings', document_id=target, layer=0, fields=['tone_curve'],
                 targets=[dict(document_id=synced, layer=0)])
    assert run.is_float_master(synced)
    assert close_to(run.recipe(synced)['values']['toneCurve'], 0.4), run.recipe(synced)['values']
    assert run.recipe(synced)['outputSpace'] == V2_POLICY
    run.close(target)
    run.close(synced)
    return dict(preset=name)


def place_case(run):
    reply = run.app.call('place_image', document_id=run.fresh, path=str(run.copy('place-tone')),
                         name='placed-tone', raw=dict(tone_curve=0.5))
    layer = reply['layer']
    assert reply['document']['precision'] == 'rgba32_float', reply
    assert run.is_float_master(run.fresh, layer), run.source(run.fresh, layer)
    recipe = run.recipe(run.fresh, layer)
    assert recipe['values']['toneCurve'] == 0.5 and recipe['outputSpace'] == V2_POLICY, recipe
    return dict(layer=layer, master_depth=run.source(run.fresh, layer)['master_depth'])


def hdr_preview_case(run):
    report = run.inspect(run.bright)
    preview = report.get('hdr_preview')
    assert isinstance(preview, dict), report.keys()
    width, height = run.size
    divisor = math.ceil(max(width, height) / 1600)
    assert preview['after_peak'] is not None and preview['after_peak'] > 1.0, preview
    assert preview['after_divisor'] == divisor, (preview, divisor)
    assert preview['before_divisor'] == divisor, (preview, divisor)
    sixteen = run.open('sdr-preview', tone_curve=0)
    assert sixteen['precision'] == 'rgba16', sixteen
    plain = run.inspect(sixteen['id'])
    assert 'hdr_preview' not in plain, plain.get('hdr_preview')
    run.close(sixteen['id'])
    return dict(hdr_preview=preview)


def hdr_detail_case(run):
    lattice = run.float_lattice(run.bright)
    brightest = max(range(len(lattice)), key=lambda index: max(lattice[index][:3]))
    width, height = run.size
    x, y = run.points[brightest]
    side = DETAIL_SIDE
    region = dict(x=min(max(0, x - side // 2), width - side),
                  y=min(max(0, y - side // 2), height - side), width=side, height=side)
    report = run.inspect(run.bright, region=region)
    preview = report.get('hdr_preview')
    assert isinstance(preview, dict), report.keys()
    assert preview['after_divisor'] == 1 and preview['before_divisor'] == 1, preview
    assert preview['after_peak'] is not None and preview['after_peak'] > 1.0, preview
    assert preview['after_marked'] == 0, preview
    clipped = run.inspect(run.bright, region=region, clipping='highlights')['hdr_preview']
    assert 1 <= clipped['after_marked'] <= side * side, clipped
    assert clipped['after_peak'] == preview['after_peak'], (clipped, preview)
    histogram = report['after_histogram']
    assert histogram['storage'] == 'rgba32_float', histogram.get('storage')
    layer = run.app.call('histogram', document_id=run.bright, source='layer', layer=0, stride=1)
    assert layer['storage'] == 'rgba32_float', layer.get('storage')
    assert histogram['above_white_rgba'] == layer['above_white_rgba'], (
        histogram['above_white_rgba'], layer['above_white_rgba'])
    assert sum(histogram['above_white_rgba'][:3]) > 0, histogram['above_white_rgba']
    return dict(region=region, peak=preview['after_peak'], marked=clipped['after_marked'],
                above_white=histogram['above_white_rgba'])


def grain_case(run):
    base = run.float_lattice(run.bright)
    reply = run.app.call('redevelop_raw', document_id=run.bright, layer=0,
                         raw=dict(grain=dict(amount=GRAIN_AMOUNT)))
    assert reply['ok'] is True and run.is_float_master(run.bright), reply
    grained = run.float_lattice(run.bright)
    above = sum(1 for point in grained if max(point[:3]) > 1.0)
    assert above >= MIN_CLASS, above
    white = [index for index, point in enumerate(base) if min(point[:3]) >= 1.0]
    for index in white:
        assert grained[index] == base[index], (index, grained[index], base[index])
    darker = [index for index, point in enumerate(base) if max(point[:3]) < 1.0]
    changed = sum(1 for index in darker if grained[index] != base[index])
    assert changed >= 1, (len(darker), changed)
    opened = run.open_float('bright-grain', tone_curve=1, exposure=2, lens_correction=False,
                            grain=dict(amount=GRAIN_AMOUNT))
    assert run.float_lattice(opened) == grained, 'the open with grain differs from the redevelop'
    run.close(opened)
    reply = run.app.call('redevelop_raw', document_id=run.bright, layer=0,
                         raw=dict(grain=dict(amount=0)))
    assert reply['ok'] is True and run.is_float_master(run.bright), reply
    assert run.float_lattice(run.bright) == base, 'grain 0 did not restore the lattice'
    return dict(points_above_one=above, white_points=len(white), darker_changed=changed,
                darker=len(darker))


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, the copies and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1x-raw-hdr-'))
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
            run = HDRRun(app, output)
            run_case('highlights', lambda: highlights_case(run))
            run_case('curve', lambda: curve_case(run))
            run_case('shadows', lambda: shadows_case(run))
            run_case('linear again', lambda: linear_again_case(run))
            run_case('locals', lambda: locals_case(run))
            run_case('rz', lambda: rz_case(run))
            run_case('action', lambda: action_case(run))
            run_case('preset and sync', lambda: preset_sync_case(run))
            run_case('place', lambda: place_case(run))
            run_case('hdr preview', lambda: hdr_preview_case(run))
            run_case('hdr detail', lambda: hdr_detail_case(run))
            run_case('grain', lambda: grain_case(run))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
