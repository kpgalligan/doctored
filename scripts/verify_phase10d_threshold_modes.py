#!/usr/bin/env python3
"""The 10D threshold slice's own MCP verifier: Hard Mix, Darker/Lighter Color
and Dissolve on a built Release app.

Four gates, all fixed before the run and never widened afterwards; a failed
gate IS the receipt.

- **T1 appearance.** The rendered 16-bit copy is within 1 code of an
  independent f64 oracle written from `designs/phase-10d-threshold-modes-status.md`
  (Hard Mix per channel at `cb + cs >= 1`, Darker/Lighter Color by the whole
  triple's `0.3R + 0.59G + 0.11B` with the backdrop on a tie), and Dissolve is
  EXACT against the documented 24-bit hash.
- **T2 persistence.** The native file is version 34 with one `LBLD` record on
  layer 1, reopens at `precision: preserve` with the same mode, and Undo, Redo
  and an Actions replay produce byte-identical files; Normal restores the
  baseline bytes.
- **T3 exchange.** A 16-bit PSD export reports NO loss, carries `hMix`/`dkCl`/
  `lgCl`/`diss` in the record an independent decoder reads, and reopens layered
  with the same mode.
- **T4 refusal.** `Pass Through` on a raster refuses atomically: bytes, history
  and selection are unchanged.

Nothing here derives an expectation from Doctored: the fixture is written by
this file, the appearance oracle is f64 Python, the native file is read by
`scripts/live-filter-probe/native.py` and the PSD by
`scripts/psd_qualification_decode.py`.

The foreground is PLACED with `place_image`, which lands a PNG as an ordinary
raster layer with the authored pixels and keeps nothing of the file, so T3
sees `losses == []`. An invert-duplicate
foreground is not an option here — it puts every channel sum exactly on the
Hard Mix threshold.
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

_spec = importlib.util.spec_from_file_location('threshold_modes_fixture_helpers',
                                               ROOT / 'scripts/verify_phase10d_separable.py')
_prior = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_prior)
support, digest, samples, png, history = (_prior.support, _prior.digest, _prior.samples,
                                          _prior.png, _prior.history)

# `precision_layer_blend::COLOR_MODES` ids, and the PSD key `PSD_BLEND_KEYS`
# pairs with each.
MODES = {'Hard Mix': (21, 'hMix'), 'Darker Color': (16, 'dkCl'),
         'Lighter Color': (17, 'lgCl'), 'Dissolve': (14, 'diss')}
OPACITIES = (1, 0.625)
NATIVE_VERSION = 34
FULL = 65535

# 4 x 3. Every channel SUM is at least 4 codes away from 65535, so no Hard Mix
# branch is decided by a rounding difference, and every backdrop/foreground
# luminance pair is at least 4 codes apart, so no Darker/Lighter Color pick is
# decided by one either. Both facts are asserted by `fixture_contract()`.
BACKDROP = [(0, 65535, 32767, FULL), (65535, 0, 32768, FULL),
            (16000, 16400, 16800, FULL), (32000, 33000, 34000, FULL),
            (8192, 49152, 23567, FULL), (50000, 23000, 4000, FULL),
            (30000, 50000, 7000, FULL), (65001, 55003, 45005, FULL),
            (18000, 37000, 59000, FULL), (10001, 20003, 30007, FULL),
            (65535, 65535, 65535, FULL), (17, 32767, 65501, FULL)]
FOREGROUND = [(65000, 300, 32000, FULL), (60000, 700, 32000, 32768),
              (49152, 50000, 65535, FULL), (16384, 16383, 50000, 45000),
              (57000, 10001, 30000, 32769), (12000, 43000, 61000, 1),
              (62000, 36000, 11000, 32771), (15001, 25003, 35005, FULL),
              (65000, 31000, 1000, 7), (255, 256, 257, 12345),
              (1003, 2005, 3007, FULL), (60000, 16384, 32768, 60000)]

MARGIN = 4


# ------------------------------------------------------------- the oracle --

def luminance(triple):
    """The slice's own `lum = 0.3 R + 0.59 G + 0.11 B` on normalized codes."""
    return 0.3 * triple[0] + 0.59 * triple[1] + 0.11 * triple[2]


def fixture_contract():
    """Both branch margins, asserted before anything is rendered."""
    sums, luminances = [], []
    for back, front in zip(BACKDROP, FOREGROUND, strict=True):
        for bc, sc in zip(back[:3], front[:3], strict=True):
            assert abs(bc + sc - FULL) >= MARGIN, (bc, sc)
            sums.append(bc + sc)
        gap = abs(luminance(back[:3]) - luminance(front[:3]))
        assert gap >= MARGIN, (back, front, gap)
        luminances.append(gap)
    assert any(value > FULL for value in sums) and any(value < FULL for value in sums)
    assert any(luminance(f[:3]) < luminance(b[:3]) for b, f in zip(BACKDROP, FOREGROUND))
    assert any(luminance(f[:3]) > luminance(b[:3]) for b, f in zip(BACKDROP, FOREGROUND))
    assert all(front[3] > 0 for front in FOREGROUND), 'every source pixel contributes'
    assert all(back[3] == FULL for back in BACKDROP), 'the backdrop is opaque'
    return dict(minimum_channel_sum_margin=min(abs(value - FULL) for value in sums),
                minimum_luminance_margin=min(luminances))


def dissolve_threshold(x, y):
    """The documented canvas-absolute 24-bit mix, exactly as the status doc
    states it; the quotient is a 24-bit integer over 2^24, so it is exact in
    f32 as well as f64."""
    h = (x * 0x9E3779B9 + y * 0x85EBCA6B) & 0xFFFFFFFF
    h ^= h >> 16
    h = (h * 0x85EBCA6B) & 0xFFFFFFFF
    h ^= h >> 13
    h = (h * 0xC2B2AE35) & 0xFFFFFFFF
    h ^= h >> 16
    return (h >> 8) / float(1 << 24)


def blend_triple(mode, back, front):
    """`B(cb, cs)` for the three non-Dissolve modes, on normalized triples."""
    if mode == 'Hard Mix':
        return tuple(1.0 if b + s >= 1.0 else 0.0
                     for b, s in zip(back, front, strict=True))
    if mode == 'Darker Color':
        return front if luminance(front) < luminance(back) else back
    if mode == 'Lighter Color':
        return front if luminance(front) > luminance(back) else back
    raise AssertionError(mode)


def expected(mode, opacity):
    """The whole 4 x 3 render. The backdrop is opaque, so the composite alpha
    is always 1 and the colour is `sa·B(cb, cs) + (1 - sa)·cb`."""
    output = []
    for index, (back, front) in enumerate(zip(BACKDROP, FOREGROUND, strict=True)):
        x, y = index % 4, index // 4
        sa = front[3] / FULL * opacity
        b = tuple(code / FULL for code in back[:3])
        s = tuple(code / FULL for code in front[:3])
        if mode == 'Dissolve':
            # Fully opaque source where the threshold admits it, otherwise the
            # backdrop pixel is untouched.
            shown = front[:3] if dissolve_threshold(x, y) < sa else back[:3]
            output.extend(list(shown) + [FULL])
            continue
        blended = blend_triple(mode, b, s)
        for bc, value in zip(b, blended, strict=True):
            output.append(FULL * (sa * value + (1 - sa) * bc))
        output.append(FULL)
    return tuple(output)


def dissolve_margin():
    """How close any pixel's threshold comes to its effective alpha. A tiny
    margin would make the exact Dissolve gate an f32/f64 coin toss."""
    worst = 1.0
    for index, front in enumerate(FOREGROUND):
        for opacity in OPACITIES:
            worst = min(worst, abs(dissolve_threshold(index % 4, index // 4)
                                   - front[3] / FULL * opacity))
    return worst


# ------------------------------------------------------------- the gates --

def gate_t1(call, folder, mode, opacity):
    path = folder / 'render.png'
    call('save_copy', path=str(path))
    value = support.png_samples(path.read_bytes())
    assert (value['width'], value['height'], value['depth']) == (4, 3, 16), value
    reference = expected(mode, opacity)
    errors = [abs(a - b) for a, b in zip(value['samples'], reference, strict=True)]
    if mode == 'Dissolve':
        assert max(errors) == 0, (mode, opacity, value['samples'], reference)
    else:
        assert max(errors) <= 1, (mode, opacity, max(errors))
    return max(errors)


def blend_matches(row, mode):
    """A typed document reports a layer's mode as its numeric id; a byte one
    would report the name, so both are accepted."""
    return row['blend_mode'] in (mode, MODES[mode][0])


def native_state(call, path, mode, opacity, names, baseline_version):
    """The stored file, read independently, with the slice's own claims."""
    call('save_copy', path=str(path))
    parsed = support.read_native(path)
    document = parsed['document']
    identifier = MODES[mode][0] if mode else 0
    assert parsed['version'] == (NATIVE_VERSION if mode else baseline_version), parsed['version']
    assert document['blend_version'] == (NATIVE_VERSION if mode else None)
    assert document.get('layer_blend_records', []) == ([(1, identifier)] if mode else [])
    assert (document['width'], document['height'], document['depth']) == (4, 3, 16)
    assert len(document['layers']) == len(document['fallbacks']) == 2
    for index, pixels in enumerate((BACKDROP, FOREGROUND)):
        row = document['layers'][index]
        decoded = support.png_samples(document['fallbacks'][index])
        assert (decoded['width'], decoded['height'], decoded['depth']) == (4, 3, 16)
        assert tuple(decoded['samples']) == samples(pixels), index
        assert row['name'] == names[index]
        assert row['blend'] == (0 if index == 0 else identifier)
        assert row['opacity'] == (1 if index == 0 else opacity)
        assert row['visible'] and row['mask'] is None and (row['x'], row['y']) == (0, 0)
    # A placed PNG is an ordinary raster layer, so nothing is retained.
    assert not any(parsed[key] for key in ('assets', 'bindings', 'resources'))
    return digest(path)


def gate_t2(app, call, folder, mode, opacity, names, baseline_sha, baseline_version):
    before = history(call)
    action = f'Threshold {mode} {opacity}'
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


def gate_t3(app, call, folder, mode, opacity, names):
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


def gate_t4(call, folder, baseline_version):
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
    report['gates']['T4 Pass Through refuses atomically'] = dict(
        passed=True, detail=gate_t4(call, output, baseline_version))
    cases = []
    for mode in MODES:
        for opacity in OPACITIES:
            folder = output / f"{mode.lower().replace(' ', '-')}-{opacity}"
            folder.mkdir()
            sha = gate_t2(app, call, folder, mode, opacity, names, baseline,
                          baseline_version)
            error = gate_t1(call, folder, mode, opacity)
            exchange = gate_t3(app, call, folder, mode, opacity, names)
            call('undo')
            assert native_state(call, folder / 'closed.rz', None, 1, names,
                                baseline_version) == baseline
            cases.append(dict(mode=mode, opacity=opacity, maximum_u16_error=error,
                              native_sha256=sha, exchange=exchange))
    report['cases'] = cases
    report['gates']['T1 appearance within 1 code, Dissolve exact'] = dict(
        passed=all(row['maximum_u16_error'] <= (0 if row['mode'] == 'Dissolve' else 1)
                   for row in cases),
        detail={f"{row['mode']} {row['opacity']}": row['maximum_u16_error'] for row in cases})
    report['gates']['T2 native 34 roundtrip, undo/redo/replay identical'] = dict(
        passed=True, detail={f"{row['mode']} {row['opacity']}": row['native_sha256']
                             for row in cases})
    report['gates']['T3 16-bit PSD carries the key with no loss'] = dict(
        passed=all(row['exchange']['losses'] == [] for row in cases),
        detail={f"{row['mode']} {row['opacity']}": row['exchange']['key'] for row in cases})
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
                  dissolve_threshold_margin=dissolve_margin(),
                  gates={}, cases=[], error=None)
    assert report['dissolve_threshold_margin'] > 1e-4, report['dissolve_threshold_margin']
    try:
        with support.HostApp(output / 'app', output / 'recovery', executable) as app:
            take(app, output, report)
    except BaseException as error:          # Recorded, never repaired.
        report['error'] = f'{type(error).__name__}: {error}'
    (output / 'result.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    gates = {name: gate['passed'] for name, gate in report['gates'].items()}
    print(json.dumps(dict(error=report['error'], gates=gates,
                          cases=[dict(mode=row['mode'], opacity=row['opacity'],
                                      maximum_u16_error=row['maximum_u16_error'])
                                 for row in report['cases']],
                          result=str(output / 'result.json')), indent=2))
    raise SystemExit(0 if not report['error'] and gates and all(gates.values()) else 1)


if __name__ == '__main__':
    main()
