#!/usr/bin/env python3
"""The 10D adjustment-blending slice's own MCP verifier, on a built Release app.

Four gates, all fixed before the run and never widened afterwards; a failed
gate IS the receipt.

- **A1 appearance.** The rendered 16-bit copy is within 1 code of an
  independent f64 oracle written from the byte rule the slice migrated
  (`core/src/doc.rs::composite_adjustment_into`): the ADJUSTED colour is the
  source against the backdrop, lerped back by `k = opacity x coverage`, so an
  opaque pixel is `cb + (B(cb, 1 - cb) - cb) · k` for an `invert` adjustment;
  alpha is never touched, an alpha-0 backdrop pixel stays `(0, 0, 0, 0)`, and
  Dissolve is EXACT (fully adjusted exactly where `t(x, y) < k`).
- **A2 persistence.** The native file is version 37 with one `LBLD` schema-7
  record on the kind-3 adjustment row, reopens at `precision: preserve` with
  the same mode, survives Undo, Redo and an Actions replay identically, and
  Normal restores the baseline bytes.
- **A3 exchange.** A 16-bit PSD export reports NO loss; an independent decode
  reads the row as `kind: adjustment`, `adjustment: nvrt` and the blend key
  `mul `/`scrn`/`diff`/`diss`; `inspect_psd` verifies no loss and the file
  reopens layered with the same mode.
- **A4 refusal.** `Pass Through` and a Blend If style on the adjustment both
  refuse atomically: bytes, history and selection are unchanged.

Nothing here derives an expectation from Doctored: the fixture is written by
this file, the oracle is f64 Python, the native file is read by
`scripts/live-filter-probe/native.py` and the PSD by
`scripts/psd_qualification_decode.py`. The backdrop is OPENED (never placed),
so no layer retains a source and the export has no preflight loss to report.
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

_spec = importlib.util.spec_from_file_location('adjustment_blending_helpers',
                                               ROOT / 'scripts/verify_phase10d_separable.py')
_prior = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_prior)
support, digest, samples, png, history = (_prior.support, _prior.digest, _prior.samples,
                                          _prior.png, _prior.history)

FULL = 65535
# `precision_layer_blend::COLOR_MODES` ids (Difference's is
# `verify_phase10d_separable.MODES`) and the PSD key each pairs with.
MODES = {'Multiply': (1, 'mul '), 'Screen': (2, 'scrn'),
         'Difference': (_prior.MODES['Difference'], 'diff'), 'Dissolve': (14, 'diss')}
OPACITIES = (1, 0.625)
NATIVE_VERSION = 37
ADJUSTMENT = 'invert'
PSD_ADJUSTMENT = 'nvrt'

# 4 x 3, the `png` writer's own geometry. Two pixels are fully transparent:
# an adjustment must leave them exactly `(0, 0, 0, 0)`, never lift them.
BACKDROP = [(0, 65535, 32767, FULL), (65535, 0, 32768, FULL),
            (16000, 16400, 16800, FULL), (0, 0, 0, 0),
            (8192, 49152, 23567, FULL), (50000, 23000, 4000, FULL),
            (30000, 50000, 7000, FULL), (65001, 55003, 45005, FULL),
            (0, 0, 0, 0), (10001, 20003, 30007, FULL),
            (65535, 65535, 65535, FULL), (17, 32767, 65501, FULL)]


def fixture_contract():
    transparent = [index for index, pixel in enumerate(BACKDROP) if pixel[3] == 0]
    assert len(transparent) == 2, transparent
    assert all(BACKDROP[index] == (0, 0, 0, 0) for index in transparent)
    assert all(pixel[3] == FULL for index, pixel in enumerate(BACKDROP)
               if index not in transparent)
    assert len(BACKDROP) == 12
    return dict(transparent_pixels=transparent, opaque_pixels=len(BACKDROP) - 2)


# ------------------------------------------------------------- the oracle --

def blend(mode, b, s):
    """`B(cb, cs)` on normalized channels, from the W3C separable set."""
    if mode == 'Multiply':
        return b * s
    if mode == 'Screen':
        return b + s - b * s
    if mode == 'Difference':
        return abs(b - s)
    raise AssertionError(mode)


def dissolve_threshold(x, y):
    """The documented canvas-absolute 24-bit mix (the threshold slice's own
    definition, shared by the adjustment path)."""
    h = (x * 0x9E3779B9 + y * 0x85EBCA6B) & 0xFFFFFFFF
    h ^= h >> 16
    h = (h * 0x85EBCA6B) & 0xFFFFFFFF
    h ^= h >> 13
    h = (h * 0xC2B2AE35) & 0xFFFFFFFF
    h ^= h >> 16
    return (h >> 8) / float(1 << 24)


def expected(mode, opacity):
    output = []
    for index, pixel in enumerate(BACKDROP):
        x, y = index % 4, index // 4
        if pixel[3] == 0:
            output.extend((0, 0, 0, 0))
            continue
        cb = [code / FULL for code in pixel[:3]]
        adjusted = [1.0 - value for value in cb]        # the `invert` op
        if mode == 'Dissolve':
            # Dissolve shows the adjusted colour WHOLE, so the expectation is
            # the exact integer code `FULL - v`, never a float reconstruction.
            shown = ([FULL - code for code in pixel[:3]]
                     if dissolve_threshold(x, y) < opacity else list(pixel[:3]))
            output.extend(shown + [pixel[3]])
            continue
        for b, s in zip(cb, adjusted, strict=True):
            output.append(FULL * (b + (blend(mode, b, s) - b) * opacity))
        output.append(pixel[3])
    return tuple(output)


def dissolve_margin():
    return min(abs(dissolve_threshold(index % 4, index // 4) - opacity)
               for index in range(len(BACKDROP)) for opacity in OPACITIES)


# -------------------------------------------------------------- the gates --

def gate_a1(call, folder, mode, opacity):
    path = folder / 'render.png'
    call('save_copy', path=str(path))
    value = support.png_samples(path.read_bytes())
    assert (value['width'], value['height'], value['depth']) == (4, 3, 16), value
    reference = expected(mode, opacity)
    actual = list(value['samples'])
    for index, pixel in enumerate(BACKDROP):
        window = slice(index * 4, index * 4 + 4)
        if pixel[3] == 0:
            assert tuple(actual[window]) == (0, 0, 0, 0), (index, actual[window])
        # Alpha is never touched by an adjustment, at any mode or opacity.
        assert actual[index * 4 + 3] == pixel[3], (index, actual[window])
    errors = [abs(a - b) for a, b in zip(actual, reference, strict=True)]
    if mode == 'Dissolve':
        assert max(errors) == 0, (mode, opacity, actual, reference)
    else:
        assert max(errors) <= 1, (mode, opacity, max(errors))
    return max(errors)


def blend_matches(row, mode):
    """A typed document reports a layer's mode as its numeric id; a byte one
    would report the name, so both are accepted."""
    return row['blend_mode'] in (mode, MODES[mode][0])


def native_state(call, path, mode, opacity, names, baseline_version):
    call('save_copy', path=str(path))
    parsed = support.read_native(path)
    document = parsed['document']
    identifier = MODES[mode][0] if mode else 0
    assert parsed['version'] == (NATIVE_VERSION if mode else baseline_version), parsed['version']
    assert document['blend_version'] == (NATIVE_VERSION if mode else None)
    assert document.get('layer_blend_records', []) == ([(1, identifier)] if mode else [])
    assert (document['width'], document['height'], document['depth']) == (4, 3, 16)
    assert len(document['layers']) == 2 and len(document['fallbacks']) == 1
    backdrop = document['layers'][0]
    decoded = support.png_samples(document['fallbacks'][0])
    assert (decoded['width'], decoded['height'], decoded['depth']) == (4, 3, 16)
    assert tuple(decoded['samples']) == samples(BACKDROP)
    assert backdrop['kind'] == 0 and backdrop['blend'] == 0 and backdrop['opacity'] == 1
    row = document['layers'][1]
    assert row['kind'] == 3, row
    assert json.loads(row['adjustment'])['op'] == ADJUSTMENT, row['adjustment']
    assert row['name'] == names[1] and row['opacity'] == opacity
    assert row['blend'] == identifier
    assert row['visible'] and row['mask'] is None and row['blend_if'] is None
    assert not any(parsed[key] for key in ('assets', 'bindings', 'resources'))
    return digest(path)


def gate_a2(app, call, folder, mode, opacity, names, baseline_sha, baseline_version):
    before = history(call)
    action = f'Adjustment {mode} {opacity}'
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

    call('set_layer_properties', index=1, blend_mode='Normal', opacity=1)
    assert native_state(call, folder / 'normal.rz', None, 1, names,
                        baseline_version) == baseline_sha
    call('undo')
    assert native_state(call, folder / 'restored.rz', mode, opacity, names,
                        baseline_version) == sha
    return sha


def gate_a3(app, call, folder, mode, names):
    key = MODES[mode][1]
    written = folder / 'adjustment16.psd'
    exported = call('export_psd', path=str(written), mode='layers')
    assert exported['bit_depth'] == 16, exported
    assert exported['losses'] == [], exported['losses']
    decoded = psd.decode(written)
    row = next(r for r in decoded['layers'] if r['name'] == names[1])
    assert row['kind'] == 'adjustment', row['kind']
    assert row['adjustment'] == PSD_ADJUSTMENT, row['adjustment']
    assert row['blend'] == key, (mode, row['blend'])
    report = app.call('inspect_psd', path=str(written))
    assert report['layered_available'] and report['import_precision'] == 'rgba16', report
    assert report['verified_losses'] == [], report['verified_losses']
    opened = app.call('open_document', path=str(written), psd_mode='layers',
                      psd_fingerprint=report['source_fingerprint'])
    assert opened['psd_import']['chosen_mode'] == 'layers', opened
    back = support.call_for(app, opened['document']['id'], folder / 'psd-calls.jsonl')
    reopened = back('get_document')['layers']
    assert blend_matches(reopened[1], mode), reopened[1]
    return dict(key=key, losses=exported['losses'], verified_losses=report['verified_losses'],
                reopened_blend_mode=reopened[1]['blend_mode'])


def gate_a4(call, folder):
    """Neither Pass Through nor a Blend If belongs on an adjustment record."""
    before = history(call)
    selection = call('get_selection')
    dirty = call('get_document')['unsaved_changes']
    path = folder / 'before-refusals.rz'
    call('save_copy', path=str(path))
    sha = digest(path)
    refusals = []
    cases = [
        ('set_layer_properties', dict(index=1, blend_mode='Pass Through')),
        ('set_layer_style', dict(layer=1, style={
            'fill_opacity': 1, 'effects': [],
            'blend_if': {'channel': 'red', 'this_layer': [0, 64, 192, 255],
                         'underlying': [32, 96, 160, 224]}})),
    ]
    for number, (tool, arguments) in enumerate(cases):
        error = call(tool, fail=True, **arguments)
        assert history(call) == before and call('get_selection') == selection
        assert call('get_document')['unsaved_changes'] == dirty
        after = folder / f'after-refusal-{number}.rz'
        call('save_copy', path=str(after))
        assert digest(after) == sha, tool
        refusals.append(dict(tool=tool, refusal=error,
                             bytes_history_selection_dirty_unchanged=True))
    return refusals


# ------------------------------------------------------------- the runner --

def take(app, output, report):
    source = output / 'backdrop16.png'
    png(source, BACKDROP)
    report['fixture_sha256'] = {source.name: digest(source)}
    identifier = app.call('open_document', path=str(source),
                          precision='preserve')['document']['id']
    call = support.call_for(app, identifier, output / 'calls.jsonl')
    assert call('get_precision')['mode'] == 'rgba16'
    base_name = call('get_document')['layers'][0]['name']
    call('add_adjustment_layer', op=ADJUSTMENT, name='Inverted')
    call('set_active_layer', index=1)
    names = [base_name, 'Inverted']
    report['layer_names'] = names
    report['gates']['A4 Pass Through and a Blend If refuse atomically'] = dict(
        passed=True, detail=gate_a4(call, output))
    baseline_path = output / 'baseline.rz'
    call('save_copy', path=str(baseline_path))
    baseline_version = support.read_native(baseline_path)['version']
    baseline = native_state(call, output / 'baseline-checked.rz', None, 1, names,
                            baseline_version)
    report['baseline_native_version'] = baseline_version
    report['baseline_native_sha256'] = baseline
    cases = []
    for mode in MODES:
        for opacity in OPACITIES:
            folder = output / f"{mode.lower()}-{opacity}"
            folder.mkdir()
            sha = gate_a2(app, call, folder, mode, opacity, names, baseline, baseline_version)
            error = gate_a1(call, folder, mode, opacity)
            exchange = gate_a3(app, call, folder, mode, names)
            call('undo')
            assert native_state(call, folder / 'closed.rz', None, 1, names,
                                baseline_version) == baseline
            cases.append(dict(mode=mode, opacity=opacity, maximum_u16_error=error,
                              native_sha256=sha, exchange=exchange))
    report['cases'] = cases
    report['gates']['A1 appearance within 1 code, alpha exact, Dissolve exact'] = dict(
        passed=all(row['maximum_u16_error'] <= (0 if row['mode'] == 'Dissolve' else 1)
                   for row in cases),
        detail={f"{row['mode']} {row['opacity']}": row['maximum_u16_error'] for row in cases})
    report['gates']['A2 native 37 schema7 roundtrip on the kind-3 record'] = dict(
        passed=True, detail={f"{row['mode']} {row['opacity']}": row['native_sha256']
                             for row in cases})
    report['gates']['A3 16-bit PSD carries nvrt with the key and no loss'] = dict(
        passed=all(row['exchange']['losses'] == []
                   and row['exchange']['verified_losses'] == [] for row in cases),
        detail={f"{row['mode']} {row['opacity']}": row['exchange']['key'] for row in cases})
    assert digest(source) == report['fixture_sha256'][source.name]
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
                  fixture_sha256={}, fixture_contract=fixture_contract(),
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
