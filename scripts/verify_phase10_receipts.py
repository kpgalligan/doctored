#!/usr/bin/env python3
"""Take the Phase 10 closeout receipts R1-R3 and R5-R9 against a built app.

R1-R3 are MCP drives of the built bundle: the eight binary-body adjustment
kinds, the three descriptor kinds, and the `lfx2`/`iOpa`/Blend If carry, each
exported as a PSD, preflighted and reopened at BOTH depths. R5-R9 hand a
Doctored-written 16-bit PSD to GIMP 3.2.6 through the existing
`scripts/psd-exchange-probe` launcher and gate what the independent reader saw:
a clipping chain, the four threshold-mode blend keys, Blend If, an
adjustment record's blend key, and the four HSL-mode blend keys. Nothing here
derives an expectation from the Doctored importer: every byte gate reads `scripts/psd_qualification_decode.py`
and every reopening gate reads GIMP's own `gimp.json`. The gates were fixed in
`archive/phase-10c-constructs-status.md#manual-receipts-taken-2026-09-16`
before the run (R9's in its own plan); a failed one is recorded as the
receipt, never repaired.

R4 (the regenerated `styled_effects` corpus case) is taken by
`scripts/verify_phase10b_external.py` and is never read or written here.
"""
import argparse
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import psd_qualification_decode as psd
import verify_phase10b_external as external
from verify_phase1 import png as png8_file

support = external.support
digest = external.digest

# The four keys the 10B corpus declares, plus this run's own four threshold
# and four HSL modes and the layer-mode nick GIMP must report for each.
GIMP_NICKS = external.BLENDS | {'hMix': 'hard-mix', 'dkCl': 'luma-darken-only',
                                'lgCl': 'luma-lighten-only', 'diss': 'dissolve',
                                'hue ': 'lch-hue', 'sat ': 'lch-chroma',
                                'colr': 'lch-color', 'lum ': 'lch-lightness'}
# The eight adjustment kinds whose Photoshop record is a binary body.
BINARY_OPS = {
    'invert': {},
    'threshold': {'level': 0.6},
    'posterize': {'levels': 4},
    'levels': {'black': 0.2, 'white': 0.8, 'gamma': 1.5},
    'curves': {'rgb': [[0, 0], [64, 80], [255, 255]]},
    'color_balance': {'midtones': {'cyan_red': 0.25, 'magenta_green': -0.1, 'yellow_blue': 0.3},
                      'preserve_luminosity': False},
    'hue_saturation': {'hue': 20, 'saturation': 0.25, 'lightness': -0.1},
    'photo_filter': {'color': '#ec8a00', 'density': 0.4, 'preserve_luminosity': True},
}
# The three whose Photoshop record is a descriptor body.
DESCRIPTOR_OPS = {
    'exposure': {'exposure': 1.5, 'offset': -0.25, 'gamma': 2.0},
    'vibrance': {'vibrance': 0.4, 'saturation': -0.2},
    'black_and_white': {'reds': 0.3, 'yellows': -0.5, 'greens': 1.2, 'cyans': 0.6,
                        'blues': 2.5, 'magentas': 0, 'tint': True, 'tint_color': '#998a66'},
}
# The style `core/tests/psd_export_corpus.rs::style()` builds, in the shape
# `set_layer_style` takes: the same three effects, fill opacity and Blend If.
CORPUS_STYLE = {
    'fill_opacity': 0.5,
    'blend_if': {'channel': 'gray', 'this_layer': [0, 10, 240, 255],
                 'underlying': [0, 0, 255, 255]},
    'effects': [
        {'type': 'drop_shadow', 'distance': 5, 'angle': 120,
         'use_global_light': False, 'size': 4},
        {'type': 'stroke', 'size': 2, 'position': 'outside'},
        {'type': 'color_overlay'},
    ],
}
GIMP_RECEIPTS = ('R5', 'R6', 'R7', 'R8', 'R9')


# --------------------------------------------------------------- fixtures --

def png16(path, width, height, samples):
    """An RGBA16 PNG carrying the sRGB chunk, written without any application.

    `verify_phase10d_separable.png`, generalised to any size.
    """
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))

    assert len(samples) == width * height * 4
    rows = b''.join(b'\0' + struct.pack(f'>{width * 4}H', *samples[y * width * 4:(y + 1) * width * 4])
                    for y in range(height))
    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 16, 6, 0, 0, 0))
                     + chunk(b'sRGB', b'\0') + chunk(b'IDAT', zlib.compress(rows))
                     + chunk(b'IEND', b''))


def base8(width=32, height=32):
    """An opaque byte gradient whose every colour channel is in 16..239."""
    out = bytearray()
    for y in range(height):
        for x in range(width):
            out += bytes((16 + 223 * x // (width - 1), 16 + 223 * y // (height - 1),
                          16 + 223 * (x + y) // (width + height - 2), 255))
    return bytes(out)


def base16(width=32, height=32):
    """The same gradient at 16 bits, every colour channel in 4096..61000.

    The ceiling matters for R7: a layer whose codes stay below it can never
    reach the Blend If knee at code 257 x 250, so the layer is excluded.
    """
    span = 61000 - 4096
    out = []
    for y in range(height):
        for x in range(width):
            out += [4096 + span * x // (width - 1), 4096 + span * y // (height - 1),
                    4096 + span * (x + y) // (width + height - 2), 65535]
    return out


# ------------------------------------------------------------ MCP helpers --

def open_base(ctx, depth, name):
    """Writes this receipt's base picture and opens it at its own depth."""
    folder = ctx.folder / name
    folder.mkdir(parents=True, exist_ok=True)
    path = folder / f'base{depth}.png'
    if depth == 8:
        png8_file(path, 32, 32, base8())
    else:
        png16(path, 32, 32, base16())
    opened = ctx.app.call('open_document', path=str(path), precision='preserve')
    identifier = opened['document']['id']
    call = support.call_for(ctx.app, identifier, folder / 'calls.jsonl')
    assert call('get_precision')['mode'] == ('rgba8' if depth == 8 else 'rgba16')
    return identifier, call


def layer_index(call, name):
    """Every structural edit renumbers the stack, so layers are addressed by
    NAME and the index is re-read immediately before each call."""
    rows = call('get_document')['layers']
    matches = [row['index'] for row in rows if row['name'] == name]
    assert len(matches) == 1, (name, [row['name'] for row in rows])
    return matches[0]


def duplicate(call, name, new_name):
    """Duplicates the named layer and renames the copy."""
    result = call('duplicate_layer', index=layer_index(call, name))
    landing = result['layers'][0]
    call('set_layer_properties', index=landing, name=new_name)
    return layer_index(call, new_name)


def carve(call, name, rect):
    """Leaves the named layer as the inverse of the base inside `rect` only.

    16-bit mode refuses `new_layer`/`reorder_layer`, so the only way to author
    a distinct layer there is duplicate + filter + selection, all of which are
    typed-capable (`app/Sources/DocumentPrecision.swift`).
    """
    x, y, width, height = rect
    call('apply_filter', filter='invert', layer=layer_index(call, name))
    call('select_rect', x=x, y=y, width=width, height=height)
    call('modify_selection', operation='invert')
    call('clear_selection', layer=layer_index(call, name))
    call('deselect')


def _same(want, got, tol):
    if isinstance(want, bool) or isinstance(got, bool):
        return want is got
    if isinstance(want, dict):
        return isinstance(got, dict) and all(
            key in got and _same(value, got[key], tol) for key, value in want.items())
    if isinstance(want, list):
        return isinstance(got, list) and len(want) == len(got) and all(
            _same(a, b, tol) for a, b in zip(want, got))
    if isinstance(want, (int, float)) and isinstance(got, (int, float)):
        return abs(want - got) <= tol
    return want == got


def subset_equal(authored, reopened, tol=1e-6):
    """Every authored key is present in the reopened params and equal."""
    assert _same(authored, reopened, tol), (authored, reopened)


def typed_adjustments(rz_path):
    """The stored operator description of every typed adjustment record."""
    layers = support.read_native(rz_path)['document']['layers']
    return {row['name']: json.loads(row['adjustment'])
            for row in layers.values() if row.get('adjustment')}


# ---------------------------------------------------- the independent read --

def reopen_in_gimp(ctx, receipt, cases):
    """Hands the authored PSDs to GIMP through the pinned private launcher."""
    authored = ctx.folder / 'authored'
    for case, source in cases.items():
        target = authored / case
        target.mkdir(parents=True)
        shutil.copyfile(source, target / 'source.psd')
        (target / 'manifest.json').write_text(json.dumps(
            dict(schema=1, name=case, receipt=receipt,
                 authored=dict(source_psd_sha256=digest(target / 'source.psd'))),
            indent=2) + '\n')
    output, state = ctx.folder / 'gimp/corpus', ctx.folder / 'gimp/state'
    command = [sys.executable, '-B', str(ROOT / 'scripts/psd-exchange-probe/run_reopen.py'),
               '--gimp', str(ctx.gimp), '--source', str(authored), '--output', str(output),
               '--state', str(state), '--cases', ','.join(sorted(cases))]
    finished = subprocess.run(command, capture_output=True, text=True)
    (ctx.folder / 'gimp-run.json').write_text(json.dumps(
        dict(command=command, returncode=finished.returncode, stdout=finished.stdout,
             stderr=finished.stderr), indent=2) + '\n')
    assert finished.returncode == 0, (finished.returncode, finished.stdout, finished.stderr)
    return {case: json.loads((output / case / 'gimp.json').read_text()) for case in cases}


def structure(record, decoded, depth):
    """Gate 2's shape, with this run's wider blend-key vocabulary."""
    return external.gate_structure(record, decoded, depth, blends=GIMP_NICKS)


def reopened_samples(ctx, case, decoded, depth):
    return external.reference(ctx.folder / 'gimp/corpus' / case / 'reopened.png',
                              decoded['canvas'], depth, decoded['profile'])


def merged_samples(record, decoded, depth):
    merged = record['merged']
    assert merged['precision'] == external.PRECISION[depth]
    assert merged['canvas'] == decoded['canvas']
    assert len(merged['layers']) == 1 and merged['layers'][0]['mask'] is None
    return merged['layers'][0]['rgba']


def source_over(row, backdrop, canvas, depth):
    """The straight source-over of ONE decoded record over the base picture."""
    full = external.full_code(depth)
    out = list(backdrop)
    (ox, oy), (width, height) = row['offset'], row['size']
    samples = list(row['rgba'])
    for y in range(height):
        for x in range(width):
            cx, cy = ox + x, oy + y
            if not (0 <= cx < canvas[0] and 0 <= cy < canvas[1]):
                continue
            source, target = (y * width + x) * 4, (cy * canvas[0] + cx) * 4
            alpha = samples[source + 3] / full
            if alpha == 0:
                continue
            for c in range(3):
                out[target + c] = round(samples[source + c] * alpha + out[target + c] * (1 - alpha))
            out[target + 3] = round(samples[source + 3] + out[target + 3] * (1 - alpha))
    return out


# ---------------------------------------------------------- R1/R2: carries --

def adjustment_carry(ctx, ops):
    """ONE drive per depth: author, export, preflight, reopen, compare."""
    checks = {}
    for depth in (8, 16):
        folder = ctx.folder / f'depth{depth}'
        _, call = open_base(ctx, depth, f'depth{depth}')
        for op, params in ops.items():
            call('add_adjustment_layer', op=op, params=params, name=op)
        written = folder / f'adjustments{depth}.psd'
        exported = call('export_psd', path=str(written), mode='layers')
        assert exported['bit_depth'] == depth, exported
        assert not any('cannot be exported' in loss for loss in exported['losses']), exported
        for op in ops:
            written_as = [line for line in exported['observations']
                          if f'Adjustment layer “{op}” ({op}) is written as a Photoshop ' in line
                          and ' adjustment (' in line]
            assert len(written_as) == 1, (op, exported['observations'])
        report = ctx.app.call('inspect_psd', path=str(written))
        assert report['layered_available'], report
        assert report['import_precision'] == ('rgba8' if depth == 8 else 'rgba16'), report
        for op in ops:
            carried = [line for line in report['observations']
                       if f'(“{op}”)' in line
                       and 'layered import carries it as a Doctored ' in line
                       and ' adjustment layer' in line]
            assert len(carried) == 1, (op, report['observations'])
        assert not any('omits it' in loss for loss in report['verified_losses']), report
        opened = ctx.app.call('open_document', path=str(written), psd_mode='layers',
                              psd_fingerprint=report['source_fingerprint'])
        assert opened['psd_import']['chosen_mode'] == 'layers', opened
        reopened = support.call_for(ctx.app, opened['document']['id'],
                                    folder / f'reopen{depth}.jsonl')
        rows = {row['name']: row for row in reopened('get_document')['layers']}
        if depth == 8:
            for op, params in ops.items():
                row = rows[op]
                assert row['is_adjustment'] is True, row
                assert row['adjustment']['op'] == op, row['adjustment']
                subset_equal(params, row['adjustment']['params'])
            evaluations = {op: rows[op]['adjustment']['params'].get('evaluation')
                           for op in ops}
        else:
            for op in ops:
                row = rows[op]
                # One vocabulary at both depths; the typed object
                # also carries its evaluation.
                assert row['is_adjustment'] is True, row
                assert row['adjustment']['op'] == op, row['adjustment']
                assert row['adjustment']['evaluation'] == 'precision_v1', row['adjustment']
            native = folder / f'reopened{depth}.rz'
            reopened('save_copy', path=str(native))
            stored = typed_adjustments(native)
            for op, params in ops.items():
                assert stored[op]['op'] == op, stored[op]
                subset_equal(params, stored[op]['params'])
            evaluations = {op: stored[op]['params'].get('evaluation') for op in ops}
        checks[f'depth{depth}'] = dict(
            exported_bit_depth=exported['bit_depth'], losses=exported['losses'],
            carried=sorted(ops), evaluations=evaluations,
            import_losses=report['verified_losses'])
    return checks


def receipt_r1(ctx):
    return adjustment_carry(ctx, BINARY_OPS)


def receipt_r2(ctx):
    checks = adjustment_carry(ctx, DESCRIPTOR_OPS)
    for depth in (8, 16):
        evaluation = checks[f'depth{depth}']['evaluations']['exposure']
        assert evaluation == 'document_v2', (depth, evaluation)
    return checks


# ------------------------------------------------------- R3: the lfx2 carry --

def receipt_r3(ctx):
    checks = {}
    for depth in (8, 16):
        folder = ctx.folder / f'depth{depth}'
        _, call = open_base(ctx, depth, f'depth{depth}')
        base = call('get_document')['layers'][0]['name']
        duplicate(call, base, 'Styled')
        carve(call, 'Styled', (8, 8, 12, 12))
        call('set_layer_style', layer=layer_index(call, 'Styled'), style=CORPUS_STYLE)
        preflight = call('inspect_psd_export', mode='layers')
        assert preflight['available'] is True, preflight
        preserve = [line for line in preflight['observations']
                    if 'has a layer style (drop_shadow, stroke, color_overlay, '
                       'fill opacity 50 %, Blend If on gray)' in line
                    and 'written as Photoshop layer effects' in line]
        assert len(preserve) == 1, preflight['observations']
        assert not any('rasterized' in loss.lower() for loss in preflight['losses']), preflight
        written = folder / f'styled{depth}.psd'
        exported = call('export_psd', path=str(written), mode='layers')
        assert exported['bit_depth'] == depth, exported
        assert not any('rasterized' in loss.lower() for loss in exported['losses']), exported
        report = ctx.app.call('inspect_psd', path=str(written))
        assert report['layered_available'], report
        carried = [line for line in report['observations']
                   if 'has Photoshop layer effects (' in line
                   and 'carries them as a Doctored layer style' in line]
        assert len(carried) == 1, report['observations']
        opened = ctx.app.call('open_document', path=str(written), psd_mode='layers',
                              psd_fingerprint=report['source_fingerprint'])
        reopened = support.call_for(ctx.app, opened['document']['id'],
                                    folder / f'reopen{depth}.jsonl')
        row = next(r for r in reopened('get_document')['layers'] if r['name'] == 'Styled')
        style = row['style']
        if isinstance(style, str):
            style = json.loads(style)
        assert [effect['type'] for effect in style['effects']] == [
            'drop_shadow', 'stroke', 'color_overlay'], style['effects']
        # `iOpa` is ONE byte, so 0.5 travels as 128/255 and reopens as 0.502:
        # the round-trip identity is the byte, not the authored fraction.
        assert round(style['fill_opacity'] * 255) == 128, style['fill_opacity']
        assert style['blend_if'] == CORPUS_STYLE['blend_if'], style['blend_if']
        checks[f'depth{depth}'] = dict(preserve_observation=preserve[0],
                                       carry_observation=carried[0],
                                       export_losses=exported['losses'],
                                       reopened_style=style)
    # The bake case: a sub-1 px stroke is the one thing Photoshop's own dialog
    # cannot hold, so the effects go into the layer's pixels instead.
    folder = ctx.folder / 'bake'
    _, call = open_base(ctx, 8, 'bake')
    base = call('get_document')['layers'][0]['name']
    duplicate(call, base, 'Baked')
    carve(call, 'Baked', (8, 8, 12, 12))
    call('set_layer_style', layer=layer_index(call, 'Baked'),
         style={'effects': [{'type': 'stroke', 'size': 0.5}]})
    preflight = call('inspect_psd_export', mode='layers')
    bake = [loss for loss in preflight['losses']
            if "stroke size is below Photoshop's 1 px minimum" in loss
            and "rasterized into the layer's pixels" in loss]
    assert len(bake) == 1, preflight['losses']
    written = folder / 'bake8.psd'
    exported = call('export_psd', path=str(written), mode='layers')
    assert exported['bit_depth'] == 8, exported
    report = ctx.app.call('inspect_psd', path=str(written))
    opened = ctx.app.call('open_document', path=str(written), psd_mode='layers',
                          psd_fingerprint=report['source_fingerprint'])
    reopened = support.call_for(ctx.app, opened['document']['id'], folder / 'reopen8.jsonl')
    row = next(r for r in reopened('get_document')['layers'] if r['name'] == 'Baked')
    assert 'style' not in row or row['style'] is None, row
    checks['bake'] = dict(loss=bake[0], reopened_style=row.get('style'))
    return checks


# ------------------------------------------------ R5-R9: the GIMP reopens --

def receipt_r5(ctx):
    folder = ctx.folder / 'depth16'
    _, call = open_base(ctx, 16, 'depth16')
    base = call('get_document')['layers'][0]['name']
    duplicate(call, base, 'Clip base')
    duplicate(call, 'Clip base', 'Clipped')
    carve(call, 'Clip base', (4, 4, 16, 16))
    carve(call, 'Clipped', (0, 0, 32, 20))
    call('set_layer_clipped', layer=layer_index(call, 'Clipped'), clipped=True)
    call('set_layer_properties', index=layer_index(call, 'Clipped'), blend_mode='Multiply')
    written = folder / 'clipped16.psd'
    exported = call('export_psd', path=str(written), mode='layers')
    assert exported['bit_depth'] == 16, exported
    assert not any('clip' in loss.lower() for loss in exported['losses']), exported
    decoded = psd.decode(written)
    assert decoded['depth'] == 16, decoded['depth']
    rows = {row['name']: row for row in decoded['layers']}
    assert rows['Clipped']['clipped'] is True, rows['Clipped']
    assert rows['Clip base']['clipped'] is False, rows['Clip base']
    record = reopen_in_gimp(ctx, 'R5', {'clipped16': written})['clipped16']
    assert record['error'] is None, record['error']
    shape = structure(record, decoded, 16)
    assert len(shape['synthesized_groups']) == 1, shape
    folder_row = shape['synthesized_groups'][0]
    assert folder_row['members'] == ['Clip base', 'Clipped'], folder_row
    assert folder_row['clipped'] == [False, True], folder_row
    merged = external.appearance(merged_samples(record, decoded, 16),
                                 list(decoded['composite']), 16)
    reopened = external.appearance(reopened_samples(ctx, 'clipped16', decoded, 16),
                                   list(decoded['composite']), 16, gated=False)
    return dict(export_losses=exported['losses'], structure=shape,
                merged_matches_stored_composite=merged, reopened_vs_stored_composite=reopened)


def receipt_r6(ctx):
    modes = {'Hard Mix': 'hMix', 'Darker Color': 'dkCl',
             'Lighter Color': 'lgCl', 'Dissolve': 'diss'}
    folder = ctx.folder / 'depth16'
    _, call = open_base(ctx, 16, 'depth16')
    base = call('get_document')['layers'][0]['name']
    for mode in modes:
        duplicate(call, base, mode)
        call('apply_filter', filter='invert', layer=layer_index(call, mode))
        call('set_layer_properties', index=layer_index(call, mode), opacity=0.625,
             blend_mode=mode)
    written = folder / 'modes16.psd'
    exported = call('export_psd', path=str(written), mode='layers')
    assert exported['bit_depth'] == 16, exported
    decoded = psd.decode(written)
    rows = {row['name']: row for row in decoded['layers']}
    for mode, key in modes.items():
        assert rows[mode]['blend'] == key, (mode, rows[mode]['blend'])
    record = reopen_in_gimp(ctx, 'R6', {'modes16': written})['modes16']
    assert record['error'] is None, record['error']
    shape = structure(record, decoded, 16)
    merged = external.appearance(merged_samples(record, decoded, 16),
                                 list(decoded['composite']), 16)
    reopened = external.appearance(reopened_samples(ctx, 'modes16', decoded, 16),
                                   list(decoded['composite']), 16, gated=False)
    return dict(keys={mode: rows[mode]['blend'] for mode in modes},
                nicks={row['name']: row['blend'] for row in record['reopened']['layers']},
                structure=shape, merged_matches_stored_composite=merged,
                reopened_vs_stored_composite=reopened)


def receipt_r7(ctx):
    ramp = {'channel': 'gray', 'this_layer': [250, 255, 255, 255],
            'underlying': [0, 0, 255, 255]}
    folder = ctx.folder / 'depth16'
    _, call = open_base(ctx, 16, 'depth16')
    base = call('get_document')['layers'][0]['name']
    duplicate(call, base, 'Blend If')
    carve(call, 'Blend If', (6, 6, 20, 20))
    call('set_layer_style', layer=layer_index(call, 'Blend If'),
         style={'fill_opacity': 1, 'blend_if': ramp, 'effects': []})
    written = folder / 'blendif16.psd'
    exported = call('export_psd', path=str(written), mode='layers')
    assert exported['bit_depth'] == 16, exported
    named = [line for line in exported['observations'] if 'Blend If on gray' in line]
    assert len(named) == 1, exported['observations']
    decoded = psd.decode(written)
    row = next(r for r in decoded['layers'] if r['name'] == 'Blend If')
    ranges = list(row['blend_ranges'])
    # The 40-byte layout of `core/src/psd_layer_effects.rs::encode_ranges`:
    # gray, red, green, blue, alpha, each this-layer then underlying.
    assert len(ranges) == 40, len(ranges)
    assert ranges[0:4] == [250, 255, 255, 255], ranges[0:4]
    assert ranges[4:8] == [0, 0, 255, 255], ranges[4:8]
    assert ranges[8:] == [0, 0, 255, 255] * 8, ranges[8:]
    picture = base16()
    assert list(decoded['composite']) == picture, 'the excluded layer must not reach the preview'
    record = reopen_in_gimp(ctx, 'R7', {'blendif16': written})['blendif16']
    assert record['error'] is None, record['error']
    shape = structure(record, decoded, 16)
    wanted = source_over(row, picture, decoded['canvas'], 16)
    reopened = external.appearance(reopened_samples(ctx, 'blendif16', decoded, 16), wanted, 16)
    merged = external.appearance(merged_samples(record, decoded, 16),
                                 list(decoded['composite']), 16)
    return dict(observation=named[0], blend_ranges=ranges, structure=shape,
                reopened_matches_unrestricted_source_over=reopened,
                merged_matches_stored_composite=merged)


def receipt_r8(ctx):
    folder = ctx.folder / 'depth16'
    _, call = open_base(ctx, 16, 'depth16')
    call('add_adjustment_layer', op='invert', params={}, name='Inverted')
    call('set_layer_properties', index=1, blend_mode='Multiply')
    written = folder / 'adjustment16.psd'
    exported = call('export_psd', path=str(written), mode='layers')
    assert exported['bit_depth'] == 16, exported
    assert exported['losses'] == [], exported
    decoded = psd.decode(written)
    row = decoded['layers'][1]
    assert row['kind'] == 'adjustment', row['kind']
    assert row['adjustment'] == 'nvrt', row['adjustment']
    assert row['blend'] == 'mul ', row['blend']
    record = reopen_in_gimp(ctx, 'R8', {'adjustment16': written})['adjustment16']
    assert record['error'] is None, record['error']
    shape = structure(record, decoded, 16)
    assert len(shape['adjustment_geometry']) == 1, shape
    merged = external.appearance(merged_samples(record, decoded, 16),
                                 list(decoded['composite']), 16)
    reopened = external.appearance(reopened_samples(ctx, 'adjustment16', decoded, 16),
                                   list(decoded['composite']), 16, gated=False)
    return dict(record=dict(kind=row['kind'], adjustment=row['adjustment'], blend=row['blend']),
                export_losses=exported['losses'], structure=shape,
                merged_matches_stored_composite=merged,
                reopened_vs_stored_composite=reopened)


def receipt_r9(ctx):
    modes = {'Hue': 'hue ', 'Saturation': 'sat ', 'Color': 'colr', 'Luminosity': 'lum '}
    folder = ctx.folder / 'depth16'
    _, call = open_base(ctx, 16, 'depth16')
    base = call('get_document')['layers'][0]['name']
    for mode in modes:
        duplicate(call, base, mode)
        call('apply_filter', filter='invert', layer=layer_index(call, mode))
        call('set_layer_properties', index=layer_index(call, mode), opacity=0.625,
             blend_mode=mode)
    written = folder / 'hsl16.psd'
    exported = call('export_psd', path=str(written), mode='layers')
    assert exported['bit_depth'] == 16, exported
    assert exported['losses'] == [], exported['losses']
    decoded = psd.decode(written)
    rows = {row['name']: row for row in decoded['layers']}
    for mode, key in modes.items():
        assert rows[mode]['blend'] == key, (mode, rows[mode]['blend'])
    record = reopen_in_gimp(ctx, 'R9', {'hsl16': written})['hsl16']
    assert record['error'] is None, record['error']
    shape = structure(record, decoded, 16)
    merged = external.appearance(merged_samples(record, decoded, 16),
                                 list(decoded['composite']), 16)
    reopened = external.appearance(reopened_samples(ctx, 'hsl16', decoded, 16),
                                   list(decoded['composite']), 16, gated=False)
    return dict(keys={mode: rows[mode]['blend'] for mode in modes},
                export_losses=exported['losses'],
                nicks={row['name']: row['blend'] for row in record['reopened']['layers']},
                structure=shape, merged_matches_stored_composite=merged,
                reopened_vs_stored_composite=reopened)


RECEIPTS = {'R1': receipt_r1, 'R2': receipt_r2, 'R3': receipt_r3, 'R5': receipt_r5,
            'R6': receipt_r6, 'R7': receipt_r7, 'R8': receipt_r8, 'R9': receipt_r9}


# ------------------------------------------------------------- the runner --

class Context:
    def __init__(self, app, executable, root, gimp, helpers):
        self.app, self.executable, self.root, self.gimp = app, executable, root, gimp
        self.helpers, self.folder = helpers, None


def receipt(ctx, name, body):
    """ONE receipt folder, ONE receipt.json; a failed gate IS the receipt."""
    ctx.folder = ctx.root / name
    ctx.folder.mkdir(exist_ok=False)
    record = dict(receipt=name, app=str(ctx.executable), app_sha256=digest(ctx.executable),
                  gimp_sha256=digest(ctx.gimp) if name in GIMP_RECEIPTS else None,
                  helper_sha256={str(path): digest(path) for path in ctx.helpers},
                  calls=0, checks=None, error=None)
    try:
        record['checks'] = body(ctx)
    except BaseException as error:  # Recorded, never repaired.
        record['error'] = f'{type(error).__name__}: {error}'
    record['calls'] = sum(1 for trace in sorted(ctx.folder.rglob('*.jsonl'))
                          for _ in trace.open())
    (ctx.folder / 'receipt.json').write_text(json.dumps(record, indent=2, default=str) + '\n')
    return record


def dependencies():
    """The code files `helper_sha256` hashes."""
    helpers = [Path(__file__), Path(psd.__file__), Path(external.__file__),
               ROOT / 'scripts/app_under_test.py',
               ROOT / 'scripts/verify_phase1.py', ROOT / 'scripts/verify_phase10a_pixels.py',
               ROOT / 'scripts/live-filter-probe/native.py',
               ROOT / 'scripts/retouch-probe/support.py',
               ROOT / 'scripts/host-source-probe/support.py']
    helpers += sorted((ROOT / 'scripts/psd-exchange-probe').glob('*.py'))
    helpers += [ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/precision-probe/fixtures.py',
                ROOT / 'scripts/precision-probe/retained_fixtures.py',
                ROOT / 'scripts/precision-probe/retained_linked_fixtures.py',
                ROOT / 'scripts/raw-probe/native.py', ROOT / 'scripts/retouch-probe/fixtures.py',
                ROOT / 'scripts/verification_isolation.py', ROOT / 'scripts/verify_phase2.py',
                ROOT / 'scripts/verify_phase8b.py', *sorted((ROOT / 'core/src').rglob('*.rs'))]
    return helpers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--gimp', type=Path,
                        default=Path('/Applications/GIMP.app/Contents/MacOS/gimp-3.2'))
    parser.add_argument('--receipts', default=','.join(RECEIPTS))
    args = parser.parse_args()
    selected = [name for name in args.receipts.split(',') if name]
    assert all(name in RECEIPTS for name in selected), selected
    if not selected:
        parser.error('--receipts selected no receipt')
    output = args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error('output must be outside the repository and not a symlink')
    # The parent MAY pre-exist: R4 creates it. Each selected receipt folder
    # must be fresh, and no sibling is ever read or written.
    output.mkdir(parents=True, exist_ok=True)
    executable = (args.app / 'Contents/MacOS/Doctored'
                  if args.app.suffix == '.app' else args.app).resolve()
    helpers = dependencies()
    summary = {}
    with support.HostApp(output / 'app', output / 'recovery', executable) as app:
        ctx = Context(app, executable, output, args.gimp.resolve(), helpers)
        for name in selected:
            summary[name] = receipt(ctx, name, RECEIPTS[name])
    assert digest(executable) == summary[selected[0]]['app_sha256']
    print(json.dumps({name: dict(error=record['error'], calls=record['calls'],
                                 receipt=str(output / name / 'receipt.json'))
                      for name, record in summary.items()}, indent=2))
    raise SystemExit(1 if any(record['error'] for record in summary.values()) else 0)


if __name__ == '__main__':
    main()
