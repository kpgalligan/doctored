#!/usr/bin/env python3
"""Qualify Doctored's own PSD export by reopening it in GIMP 3.2.6.

The reference direction is the reverse of 10A: GIMP's `file-psd-load` and
`file-psd-load-merged` are the references, and the Doctored-written file is the
measured side. Nothing here derives an expectation from the Doctored importer:
gate 1 compares the corpus manifest (built in `core/tests/psd_export_corpus.rs`
from the source document's own fields) against an independent decode of the
committed bytes, and gates 2-4 compare that decode against the application's
recorded reopening. The gates are fixed in designs/phase-10b-export-status.md.
"""
import argparse
import hashlib
import json
import math
import re
from pathlib import Path
import sys

sys.dont_write_bytecode = True
import psd_qualification_decode as psd
import verify_phase10a_pixels as pixels

ROOT = Path(__file__).resolve().parents[1]
support = pixels.support
CASES = {'layers_basic', 'groups_masks', 'flattened', 'p3_ppi', 'clipping',
         'styled_effects', 'depth16'}
# The four keys this corpus declares, and the layer-mode nick GIMP must report.
BLENDS = {'norm': 'normal', 'mul ': 'multiply', 'scrn': 'screen', 'pass': 'pass-through'}
PRECISION = {8: 'u8-non-linear', 16: 'u16-non-linear'}
MAX_APPEARANCE_ERROR = {8: 2, 16: 514}
# GIMP synthesizes one folder per clipping run on load and uniquifies a
# duplicate name on insert, so the second run's folder arrives as "… #1".
SYNTHESIZED = re.compile(r'^Group added by GIMP( #\d+)?$')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def full_code(depth):
    return 255 if depth == 8 else 65535


# ------------------------------------------------------- gate 1: the bytes --

def clipped_runs(rows):
    """Consecutive clipped runs, each of which GIMP wraps in one folder."""
    runs, previous = 0, {}
    for row in rows:
        depth = row['depth']
        for deeper in [key for key in previous if key > depth]:
            del previous[deeper]
        if row['clipped'] and not previous.get(depth, False):
            runs += 1
        previous[depth] = row['clipped']
    return runs


def composite_agreement(render, composite, depth):
    """The written preview against the projection the export was handed."""
    full = full_code(depth)
    assert len(render) == len(composite) and len(render) % 4 == 0
    worst, endpoints = 0, 0
    for i in range(0, len(render), 4):
        alpha = render[i + 3]
        assert alpha == composite[i + 3], ('preview alpha must be exact', i)
        limit = 0 if alpha in (0, full) else math.ceil((full / 2 + 0.5) / alpha)
        endpoints += int(alpha in (0, full))
        for c in range(3):
            error = abs(render[i + c] - composite[i + c])
            assert error <= limit, (i, c, error, limit, alpha)
            worst = max(worst, error)
    return dict(max_rgb_error=worst, endpoint_alpha_pixels=endpoints,
                minimum_alpha=min(render[3::4]))


def gate_bytes(manifest, decoded):
    """Gate 1 — the committed PSD says exactly what the source document held."""
    depth = manifest['depth']
    assert decoded['depth'] == depth and decoded['color'] == 3
    assert decoded['canvas'] == manifest['canvas']
    assert decoded['resolution'] == manifest['resolution']
    assert hashlib.sha256(decoded['profile']).hexdigest() == manifest['profile_sha256']
    canvas = manifest['canvas']
    baked = []
    for row, want in zip(decoded['layers'], manifest['layers'], strict=True):
        where = (manifest['name'], want['name'])
        for key in ('name', 'kind', 'depth', 'visible', 'clipped'):
            assert row[key] == want[key], (where, key, row[key], want[key])
        assert round(row['opacity'] * 255) == want['opacity_byte'], where
        assert row['blend'] == want['blend'], (where, row['blend'])
        # `open` is meaningless on a raster record; the manifest writes True there.
        if row['kind'] == 'group':
            assert row['open'] == want['open'], where
        if want['bake']:
            # The enabled mask is inside the bake, so no mask channel is written.
            assert row['mask'] is None, where
            (ox, oy), (w, h) = want['offset'], want['size']
            (dx, dy), (dw, dh) = row['offset'], row['size']
            assert dx <= ox and dy <= oy and dx + dw >= ox + w and dy + dh >= oy + h, where
            assert dx >= 0 and dy >= 0 and dx + dw <= canvas[0] and dy + dh <= canvas[1], where
            baked.append(dict(name=want['name'], source=[want['offset'], want['size']],
                              written=[row['offset'], row['size']]))
            continue
        assert row['offset'] == want['offset'] and row['size'] == want['size'], where
        if row['kind'] == 'group':
            assert want['rgba'] is None and 'rgba' not in row, where
        else:
            assert list(row['rgba']) == want['rgba'], (where, 'straight layer samples')
        if want['mask'] is None:
            assert row['mask'] is None, where
        else:
            assert row['mask'] is not None, where
            for key in ('size', 'offset', 'outside_coverage', 'enabled'):
                assert row['mask'][key] == want['mask'][key], (where, 'mask', key)
            assert list(row['mask']['coverage']) == want['mask']['coverage'], (where, 'mask')
    return dict(layers=len(manifest['layers']), baked=baked,
                clipped_runs=clipped_runs(decoded['layers']),
                preview=composite_agreement(manifest['render'], list(decoded['composite']), depth))


# ---------------------------------------------- gate 2: GIMP's layer tree --

def strip_synthesized(rows):
    """Remove GIMP's per-clipping-run folders and unnest their contents."""
    out, removed = [dict(row) for row in rows], []
    while True:
        index = next((i for i, row in enumerate(out)
                      if row['kind'] == 'group' and SYNTHESIZED.match(row['name'])), None)
        if index is None:
            return out, removed
        group = out[index]
        start = index
        while start > 0 and out[start - 1]['depth'] > group['depth']:
            start -= 1
        children = out[start:index]
        for child in children:
            child['depth'] -= 1
        direct = [c for c in children if c['depth'] == group['depth']]
        removed.append(dict(name=group['name'], size=group['size'], offset=group['offset'],
                            members=[c['name'] for c in direct],
                            clipped=[c['composite_mode'] == 'clip-to-backdrop' for c in direct]))
        del out[index]


def mask_expectation(stored, rect, depth):
    """The layer-sized plane GIMP must hold: the outside default with the
    stored rectangle pasted over its intersection with the layer."""
    (lx, ly), (lw, lh) = rect
    fill = stored['outside_coverage'] * (1 if depth == 8 else 257)
    plane = [fill] * (lw * lh)
    (mx, my), (mw, mh) = stored['offset'], stored['size']
    samples = list(stored['coverage'])
    covered = 0
    for y in range(max(ly, my), min(ly + lh, my + mh)):
        for x in range(max(lx, mx), min(lx + lw, mx + mw)):
            plane[(y - ly) * lw + (x - lx)] = samples[(y - my) * mw + (x - mx)]
            covered += 1
    return plane, covered


def gate_structure(record, decoded, depth, blends=BLENDS):
    """Gate 2 — what the independent application read out of those bytes."""
    observed = record['reopened']
    assert observed['canvas'] == decoded['canvas']
    assert observed['resolution'] == decoded['resolution']
    assert observed['color_mode'] == 'rgb'
    assert observed['precision'] == PRECISION[depth]
    assert observed['profile_sha256'] == hashlib.sha256(decoded['profile']).hexdigest()
    assert not observed['saved_channels']
    rows, removed = strip_synthesized(observed['layers'])
    runs = clipped_runs(decoded['layers'])
    assert len(removed) == runs, ('synthesized folders must equal the clipping runs',
                                  len(removed), runs)
    for folder in removed:
        assert folder['clipped'] and folder['clipped'][0] is False, folder
        assert all(folder['clipped'][1:]), folder
        assert len(folder['members']) >= 2, folder
    groups, adjustment_geometry = [], []
    for row, raw in zip(rows, decoded['layers'], strict=True):
        where = (record['name'], raw['name'])
        if raw['kind'] == 'adjustment':
            # A pixel-less adjustment record carries no rectangle, planes or
            # samples; the reading application makes an empty raster of its own
            # size for it, so only the package fields can be compared and its
            # geometry is recorded.
            for key in ('name', 'depth', 'visible'):
                assert row[key] == raw[key], (where, key, row[key], raw[key])
            assert row['kind'] == 'raster', (where, row['kind'])
            assert round(row['opacity'] * 255) == round(raw['opacity'] * 255), where
            assert row['blend'] == blends[raw['blend']], (where, row['blend'], raw['blend'])
            assert row['composite_mode'] in (
                ('clip-to-backdrop',) if raw['clipped'] else ('union', 'auto')), (
                    where, row['composite_mode'])
            adjustment_geometry.append(
                dict(name=raw['name'], size=row['size'], offset=row['offset']))
            continue
        for key in ('name', 'kind', 'depth', 'visible'):
            assert row[key] == raw[key], (where, key, row[key], raw[key])
        assert round(row['opacity'] * 255) == round(raw['opacity'] * 255), where
        assert row['blend'] == blends[raw['blend']], (where, row['blend'], raw['blend'])
        if raw['clipped']:
            assert row['composite_mode'] == 'clip-to-backdrop', (where, row['composite_mode'])
        else:
            assert row['composite_mode'] in ('union', 'auto'), (where, row['composite_mode'])
        if raw['kind'] == 'group':
            assert row['open'] == raw['open'], where
            groups.append(dict(name=raw['name'], size=row['size'], offset=row['offset']))
        else:
            assert row['offset'] == raw['offset'] and row['size'] == raw['size'], where
            assert list(row['rgba']) == list(raw['rgba']), (where, 'reopened layer samples')
        if raw['mask'] is None:
            assert row['mask'] is None, where
            continue
        assert row['mask'] is not None, where
        rect = (row['offset'], row['size'])
        plane, covered = mask_expectation(raw['mask'], rect, depth)
        assert covered > 0, (where, 'this corpus declares no mask outside its layer')
        assert row['mask']['size'] == row['size'] and row['mask']['offset'] == row['offset'], where
        assert list(row['mask']['coverage']) == plane, (where, 'reopened mask coverage')
        assert row['mask']['enabled'] == raw['mask']['enabled'], where
    return dict(rows=len(rows), synthesized_groups=removed, group_geometry=groups,
                adjustment_geometry=adjustment_geometry)


# ---------------------------------------------------- gates 3/4: pictures --

def reference(path, canvas, depth, profile):
    decoded = support.png_samples(path.read_bytes())
    assert (decoded['width'], decoded['height'], decoded['depth']) == (*canvas, depth), path
    assert decoded['profile'] == profile, (path, 'the render must carry the source ICC')
    assert len(decoded['samples']) == canvas[0] * canvas[1] * 4
    return list(decoded['samples'])


def appearance(actual, wanted, depth, gated=True):
    assert len(actual) == len(wanted) and len(actual) % 4 == 0
    full, limit = full_code(depth), MAX_APPEARANCE_ERROR[depth]
    raw_errors = [abs(a - b) for a, b in zip(actual, wanted)]
    errors, transparent = list(raw_errors), 0
    for index in range(0, len(actual), 4):
        if actual[index + 3] == wanted[index + 3] == 0:
            # RGB under joint zero alpha contributes no appearance anywhere.
            errors[index:index + 3] = [0, 0, 0]
            transparent += 1
    maximum = [max(errors[c::4]) for c in range(4)]
    result = dict(max_rgba=maximum, mean_rgba=[sum(errors[c::4]) / len(errors[c::4]) for c in range(4)],
                  threshold=limit, unnormalized_max_rgba=[max(raw_errors[c::4]) for c in range(4)],
                  joint_zero_alpha_pixels=transparent, gated=gated)
    if not gated:
        # Recorded, never widened: clipping semantics and a written style's
        # effects are outside this phase's appearance claim.
        return result
    assert max(maximum) <= limit, result
    for a, b in zip(actual[3::4], wanted[3::4]):
        assert b != full or a == full, 'alpha must stay at maximum where the projection is opaque'
    return result


def backdrop_appearance(actual, wanted, depth):
    full, limit = full_code(depth), MAX_APPEARANCE_ERROR[depth]
    assert len(actual) == len(wanted)
    result = {}
    for name, backdrop in [('black', 0), ('white', full)]:
        errors = [[], [], []]
        for i in range(0, len(actual), 4):
            for c in range(3):
                a = (actual[i + c] * actual[i + 3] + backdrop * (full - actual[i + 3])) / full
                b = (wanted[i + c] * wanted[i + 3] + backdrop * (full - wanted[i + 3])) / full
                errors[c].append(abs(a - b))
        maximum = [max(channel) for channel in errors]
        assert max(maximum) <= limit, (name, maximum)
        result[name] = dict(max_rgb=maximum, mean_rgb=[sum(e) / len(e) for e in errors],
                            threshold=limit)
    return result


# ------------------------------------------------------------- the corpus --

def audit_case(folder):
    manifest = json.loads((folder / 'manifest.json').read_text())
    record = json.loads((folder / 'gimp.json').read_text())
    assert manifest['schema'] == 1 and manifest['name'] == folder.name
    assert record['schema'] == 1 and record['name'] == folder.name
    assert record['application'] == {'name': 'GIMP', 'version': '3.2.6'}
    assert record['error'] is None, (folder.name, record['error'])
    assert record['source_psd_sha256'] == digest(folder / 'source.psd')
    assert set(record['files']) == {'reopened.png', 'reopened.icc', 'merged.png', 'merged.icc'}
    for name, sha in record['files'].items():
        assert digest(folder / name) == sha, (folder.name, name)
    decoded = psd.decode(folder / 'source.psd')
    depth = manifest['depth']
    result = dict(depth=depth, mode=manifest['mode'],
                  appearance_qualified=manifest['appearance_qualified'],
                  bake=manifest['bake'], export_report=manifest['export_report'])
    result['manifest_matches_decode'] = gate_bytes(manifest, decoded)
    result['reopened_matches_decode'] = gate_structure(record, decoded, depth)
    # What the reading application attached from each record's own `lfx2`:
    # recorded as its report, never an expectation of this phase.
    result['reopened_filters'] = {row['name']: row.get('filters', [])
                                  for row in record['reopened']['layers']}
    profile = decoded['profile']
    assert (folder / 'reopened.icc').read_bytes() == profile, 'no timestamp exception applies here'
    assert (folder / 'merged.icc').read_bytes() == profile
    reopened = reference(folder / 'reopened.png', decoded['canvas'], depth, profile)
    render = manifest['render']
    result['reopened_appearance'] = appearance(reopened, render, depth,
                                               manifest['appearance_qualified'])
    result['appearance_gated'] = manifest['appearance_qualified']
    merged = record['merged']
    assert merged['precision'] == PRECISION[depth] and merged['canvas'] == decoded['canvas']
    assert len(merged['layers']) == 1 and merged['layers'][0]['mask'] is None
    merged_samples = merged['layers'][0]['rgba']
    assert merged_samples == reference(folder / 'merged.png', decoded['canvas'], depth, profile)
    result['merged_matches_stored_composite'] = appearance(merged_samples, list(decoded['composite']), depth)
    if manifest['appearance_qualified']:
        result['merged_backdrops'] = backdrop_appearance(merged_samples, reopened, depth)
    else:
        result['merged_backdrops'] = None
    # Every gate asserts as it runs, so reaching this point IS the receipt.
    result['gates'] = dict(bytes_match_manifest=True, reopen_matches_decode=True,
                           reopened_appearance=True, merged_matches_composite=True,
                           application_round_trip=False)
    return manifest, decoded, reopened, result


def audit_corpus(cases):
    """Pin the claimed coverage, so keeping filenames cannot hide a reduction."""
    audited = {folder.name: audit_case(folder) for folder in cases}
    assert {name for name in audited} == CASES
    kinds = {name: values[0] for name, values in audited.items()}
    assert kinds['depth16']['depth'] == 16 and all(
        kinds[name]['depth'] == 8 for name in CASES - {'depth16'})
    assert kinds['flattened']['mode'] == 'flattened'
    assert all(kinds[name]['mode'] == 'layers' for name in CASES - {'flattened'})
    assert not any(kinds[name]['bake'] for name in CASES)
    assert {name for name in CASES if not kinds[name]['appearance_qualified']} == {
        'clipping', 'styled_effects'}
    assert audited['clipping'][3]['manifest_matches_decode']['clipped_runs'] == 2
    assert all(audited[name][3]['manifest_matches_decode']['clipped_runs'] == 0
               for name in CASES - {'clipping'})
    basic = audited['layers_basic'][1]['layers']
    assert set(basic[3]['rgba'][3::4]) == {0, 1, 64, 127, 128, 200, 255}
    assert any(any(basic[3]['rgba'][i:i + 3]) for i in range(0, len(basic[3]['rgba']), 4)
               if basic[3]['rgba'][i + 3] == 0), 'hidden RGB under alpha 0'
    assert basic[2]['visible'] is False and basic[2]['offset'] == [9, -3]
    groups = audited['groups_masks'][1]['layers']
    assert [row['depth'] for row in groups] == [0, 1, 2, 1, 0, 1, 1, 0, 0]
    assert {row['mask']['outside_coverage'] for row in groups if row['mask']} == {0, 255}
    assert audited['p3_ppi'][1]['profile'] != audited['layers_basic'][1]['profile']
    return {name: values[3] for name, values in audited.items()}


# ------------------------------------------------------ gate 5: the app --

def native_decode(path):
    decoded = psd.decode(path)
    # `resources` holds the raw image-resource bodies the two files must share.
    return decoded


def verify_case(app, folder, output, manifest, decoded, reopened):
    output.mkdir()
    source = folder / 'source.psd'
    depth = manifest['depth']
    preflight = app.call('inspect_psd', path=str(source))
    assert preflight['layered_available'] and preflight['stored_composite_available'], preflight
    if depth == 16:
        assert preflight['import_precision'] == 'rgba16', preflight
    opened = app.call('open_document', path=str(source), psd_mode='layers',
                      psd_fingerprint=preflight['source_fingerprint'])
    assert opened['psd_import']['chosen_mode'] == 'layers'
    imported = opened['psd_import']
    call = support.call_for(app, opened['document']['id'], output / 'calls.jsonl')
    export_preflight = call('inspect_psd_export', mode=manifest['mode'])
    assert export_preflight['available'] is True, export_preflight
    assert export_preflight['depth'] == depth, export_preflight
    written = output / 're-export.psd'
    exported = call('export_psd', path=str(written), mode=manifest['mode'])
    assert exported['bit_depth'] == depth, exported
    again = native_decode(written)
    identical = written.read_bytes() == source.read_bytes()
    excluded = {}
    if folder.name == 'styled_effects':
        # The committed preview is the LIVE styled projection; the re-export's
        # is re-derived from the carried style.
        full = full_code(depth)
        for key in ('composite', 'composite_encoded'):
            errors = [abs(a - b) for a, b in zip(again[key], decoded[key])]
            excluded[key] = dict(max_abs_error=max(errors), full_code=full)
            again[key] = decoded[key]
        excluded['observation'] = ('layer style written as lfx2; reopened composite '
                                   're-derived from the carried style')
        assert not any('rasterize' in loss.lower() for loss in exported['losses']), exported
        assert not any('style' in loss.lower() for loss in imported.get('verified_losses', [])), imported
    assert again == decoded, 're-export must decode identically to the committed source'
    result = dict(preflight=preflight, export_preflight=export_preflight,
                  re_export=dict(bytes_identical=identical, losses=exported['losses'],
                                 excluded=excluded or None),
                  psd_import=imported)
    if depth == 8:
        call('save_copy', path=str(output / 'projection.png'))
        samples = reference(output / 'projection.png', decoded['canvas'], 8, decoded['profile'])
        comparison = dict(threshold=MAX_APPEARANCE_ERROR[8],
                          max_rgba=[max(abs(a - b) for a, b in zip(samples[c::4], reopened[c::4]))
                                    for c in range(4)])
        if folder.name not in ('clipping', 'styled_effects'):
            assert max(comparison['max_rgba']) <= MAX_APPEARANCE_ERROR[8], comparison
            comparison['gated'] = True
        else:
            # Clipping semantics are a 10D boundary, and GIMP attaches no
            # filter from a written `lfx2`, so its projection is the bare layer
            # stack, while `save_copy` renders the whole style. Recorded, never
            # gated.
            comparison['gated'] = False
        result['projection_vs_reopened'] = comparison
    else:
        result['projection_vs_reopened'] = None
    return result


def dependencies():
    """The code files `helper_sha256` hashes."""
    helpers = [Path(__file__), Path(psd.__file__), Path(pixels.__file__),
               ROOT / 'scripts/live-filter-probe/native.py', ROOT / 'scripts/raw-probe/native.py',
               ROOT / 'scripts/retouch-probe/support.py', ROOT / 'scripts/host-source-probe/support.py']
    helpers += sorted((ROOT / 'scripts/psd-exchange-probe').glob('*.py'))
    helpers += [ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/precision-probe/fixtures.py',
                ROOT / 'scripts/precision-probe/retained_fixtures.py',
                ROOT / 'scripts/precision-probe/retained_linked_fixtures.py',
                ROOT / 'scripts/retouch-probe/fixtures.py', ROOT / 'scripts/verification_isolation.py',
                ROOT / 'scripts/app_under_test.py',
                ROOT / 'scripts/verify_phase1.py', ROOT / 'scripts/verify_phase2.py',
                ROOT / 'scripts/verify_phase8b.py', *sorted((ROOT / 'core/src').rglob('*.rs'))]
    return helpers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path)
    parser.add_argument('--fixtures', type=Path, default=ROOT / 'core/tests/fixtures/psd_10b')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--oracle-only', action='store_true')
    args = parser.parse_args()
    cases = sorted(path.parent for path in args.fixtures.glob('*/manifest.json'))
    assert {path.name for path in cases} == CASES, 'Missing or extra corpus cases'
    hashes = {str(path): digest(path) for folder in cases
              for path in sorted(folder.iterdir()) if path.is_file()}
    audits = audit_corpus(cases)
    if args.oracle_only:
        print(json.dumps(dict(cases=audits, fixture_hashes=hashes), indent=2))
        return
    if args.app is None or args.output is None:
        parser.error('--app and --output are required for production verification')
    output = args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error('output must be new and outside the repository')
    output.mkdir(exist_ok=False)
    executable = (args.app / 'Contents/MacOS/Doctored' if args.app.suffix == '.app' else args.app).resolve()
    helpers = dependencies()
    result = dict(app=str(executable), app_sha256=digest(executable), fixture_sha256=hashes,
                  helper_sha256={str(path): digest(path) for path in helpers}, cases={})
    with support.HostApp(output / 'app', output / 'recovery', executable) as app:
        for folder in cases:
            manifest, decoded, reopened, audit = audit_case(folder)
            audit['application'] = verify_case(app, folder, output / folder.name,
                                               manifest, decoded, reopened)
            audit['gates']['application_round_trip'] = True
            result['cases'][folder.name] = audit
    assert all(digest(Path(path)) == sha for path, sha in hashes.items())
    assert all(digest(Path(path)) == sha for path, sha in result['helper_sha256'].items())
    assert digest(executable) == result['app_sha256']
    result['fixtures_unchanged'] = True
    (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(dict(output=str(output), cases=len(cases)), indent=2))


if __name__ == '__main__':
    main()
