#!/usr/bin/env python3
"""The 10D HSL slice's own MCP verifier: Hue, Saturation, Color and
Luminosity on a built Release app.

Five gates, all fixed before the run and never widened afterwards; a failed
gate IS the receipt.

- **H1 appearance.** The rendered 16-bit copy is within 0.65 codes, every RGB
  component and alpha, of an UNROUNDED independent f64 oracle written from the
  W3C nonseparable equations and `designs/phase-10d-hsl-modes-status.md`
  (`Lum = 0.3R + 0.59G + 0.11B`, ClipColor toward luminance). The fixture is
  inside the ADR's ordinary-bound region for Hue and Saturation, with every
  `Δ/s` at or above the 13900 the core qualification observed.
- **H2 persistence.** The native file is version 33 with one `LBLD` record on
  layer 1, reopens at `precision: preserve` with the same mode, and Undo, Redo
  and an Actions replay produce byte-identical files; Normal restores the
  baseline bytes.
- **H3 exchange.** A 16-bit PSD export reports NO loss, carries `hue `/`sat `/
  `colr`/`lum ` in the record an independent decoder reads, and reopens
  layered with the same mode.
- **H4 refusal.** `Pass Through` on a raster refuses atomically: bytes,
  history, selection and dirty state are unchanged.
- **H5 isolated group.** The foreground grouped alone, the GROUP set to each
  mode at 0.625, renders within the same 0.65 codes, stores one `LBLD` record
  on the group at version 33, and Undo restores the Pass Through group's exact
  bytes; undoing the grouping restores the baseline bytes.

Nothing here derives an expectation from Doctored: the fixture is written by
this file, the appearance oracle is f64 Python, the native file is read by
`scripts/live-filter-probe/native.py` and the PSD by
`scripts/psd_qualification_decode.py`.

The foreground is PLACED with `place_image`, which lands a PNG as an ordinary
raster layer with the authored pixels and keeps nothing of the file, so H3
sees `losses == []`.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import psd_qualification_decode as psd

_spec = importlib.util.spec_from_file_location('hsl_modes_fixture_helpers',
                                               ROOT / 'scripts/verify_phase10d_separable.py')
_prior = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_prior)
support, digest, samples, png, history = (_prior.support, _prior.digest, _prior.samples,
                                          _prior.png, _prior.history)

# `precision_layer_blend::COLOR_MODES` ids, and the PSD key `PSD_BLEND_KEYS`
# pairs with each.
MODES = {'Hue': (23, 'hue '), 'Saturation': (24, 'sat '), 'Color': (25, 'colr'),
         'Luminosity': (26, 'lum ')}
OPACITIES = (1, 0.625)
GROUP_OPACITY = 0.625
NATIVE_VERSION = 33
FULL = 65535
# ADR: Hue/Saturation are inside the ordinary bound iff Δ == 0, s == 0 or
# Δ >= 6830·s. At 6830 the `0.5 + 1024·s/Δ` budget is the whole 0.65, leaving
# nothing for the unquantized f32 group intermediate H5 feeds in, so the
# fixture floor is the 13900 the core qualification observed (ADR line 38).
ELIGIBLE_RATIO = 6830
QUALIFIED_RATIO = 13900
BOUND = 0.65

# 4 x 3. Two gray backdrops, one gray foreground, codes 0 and 65535 in both,
# partial foreground alpha, and every non-gray pair's spreads within the
# qualified ratio in BOTH directions. All asserted by `fixture_contract()`.
BACKDROP = [(0, 65535, 32767, FULL), (65535, 0, 32768, FULL),
            (16000, 16400, 16800, FULL), (32768, 32768, 32768, FULL),
            (8192, 49152, 23567, FULL), (50000, 23000, 4000, FULL),
            (30000, 50000, 7000, FULL), (65001, 55003, 45005, FULL),
            (18000, 37000, 59000, FULL), (10001, 20003, 30007, FULL),
            (65535, 65535, 65535, FULL), (17, 32767, 65501, FULL)]
FOREGROUND = [(65535, 300, 32000, FULL), (60000, 700, 32000, 32768),
              (20000, 21000, 22500, FULL), (16384, 16383, 50000, 45000),
              (40000, 40000, 40000, 32769), (12000, 43000, 61000, 1),
              (62000, 36000, 11000, 32771), (15001, 25003, 35005, FULL),
              (65000, 31000, 1000, 7), (5000, 30000, 60000, 12345),
              (1003, 2005, 3007, FULL), (0, 16384, 32768, 60000)]


# ------------------------------------------------------------- the oracle --

def lum(c):
    return 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]


def clip_color(c, fired=None):
    """W3C ClipColor: rescale toward luminance until the triple fits."""
    l = lum(c)
    n, x = min(c), max(c)
    out = tuple(c)
    if n < 0:
        if fired is not None:
            fired.add('low')
        out = tuple(l + (v - l) * l / (l - n) for v in out)
    if x > 1:
        if fired is not None:
            fired.add('high')
        out = tuple(l + (v - l) * (1 - l) / (x - l) for v in out)
    return out


def set_lum(c, l, fired=None):
    d = l - lum(c)
    return clip_color(tuple(v + d for v in c), fired)


def sat(c):
    return max(c) - min(c)


def set_sat(c, s):
    """W3C SetSat: max -> s, min -> 0, mid rescaled; a gray triple -> 0."""
    order = sorted(range(3), key=lambda index: c[index])
    lo, mid, hi = order
    out = [0.0, 0.0, 0.0]
    if c[hi] > c[lo]:
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo])
        out[hi] = s
    return tuple(out)


def blend_triple(mode, back, front, fired=None):
    """`B(Cb, Cs)`, HSL ADR Decisions, on normalized triples."""
    if mode == 'Hue':
        return set_lum(set_sat(front, sat(back)), lum(back), fired)
    if mode == 'Saturation':
        return set_lum(set_sat(back, sat(front)), lum(back), fired)
    if mode == 'Color':
        return set_lum(front, lum(back), fired)
    if mode == 'Luminosity':
        return set_lum(back, lum(front), fired)
    raise AssertionError(mode)


def _spread(triple):
    return max(triple[:3]) - min(triple[:3])


def _delta_s(mode, back, front):
    """(Δ in codes of the rescaled triple, s of the other pixel normalized)."""
    if mode == 'Hue':
        return _spread(front), _spread(back) / FULL
    if mode == 'Saturation':
        return _spread(back), _spread(front) / FULL
    raise AssertionError(mode)


def eligible(mode, back, front):
    if mode not in ('Hue', 'Saturation'):
        return True
    delta, s = _delta_s(mode, back, front)
    return delta == 0 or s == 0 or delta >= ELIGIBLE_RATIO * s


def ratio(mode, back, front):
    if mode not in ('Hue', 'Saturation'):
        return None
    delta, s = _delta_s(mode, back, front)
    if delta > 0 and s > 0:
        return delta / s
    return None


def fixture_contract():
    """The fixture's claims, asserted before anything is launched."""
    ratios, gray_pairs, fired = [], [], set()
    for index, (back, front) in enumerate(zip(BACKDROP, FOREGROUND, strict=True)):
        for mode in ('Hue', 'Saturation'):
            assert eligible(mode, back, front), (mode, back, front)
            value = ratio(mode, back, front)
            if value is not None:
                assert value >= QUALIFIED_RATIO, (mode, back, front, value)
                ratios.append(value)
        if _spread(back) == 0 or _spread(front) == 0:
            gray_pairs.append(index)
        b = tuple(code / FULL for code in back[:3])
        s = tuple(code / FULL for code in front[:3])
        for mode in MODES:
            blend_triple(mode, b, s, fired)
    assert any(_spread(back) == 0 for back in BACKDROP), 'a gray backdrop'
    assert any(_spread(front) == 0 for front in FOREGROUND), 'a gray foreground'
    for pixels in (BACKDROP, FOREGROUND):
        codes = {code for pixel in pixels for code in pixel[:3]}
        assert 0 in codes and FULL in codes, 'both endpoints in both fixtures'
    assert any(0 < front[3] < FULL for front in FOREGROUND), 'partial foreground alpha'
    assert all(front[3] > 0 for front in FOREGROUND), 'every source pixel contributes'
    assert all(back[3] == FULL for back in BACKDROP), 'the backdrop is opaque'
    assert fired == {'low', 'high'}, fired
    assert ratios and min(ratios) >= QUALIFIED_RATIO
    return dict(minimum_eligible_ratio=min(ratios), gray_pairs=gray_pairs,
                clip_branches=sorted(fired))


def expected(mode, opacity):
    """The whole 4 x 3 render, unrounded, by the general W3C composite."""
    output = []
    for back, front in zip(BACKDROP, FOREGROUND, strict=True):
        sa = front[3] / FULL * opacity
        ab = back[3] / FULL
        ao = sa + ab * (1 - sa)
        b = tuple(code / FULL for code in back[:3])
        s = tuple(code / FULL for code in front[:3])
        blended = blend_triple(mode, b, s)
        for cb, cs, value in zip(b, s, blended, strict=True):
            output.append(FULL * (sa * (1 - ab) * cs + sa * ab * value
                                  + (1 - sa) * ab * cb) / ao)
        output.append(FULL * ao)
    return tuple(output)


# ------------------------------------------------------------- the gates --

def render_error(call, path, mode, opacity):
    call('save_copy', path=str(path))
    value = support.png_samples(path.read_bytes())
    assert (value['width'], value['height'], value['depth']) == (4, 3, 16), value
    reference = expected(mode, opacity)
    errors = [abs(a - b) for a, b in zip(value['samples'], reference, strict=True)]
    assert max(errors) <= BOUND, (mode, opacity, max(errors), value['samples'], reference)
    return max(errors)


def blend_matches(row, mode):
    """A typed document reports a layer's mode as its numeric id; a byte one
    would report the name, so both are accepted."""
    return row['blend_mode'] in (mode, MODES[mode][0])


def _rasters(document, names, opacity_of_1, blend_of_1):
    for index, pixels in enumerate((BACKDROP, FOREGROUND)):
        row = document['layers'][index]
        decoded = support.png_samples(document['fallbacks'][index])
        assert (decoded['width'], decoded['height'], decoded['depth']) == (4, 3, 16)
        assert tuple(decoded['samples']) == samples(pixels), index
        assert row['name'] == names[index]
        assert row['blend'] == (0 if index == 0 else blend_of_1)
        assert row['opacity'] == (1 if index == 0 else opacity_of_1)
        assert row['visible'] and row['mask'] is None and (row['x'], row['y']) == (0, 0)


def native_state(call, path, mode, opacity, names, baseline_version):
    """The stored ungrouped file, read independently, with the slice's claims."""
    call('save_copy', path=str(path))
    parsed = support.read_native(path)
    document = parsed['document']
    identifier = MODES[mode][0] if mode else 0
    assert parsed['version'] == (NATIVE_VERSION if mode else baseline_version), parsed['version']
    assert document['blend_version'] == (NATIVE_VERSION if mode else None)
    assert document.get('layer_blend_records', []) == ([(1, identifier)] if mode else [])
    assert (document['width'], document['height'], document['depth']) == (4, 3, 16)
    assert len(document['layers']) == len(document['fallbacks']) == 2
    _rasters(document, names, opacity, identifier)
    # A placed PNG is an ordinary raster layer, so nothing is retained.
    assert not any(parsed[key] for key in ('assets', 'bindings', 'resources'))
    return digest(path)


def group_state(call, path, mode, group, names, grouped_version):
    """The stored three-layer file of H5: backdrop, foreground, its group."""
    call('save_copy', path=str(path))
    parsed = support.read_native(path)
    document = parsed['document']
    assert parsed['version'] == (NATIVE_VERSION if mode else grouped_version), parsed['version']
    assert document['blend_version'] == (NATIVE_VERSION if mode else None)
    assert document.get('layer_blend_records', []) == (
        [(group, MODES[mode][0])] if mode else [])
    assert (document['width'], document['height'], document['depth']) == (4, 3, 16)
    assert len(document['layers']) == 3 and len(document['fallbacks']) == 2
    assert sorted(document['fallbacks']) == [0, 1]
    _rasters(document, names, 1, 0)
    row = document['layers'][group]
    if mode:
        assert row['kind'] == 1 and row['blend'] == MODES[mode][0], row
        assert row['opacity'] == GROUP_OPACITY, row
    else:
        assert row['kind'] == 2 and row['blend'] == 27, row
        assert row['opacity'] == 1, row
    assert not any(parsed[key] for key in ('assets', 'bindings', 'resources'))
    return digest(path)


def gate_h1(call, folder, mode, opacity):
    return render_error(call, folder / 'render.png', mode, opacity)


def gate_h2(app, call, folder, mode, opacity, names, baseline_sha, baseline_version):
    before = history(call)
    action = f'HSL {mode} {opacity}'
    app.call('start_recording')
    call('set_layer_properties', index=1, blend_mode=mode, opacity=opacity)
    recorded = app.call('stop_recording', name=action)
    assert [step['tool'] for step in recorded['steps']] == ['set_layer_properties'], recorded
    assert recorded['steps'][0]['arguments']['blend_mode'] == mode
    assert history(call)[1] == before[1] + 1
    sha = native_state(call, folder / 'mode.rz', mode, opacity, names, baseline_version)

    identifier = app.call('open_document', path=str(folder / 'mode.rz'),
                          precision='preserve')['document']['id']
    reopened = support.call_for(app, identifier, folder / 'reopen-calls.jsonl')
    rows = reopened('get_document')['layers']
    assert blend_matches(rows[1], mode), rows[1]
    assert native_state(reopened, folder / 'reopened.rz', mode, opacity, names,
                        baseline_version) == sha

    call('undo')
    assert history(call)[0] == before[0]
    assert native_state(call, folder / 'undone.rz', None, 1, names,
                        baseline_version) == baseline_sha
    call('redo')
    assert native_state(call, folder / 'redone.rz', mode, opacity, names,
                        baseline_version) == sha
    call('undo')
    assert call('run_action', name=action)['ok']
    assert history(call)[1] == before[1] + 1
    assert native_state(call, folder / 'replayed.rz', mode, opacity, names,
                        baseline_version) == sha

    # Normal restores the prior writer's exact bytes.
    call('set_layer_properties', index=1, blend_mode='Normal', opacity=1)
    assert native_state(call, folder / 'normal.rz', None, 1, names,
                        baseline_version) == baseline_sha
    call('undo')
    assert native_state(call, folder / 'restored.rz', mode, opacity, names,
                        baseline_version) == sha
    return sha


def gate_h3(app, call, folder, mode, opacity, names):
    key = MODES[mode][1]
    written = folder / 'modes16.psd'
    exported = call('export_psd', path=str(written), mode='layers')
    assert exported['bit_depth'] == 16, exported
    assert exported['losses'] == [], exported['losses']
    decoded = psd.decode(written)
    rows = {row['name']: row for row in decoded['layers']}
    assert rows[names[1]]['blend'] == key, (mode, rows[names[1]]['blend'])
    assert rows[names[0]]['blend'] == 'norm', rows[names[0]]['blend']
    report = app.call('inspect_psd', path=str(written))
    assert report['layered_available'] and report['import_precision'] == 'rgba16', report
    opened = app.call('open_document', path=str(written), psd_mode='layers',
                      psd_fingerprint=report['source_fingerprint'])
    assert opened['psd_import']['chosen_mode'] == 'layers', opened
    back = support.call_for(app, opened['document']['id'], folder / 'psd-calls.jsonl')
    reopened = back('get_document')['layers']
    assert blend_matches(reopened[1], mode), reopened[1]
    return dict(key=key, losses=exported['losses'],
                observations=exported['observations'],
                reopened_blend_mode=reopened[1]['blend_mode'])


def gate_h4(call, folder, baseline_version):
    """Pass Through is a GROUP mode; on a raster it must change nothing."""
    before = history(call)
    selection = call('get_selection')
    dirty = call('get_document')['unsaved_changes']
    sha = native_state(call, folder / 'before-refusal.rz', None, 1,
                       [row['name'] for row in call('get_document')['layers']],
                       baseline_version)
    error = call('set_layer_properties', index=1, fail=True, blend_mode='Pass Through')
    assert 'group' in error.lower(), error
    assert history(call) == before and call('get_selection') == selection
    assert call('get_document')['unsaved_changes'] == dirty
    assert digest_of(call, folder / 'after-refusal.rz') == sha
    return dict(refusal=error, bytes_history_selection_dirty_unchanged=True)


def gate_h5(call, output, names, baseline_sha, baseline_version):
    """The foreground ALONE in an isolated group; the group carries the mode.
    Never `[0, 1]`: grouping the backdrop would blend against transparency."""
    grouped = call('group_layers', layers=[1], name='HSL group')
    group = grouped['group']
    probe = output / 'grouped-probe.rz'
    call('save_copy', path=str(probe))
    grouped_version = support.read_native(probe)['version']
    grouped_sha = group_state(call, output / 'grouped.rz', None, group, names, grouped_version)
    rows = []
    for mode in MODES:
        folder = output / f"group-{mode.lower()}"
        folder.mkdir()
        call('set_layer_properties', index=group, blend_mode=mode, opacity=GROUP_OPACITY)
        error = render_error(call, folder / 'render.png', mode, GROUP_OPACITY)
        sha = group_state(call, folder / 'mode.rz', mode, group, names, grouped_version)
        call('undo')
        assert group_state(call, folder / 'closed.rz', None, group, names,
                           grouped_version) == grouped_sha
        rows.append(dict(mode=mode, opacity=GROUP_OPACITY, group=group,
                         grouped_native_version=grouped_version,
                         grouped_sha256=grouped_sha, maximum_u16_error=error,
                         native_sha256=sha))
    call('undo')
    assert native_state(call, output / 'ungrouped.rz', None, 1, names,
                        baseline_version) == baseline_sha
    return rows


def digest_of(call, path):
    call('save_copy', path=str(path))
    return digest(path)


# ------------------------------------------------------------- the runner --

def take(app, output, report):
    back, front = output / 'backdrop16.png', output / 'foreground16.png'
    png(back, BACKDROP)
    png(front, FOREGROUND)
    report['fixture_sha256'] = {p.name: digest(p) for p in (back, front)}
    identifier = app.call('open_document', path=str(back),
                          precision='preserve')['document']['id']
    call = support.call_for(app, identifier, output / 'calls.jsonl')
    assert call('get_precision')['mode'] == 'rgba16'
    placed = call('place_image', path=str(front), storage='embedded')
    # A placed PNG is an ordinary raster layer: no source is kept.
    assert placed['layer'] == 1 and placed['source'] is None, placed
    call('set_active_layer', index=1)
    names = [row['name'] for row in call('get_document')['layers']]
    report['layer_names'] = names
    probe_path = output / 'baseline-probe.rz'
    call('save_copy', path=str(probe_path))
    baseline_version = support.read_native(probe_path)['version']
    report['baseline_native_version'] = baseline_version
    baseline = native_state(call, output / 'baseline.rz', None, 1, names, baseline_version)
    report['baseline_native_sha256'] = baseline
    report['gates']['H4 Pass Through refuses atomically'] = dict(
        passed=True, detail=gate_h4(call, output, baseline_version))
    cases = []
    for mode in MODES:
        for opacity in OPACITIES:
            folder = output / f"{mode.lower()}-{opacity}"
            folder.mkdir()
            sha = gate_h2(app, call, folder, mode, opacity, names, baseline,
                          baseline_version)
            error = gate_h1(call, folder, mode, opacity)
            exchange = gate_h3(app, call, folder, mode, opacity, names)
            call('undo')
            assert native_state(call, folder / 'closed.rz', None, 1, names,
                                baseline_version) == baseline
            cases.append(dict(mode=mode, opacity=opacity, maximum_u16_error=error,
                              native_sha256=sha, exchange=exchange))
    report['cases'] = cases
    report['gates']['H1 appearance within 0.65 codes of the unrounded f64 oracle'] = dict(
        passed=all(row['maximum_u16_error'] <= BOUND for row in cases),
        detail={f"{row['mode']} {row['opacity']}": row['maximum_u16_error'] for row in cases})
    report['gates']['H2 native 33 roundtrip, undo/redo/replay identical'] = dict(
        passed=True, detail={f"{row['mode']} {row['opacity']}": row['native_sha256']
                             for row in cases})
    report['gates']['H3 16-bit PSD carries the key with no loss'] = dict(
        passed=all(row['exchange']['losses'] == [] for row in cases),
        detail={f"{row['mode']} {row['opacity']}": row['exchange']['key'] for row in cases})
    group_cases = gate_h5(call, output, names, baseline, baseline_version)
    report['group_cases'] = group_cases
    report['gates']['H5 isolated group within 0.65 codes, v33 record, undo exact'] = dict(
        passed=all(row['maximum_u16_error'] <= BOUND for row in group_cases),
        detail={f"{row['mode']} {row['opacity']}": row['maximum_u16_error']
                for row in group_cases})
    assert all(digest(p) == report['fixture_sha256'][p.name] for p in (back, front))
    report['fixtures_unchanged'] = True


def dependencies():
    """The code files `helper_sha256` hashes."""
    helpers = [Path(_prior.__file__), Path(psd.__file__),
               ROOT / 'scripts/retouch-probe/support.py',
               ROOT / 'scripts/host-source-probe/support.py',
               ROOT / 'scripts/live-filter-probe/native.py']
    helpers += [ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/precision-probe/fixtures.py',
                ROOT / 'scripts/precision-probe/retained_fixtures.py',
                ROOT / 'scripts/precision-probe/retained_linked_fixtures.py',
                ROOT / 'scripts/raw-probe/native.py', ROOT / 'scripts/retouch-probe/fixtures.py',
                ROOT / 'scripts/verification_isolation.py',
                ROOT / 'scripts/app_under_test.py', ROOT / 'scripts/verify_phase1.py',
                ROOT / 'scripts/verify_phase2.py', ROOT / 'scripts/verify_phase8b.py',
                *sorted((ROOT / 'core/src').rglob('*.rs'))]
    return helpers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    output = arguments.output.resolve()
    if output.is_relative_to(ROOT) or arguments.output.is_symlink():
        parser.error('output must be outside the repository and not a symlink')
    assert not output.exists() or not any(output.iterdir()), 'use a fresh output folder'
    output.mkdir(parents=True, exist_ok=True)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    helpers = dependencies()
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)),
                  helper_sha256={str(p): digest(p) for p in helpers},
                  fixture_sha256={},
                  fixture_contract=fixture_contract(),
                  gates={}, cases=[], group_cases=[], error=None)
    try:
        with support.HostApp(output / 'app', output / 'recovery', executable) as app:
            take(app, output, report)
    except BaseException as error:          # Recorded, never repaired.
        report['error'] = f'{type(error).__name__}: {error}'
    (output / 'result.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    gates = {name: gate['passed'] for name, gate in report['gates'].items()}
    print(json.dumps(dict(error=report['error'], gates=gates,
                          fixture_contract=report['fixture_contract'],
                          cases=[dict(mode=row['mode'], opacity=row['opacity'],
                                      maximum_u16_error=row['maximum_u16_error'])
                                 for row in report['cases']],
                          group_cases=[dict(mode=row['mode'],
                                            maximum_u16_error=row['maximum_u16_error'])
                                       for row in report['group_cases']],
                          result=str(output / 'result.json')), indent=2))
    raise SystemExit(0 if not report['error'] and gates and all(gates.values()) else 1)


if __name__ == '__main__':
    main()
