#!/usr/bin/env python3
"""The 10D Blend If slice's own MCP verifier, on a built Release app.

Five gates, all fixed before the run and never widened afterwards; a failed
gate IS the receipt.

- **B1 publication.** `get_document` reports the authored ramp as
  `style.blend_if` with `effects: []`, and an IDENTITY ramp leaves the layer's
  `style` null (a Blend If that restricts nothing is not a style).
- **B2 appearance.** The rendered 16-bit copy is within 1 code of an
  independent f64 oracle written from `core/src/style_blend_if.rs`'s documented
  semantics: knees at `257 · k`, `w = this(v_source) · under(v_backdrop)`, and
  `out = cb + w · op · (cs - cb)`.
- **B3 persistence.** The native file is version 35 with one `LBIF` record on
  layer 1, reopens at `precision: preserve` byte-identically, survives Undo,
  Redo and an Actions replay identically, and the identity ramp restores the
  baseline bytes.
- **B4 exchange.** A 16-bit PSD export names the Blend If once, writes the two
  red ramps into the record's 40-byte blending-ranges section with every other
  slot unrestricted, and reopens layered with the same `style.blend_if`.
- **B5 adjustment.** `set_layer_style` with a Blend If on an ADJUSTMENT layer
  is ACCEPTED (it was refused before): `get_document`
  reports it as the row's `style.blend_if` with `effects: []`, it is ONE undo
  step, and Undo clears it again with the selection untouched.

Nothing here derives an expectation from Doctored: the fixture is written by
this file, the weight oracle is f64 Python, the native file is read by
`scripts/live-filter-probe/native.py` and the PSD by
`scripts/psd_qualification_decode.py`.

The top layer is a DUPLICATE inverted in place rather than a placed image, so
no layer retains a source and the 16-bit export has no preflight loss to
report.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import psd_qualification_decode as psd

_spec = importlib.util.spec_from_file_location('blend_if_probe_helpers',
                                               ROOT / 'scripts/verify_phase10d_separable.py')
_prior = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_prior)
support, digest, history = _prior.support, _prior.digest, _prior.history

FULL = 65535
# The document is 8 x 8 rather than the receipts' 32 x 32 for ONE reason: the
# gradient's own codes must stay clear of every ramp knee, and a 32-wide
# gradient steps by ~1835 codes, which cannot clear a knee by 2048 on both
# sides. `fixture_contract()` asserts the margin that is actually achieved.
SIZE = 8
KNEE_MARGIN = 2048
OPACITIES = (1, 0.625)
NATIVE_VERSION = 35
CHANNEL = 'red'
THIS_LAYER = [0, 64, 192, 255]
UNDERLYING = [32, 96, 160, 224]
IDENTITY = [0, 0, 255, 255]
RAMP = {'channel': CHANNEL, 'this_layer': THIS_LAYER, 'underlying': UNDERLYING}
IDENTITY_RAMP = {'channel': CHANNEL, 'this_layer': IDENTITY, 'underlying': IDENTITY}


# ------------------------------------------------------------- the fixture --

def png16(path, width, height, samples):
    """An RGBA16 PNG carrying the sRGB chunk, written without any application.

    The master copy is `verify_phase10d_separable.png`; this is that writer
    generalised to any size, exactly as `verify_phase10_receipts.png16` is.
    """
    def chunk(kind, body):
        return (struct.pack('>I', len(body)) + kind + body
                + struct.pack('>I', zlib.crc32(kind + body)))

    assert len(samples) == width * height * 4
    rows = b''.join(b'\0' + struct.pack(f'>{width * 4}H',
                                        *samples[y * width * 4:(y + 1) * width * 4])
                    for y in range(height))
    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 16, 6, 0, 0, 0))
                     + chunk(b'sRGB', b'\0') + chunk(b'IDAT', zlib.compress(rows))
                     + chunk(b'IEND', b''))


def base16(width=SIZE, height=SIZE):
    """`verify_phase10_receipts.base16`, whose every colour channel runs from
    4096 to 61000 so no code sits at 0 or 65535."""
    span = 61000 - 4096
    out = []
    for y in range(height):
        for x in range(width):
            out += [4096 + span * x // (width - 1), 4096 + span * y // (height - 1),
                    4096 + span * (x + y) // (width + height - 2), FULL]
    return out


def knees(ramp):
    """An 8-bit knee `k` is the code `257 · k` (the promotion, exact)."""
    return [257.0 * k for k in ramp]


def fixture_contract():
    """Every red code the ramps will read, and its distance to every knee.

    A code sitting ON a knee would make the oracle's branch a rounding
    question rather than a fact, so the margin is asserted before the run.
    """
    picture = base16()
    backdrop = picture[0::4]
    source = [FULL - code for code in backdrop]
    everything = knees(THIS_LAYER) + knees(UNDERLYING)
    margin = min(abs(code - knee) for code in set(backdrop) | set(source)
                 for knee in everything if 0 < knee < FULL)
    assert margin >= KNEE_MARGIN, margin
    weights = {ramp_weight(code, knees(THIS_LAYER)) * ramp_weight(other, knees(UNDERLYING))
               for code, other in zip(source, backdrop, strict=True)}
    # The fixture has to exercise the ramp, not one flat weight.
    assert len(weights) > 3 and min(weights) < 0.5 < max(weights), sorted(weights)
    return dict(minimum_knee_margin_codes=margin, distinct_weights=len(weights),
                weight_range=[min(weights), max(weights)])


# ------------------------------------------------------------- the oracle --

def ramp_weight(value, ramp_knees):
    """`style_blend_if::ramp_weight_at`, written independently in f64."""
    lo0, lo1, hi0, hi1 = ramp_knees
    if value < lo0:
        return 0.0
    if value < lo1:
        return (value - lo0) / (lo1 - lo0)
    if value <= hi0:
        return 1.0
    if value <= hi1:
        return (hi1 - value) / (hi1 - hi0)
    return 0.0


def expected(opacity):
    """The whole render: an opaque backdrop under an opaque inverted copy
    whose alpha is scaled by the Blend If weight."""
    picture = base16()
    output = []
    for index in range(SIZE * SIZE):
        back = picture[index * 4:index * 4 + 4]
        source = [FULL - code for code in back[:3]] + [FULL]
        weight = (ramp_weight(source[0], knees(THIS_LAYER))
                  * ramp_weight(back[0], knees(UNDERLYING)))
        for cb, cs in zip(back[:3], source[:3], strict=True):
            output.append(cb + weight * opacity * (cs - cb))
        output.append(FULL)
    return tuple(output)


def inverted_base():
    picture = base16()
    out = []
    for index in range(SIZE * SIZE):
        out += [FULL - code for code in picture[index * 4:index * 4 + 3]] + [FULL]
    return tuple(out)


# -------------------------------------------------------------- the gates --

def gate_b1(call, layer, expected_style):
    rows = call('get_document')['layers']
    style = rows[layer].get('style')
    if expected_style is None:
        assert style is None, style
        return None
    assert style is not None, rows[layer]
    assert style['blend_if'] == expected_style, style
    assert style['effects'] == [], style
    return style


def gate_b2(call, folder, opacity, label):
    path = folder / f'{label}.png'
    call('save_copy', path=str(path))
    value = support.png_samples(path.read_bytes())
    assert (value['width'], value['height'], value['depth']) == (SIZE, SIZE, 16), value
    reference = expected(opacity)
    error = max(abs(a - b) for a, b in zip(value['samples'], reference, strict=True))
    assert error <= 1, (opacity, error)
    return error


def native_state(call, path, opacity, names, ramp, baseline_version):
    call('save_copy', path=str(path))
    parsed = support.read_native(path)
    document = parsed['document']
    assert parsed['version'] == (NATIVE_VERSION if ramp else baseline_version), parsed['version']
    assert document['blend_if_version'] == (NATIVE_VERSION if ramp else None)
    records = document.get('blend_if_records', [])
    assert records == ([(1, ramp)] if ramp else []), records
    assert (document['width'], document['height'], document['depth']) == (SIZE, SIZE, 16)
    assert len(document['layers']) == len(document['fallbacks']) == 2
    for index, pixels in enumerate((tuple(base16()), inverted_base())):
        row = document['layers'][index]
        decoded = support.png_samples(document['fallbacks'][index])
        assert (decoded['width'], decoded['height'], decoded['depth']) == (SIZE, SIZE, 16)
        assert tuple(decoded['samples']) == pixels, index
        assert row['name'] == names[index]
        assert row['blend'] == 0 and row['style'] is None
        assert row['opacity'] == (1 if index == 0 else opacity)
        assert row['visible'] and row['mask'] is None and (row['x'], row['y']) == (0, 0)
    assert not any(parsed[key] for key in ('assets', 'bindings', 'resources'))
    return digest(path)


def gate_b3(app, call, folder, opacity, names, baseline_sha, baseline_version):
    stored = dict(channel=CHANNEL, this_layer=THIS_LAYER, underlying=UNDERLYING)
    before = history(call)
    action = f'Blend If red {opacity}'
    app.call('start_recording')
    call('set_layer_style', layer=1,
         style={'fill_opacity': 1, 'blend_if': RAMP, 'effects': []})
    recorded = app.call('stop_recording', name=action)
    assert [step['tool'] for step in recorded['steps']] == ['set_layer_style'], recorded
    assert history(call)[1] == before[1] + 1
    sha = native_state(call, folder / 'blendif.rz', opacity, names, stored, baseline_version)

    identifier = app.call('open_document', path=str(folder / 'blendif.rz'),
                          precision='preserve')['document']['id']
    reopened = support.call_for(app, identifier, folder / 'reopen-calls.jsonl')
    assert native_state(reopened, folder / 'reopened.rz', opacity, names, stored,
                        baseline_version) == sha
    gate_b1(reopened, 1, RAMP)

    call('undo')
    assert history(call)[0] == before[0]
    assert native_state(call, folder / 'undone.rz', opacity, names, None,
                        baseline_version) == baseline_sha
    call('redo')
    assert native_state(call, folder / 'redone.rz', opacity, names, stored,
                        baseline_version) == sha
    call('undo')
    assert call('run_action', name=action)['ok']
    assert history(call)[1] == before[1] + 1
    assert native_state(call, folder / 'replayed.rz', opacity, names, stored,
                        baseline_version) == sha

    # An identity ramp is no style at all: the prior writer's exact bytes.
    call('set_layer_style', layer=1,
         style={'fill_opacity': 1, 'blend_if': IDENTITY_RAMP, 'effects': []})
    gate_b1(call, 1, None)
    assert native_state(call, folder / 'identity.rz', opacity, names, None,
                        baseline_version) == baseline_sha
    call('undo')
    assert native_state(call, folder / 'restored.rz', opacity, names, stored,
                        baseline_version) == sha
    return sha


def gate_b4(app, call, folder, names):
    written = folder / 'blendif16.psd'
    exported = call('export_psd', path=str(written), mode='layers')
    assert exported['bit_depth'] == 16, exported
    named = [line for line in exported['observations'] if 'Blend If on red' in line]
    assert len(named) == 1, exported['observations']
    decoded = psd.decode(written)
    row = next(r for r in decoded['layers'] if r['name'] == names[1])
    ranges = list(row['blend_ranges'])
    # `core/src/psd_layer_effects.rs::encode_ranges`: gray, red, green, blue,
    # alpha, each this-layer then underlying. Red is slot index 1.
    assert len(ranges) == 40, len(ranges)
    assert ranges[8:12] == THIS_LAYER, ranges[8:12]
    assert ranges[12:16] == UNDERLYING, ranges[12:16]
    assert ranges[0:8] == IDENTITY * 2, ranges[0:8]
    assert ranges[16:] == IDENTITY * 6, ranges[16:]
    report = app.call('inspect_psd', path=str(written))
    assert report['layered_available'] and report['import_precision'] == 'rgba16', report
    opened = app.call('open_document', path=str(written), psd_mode='layers',
                      psd_fingerprint=report['source_fingerprint'])
    assert opened['psd_import']['chosen_mode'] == 'layers', opened
    back = support.call_for(app, opened['document']['id'], folder / 'psd-calls.jsonl')
    style = gate_b1(back, 1, RAMP)
    return dict(observation=named[0], blend_ranges=ranges, reopened_style=style)


def gate_b5(app, call, folder):
    """Blend If on an adjustment layer: accepted, reported, one step."""
    call('add_adjustment_layer', op='invert', name='Adjustment')
    rows = call('get_document')['layers']
    index = next(row['index'] for row in rows if row['name'] == 'Adjustment')
    before = history(call)
    selection = call('get_selection')
    call('set_layer_style', layer=index,
         style={'fill_opacity': 1, 'blend_if': RAMP, 'effects': []})
    style = gate_b1(call, index, RAMP)
    after = history(call)
    assert after[1] == before[1] + 1 and after[2] == 0, (before, after)
    assert call('get_selection') == selection
    path = folder / 'adjustment-blend-if.rz'
    call('save_copy', path=str(path))
    call('undo')
    assert history(call)[1] == before[1], history(call)
    assert call('get_document')['layers'][index].get('style') is None
    call('undo')
    return dict(style=style, history_before=before, history_after=after,
                native_sha256=digest(path))


# ------------------------------------------------------------- the runner --

def take(app, output, report):
    source = output / 'base16.png'
    png16(source, SIZE, SIZE, base16())
    report['fixture_sha256'] = {source.name: digest(source)}
    identifier = app.call('open_document', path=str(source),
                          precision='preserve')['document']['id']
    call = support.call_for(app, identifier, output / 'calls.jsonl')
    assert call('get_precision')['mode'] == 'rgba16'
    base_name = call('get_document')['layers'][0]['name']
    call('duplicate_layer', index=0)
    call('set_layer_properties', index=1, name='Blend If')
    call('apply_filter', filter='invert', layer=1)
    call('set_active_layer', index=1)
    names = [base_name, 'Blend If']
    report['layer_names'] = names
    report['gates']['B5 a Blend If on an adjustment is accepted as one step'] = dict(
        passed=True, detail=gate_b5(app, call, output))
    cases = []
    for opacity in OPACITIES:
        folder = output / f'opacity-{opacity}'
        folder.mkdir()
        if opacity != 1:
            call('set_layer_properties', index=1, opacity=opacity)
        probe_path = folder / 'baseline-probe.rz'
        call('save_copy', path=str(probe_path))
        baseline_version = support.read_native(probe_path)['version']
        baseline = native_state(call, folder / 'baseline.rz', opacity, names, None,
                                baseline_version)
        gate_b1(call, 1, None)
        sha = gate_b3(app, call, folder, opacity, names, baseline, baseline_version)
        error = gate_b2(call, folder, opacity, 'render')
        style = gate_b1(call, 1, RAMP)
        exchange = gate_b4(app, call, folder, names)
        call('undo')
        assert native_state(call, folder / 'closed.rz', opacity, names, None,
                            baseline_version) == baseline
        cases.append(dict(opacity=opacity, maximum_u16_error=error, native_sha256=sha,
                          published_style=style, exchange=exchange,
                          baseline_native_version=baseline_version,
                          baseline_native_sha256=baseline))
    report['cases'] = cases
    report['gates']['B1 the ramp is published and an identity is no style'] = dict(
        passed=all(row['published_style']['blend_if'] == RAMP for row in cases),
        detail={str(row['opacity']): row['published_style'] for row in cases})
    report['gates']['B2 appearance within 1 code of the ramp oracle'] = dict(
        passed=all(row['maximum_u16_error'] <= 1 for row in cases),
        detail={str(row['opacity']): row['maximum_u16_error'] for row in cases})
    report['gates']['B3 native 35 roundtrip, undo/redo/replay identical'] = dict(
        passed=True, detail={str(row['opacity']): row['native_sha256'] for row in cases})
    report['gates']['B4 16-bit PSD carries the two red ramps'] = dict(
        passed=True,
        detail={str(row['opacity']): row['exchange']['blend_ranges'][8:16] for row in cases})
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
                  gates={}, cases=[], error=None)
    try:
        with support.HostApp(output / 'app', output / 'recovery', executable) as app:
            take(app, output, report)
    except BaseException as error:          # Recorded, never repaired.
        report['error'] = f'{type(error).__name__}: {error}'
    (output / 'result.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    gates = {name: gate['passed'] for name, gate in report['gates'].items()}
    print(json.dumps(dict(error=report['error'], gates=gates,
                          cases=[dict(opacity=row['opacity'],
                                      maximum_u16_error=row['maximum_u16_error'])
                                 for row in report['cases']],
                          result=str(output / 'result.json')), indent=2))
    raise SystemExit(0 if not report['error'] and gates and all(gates.values()) else 1)


if __name__ == '__main__':
    main()
