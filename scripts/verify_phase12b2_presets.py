#!/usr/bin/env python3
"""The Phase 12B-2 RAW preset library MCP drive, on a built Release app.

Nine gates, FIXED before the run; a failed gate IS the receipt and no
tolerance is widened afterwards. Every instance is owned by this file (a free
port, a dummy API key, private RAW/filter/export preset and metadata-policy
directories) and is terminated by its own context manager; no Doctored process
this file did not start is ever signalled.

- **P1 listing.** A folder holding two GOOD presets (a schema-1 "Sunny" at its
  canonical digest name and a schema-2 "Second" deliberately parked at
  "Collide"'s digest name) plus three bad files — malformed JSON, an 8193-byte
  file and a `schema_version: 99` file — lists BOTH presets with their on-disk
  `schema_version` and `file`, and reports the three bad files as `broken` rows
  carrying `not_json`, `oversize` and `unknown_version` with `repairable:
  false`. The whole library is no longer refused for one bad file, and
  `apply_raw_preset` of the v1 entry still works.
- **P2 removal and repair.** The advertised `required` lists (`[]` for
  `delete_raw_preset`, `["file"]` for `repair_raw_preset`); `delete_raw_preset
  {file}` clears the oversize file; `repair_raw_preset` refuses the
  unknown-version and malformed files NAMING their code and leaves their bytes
  byte-for-byte unchanged; a repairable file (schema 1 carrying `moire`, an
  unknown `decoder` key, an out-of-range `exposure` and an untrimmed name)
  lists as `invalid_schema, repairable: true` and repairs to the canonical
  digest name as schema 4 with `dropped == ["decoder", "exposure"]`; a file
  whose untrimmed name collides with a listed preset lists as `repairable:
  false` and is refused naming `duplicate_name`; and a `file` with a `/`, a
  `..` or a non-`.json` suffix, both spellings together, and neither spelling,
  are each refused.
- **P3 migration on save.** Replacing the schema-1 entry rewrites ITS OWN file
  with `"schema_version":4`, the identical `raw` keys and values, and the
  listing row then reads 4.
- **P4 a bad file blocks nothing.** With the unknown-version file still there,
  `redevelop_raw`, `apply_raw_settings`, `sync_raw_settings` and a new
  `save_raw_preset` all succeed. A save whose name digests to the broken file's
  basename is refused NAMING that file, and so is a save whose name digests to
  a valid other-named entry's file — whose bytes are unchanged — while
  `replace: true` on that entry's own name still writes in place.
- **P5 resolved Actions are immune.** A recorded `apply_raw_preset` becomes ONE
  `apply_raw_settings` step; the preset is then REPLACED, RENAMED and DELETED,
  and at each of the three stages a freshly opened copy of the same DNG exports
  the identical PNG digest.
- **P6 the clipped fold.** At 16 bits, with one entry already clipped, a set
  clip applies the entry that changes, reports the other in `skipped`, and ONE
  undo restores; a set whose entries ALL refuse is still refused naming both.
- **P7 the lock no-op echo.** A repeated single-entry `set_layer_lock` answers
  `changed: false` and echoes the `layer` it was given.
- **P8 the write bound counts files.** A folder holding exactly 128 `.json`
  files, one of them a bad file, refuses a new preset by name as full and
  writes nothing — the library still LISTS rather than tipping over — and
  `delete_raw_preset {file}` on the bad file frees the slot, after which the
  same save succeeds.
- **P9 repair only repairs.** `repair_raw_preset` on a VALID entry's file is
  refused and leaves its bytes unchanged: repair is not a rename tool.

Nothing here derives an expectation from Doctored: the preset files are
written by this file, the digest file names are computed here from `hashlib`,
the DNG comes from the Phase 3 fixture writer and the digests from `hashlib`.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

import phase5_support
from phase5_support import App, fixture
from phase3_fixtures import synthetic_dng

# ---- the tolerances, fixed here BEFORE anything is measured ----------------
# There are none to spend. Every claim below is EXACT: a byte digest, a file
# name, a code string, an integer index, a parsed JSON object. A preset library
# that keeps "almost" the right controls has lost data.
EXACT = 0

MALFORMED = 'zz-malformed.json'
OVERSIZE = 'zz-oversize.json'
CLOUDY_SOURCE = 'zz-cloudy.json'
SUNNY_COPY = 'zz-sunny-copy.json'


def digest_name(name):
    """The library's canonical file name for `name`.

    `RawPresetLibrary.url` is sha256 of the en_US_POSIX case-fold; every name
    this drive uses is ASCII, where that fold IS `str.lower`.
    """
    assert name.isascii(), name
    return hashlib.sha256(name.lower().encode()).hexdigest() + '.json'


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def export_digest(app, document, path):
    app.call('export_image', document_id=document, path=str(path),
             settings={'format': 'png', 'profile': 'document'}, overwrite=True)
    return digest(path)


def seed(presets):
    """The starting library: two good presets and three bad files."""
    presets.mkdir(parents=True, exist_ok=True)
    (presets / digest_name('Sunny')).write_text(json.dumps(
        {'schema_version': 1, 'name': 'Sunny', 'raw': {'exposure': 0.5}}))
    # Parked at ANOTHER name's digest deliberately: P4 asserts that saving
    # "Collide" is refused rather than overwriting this file.
    (presets / digest_name('Collide')).write_text(json.dumps(
        {'schema_version': 2, 'name': 'Second', 'raw': {'exposure': -0.25}}))
    (presets / MALFORMED).write_text('{ this is not json')
    padding = ' ' * 64
    body = json.dumps({'schema_version': 2, 'name': 'Big' + padding, 'raw': {'exposure': 0.5}})
    (presets / OVERSIZE).write_bytes(body.encode() + b' ' * (8193 - len(body.encode())))
    assert (presets / OVERSIZE).stat().st_size == 8193
    # Named so that P4's `save_raw_preset {name: "Future"}` collides with it.
    (presets / digest_name('Future')).write_text(json.dumps(
        {'schema_version': 99, 'name': 'Future', 'raw': {'exposure': 0.5}}))


# ------------------------------------------------------------------ gates --

def gate_p1(app, presets, files, receipts):
    gate = 'p1_listing'
    listing = app.call('list_raw_presets')
    rows = {row['name']: row for row in listing['presets']}
    assert set(rows) == {'Sunny', 'Second'}, listing
    assert rows['Sunny']['schema_version'] == 1, rows['Sunny']
    assert rows['Sunny']['file'] == digest_name('Sunny'), rows['Sunny']
    assert rows['Sunny']['raw'] == {'exposure': 0.5}, rows['Sunny']
    assert rows['Second']['schema_version'] == 2, rows['Second']
    assert rows['Second']['file'] == digest_name('Collide'), rows['Second']
    codes = {row['file']: row['code'] for row in listing['broken']}
    assert codes == {MALFORMED: 'not_json', OVERSIZE: 'oversize',
                     digest_name('Future'): 'unknown_version'}, listing['broken']
    assert all(row['repairable'] is False for row in listing['broken']), listing['broken']
    assert all(row['reason'] for row in listing['broken']), listing['broken']
    assert listing['maximum_presets'] == 128 and listing['maximum_file_bytes'] == 8192, listing

    document = app.call('open_document', path=str(files / 'source.dng'))['document']['id']
    applied = app.call('apply_raw_preset', document_id=document, name='Sunny', layer=0)
    assert applied['ok'] and applied['result']['changed_documents'] == 1, applied
    receipts[gate] = {'presets': listing['presets'], 'broken': listing['broken'],
                      'applied': applied['result']}
    print('PASS p1: two good presets list beside three named bad files', flush=True)
    return document


def gate_p2(app, presets, receipts):
    gate = 'p2_removal_and_repair'
    findings = {}
    catalog = {tool['name']: tool for tool in app.client.rpc('tools/list', {})['tools']}
    assert catalog['delete_raw_preset']['inputSchema']['required'] == [], catalog['delete_raw_preset']
    assert catalog['repair_raw_preset']['inputSchema']['required'] == ['file'], catalog['repair_raw_preset']
    findings['required'] = {'delete_raw_preset': catalog['delete_raw_preset']['inputSchema']['required'],
                            'repair_raw_preset': catalog['repair_raw_preset']['inputSchema']['required']}

    removed = app.call('delete_raw_preset', file=OVERSIZE)
    assert removed['ok'] and removed['file'] == OVERSIZE, removed
    assert not (presets / OVERSIZE).exists()
    findings['deleted_file'] = removed['file']

    # A refused repair must not touch a byte of the file it refused.
    refusals = {}
    for name, code in [(digest_name('Future'), 'unknown_version'), (MALFORMED, 'not_json')]:
        before = (presets / name).read_bytes()
        text = app.call('repair_raw_preset', file=name, fail=True)
        assert code in text, (name, text)
        assert (presets / name).read_bytes() == before, name
        refusals[code] = text
    findings['refused_by_code'] = refusals

    # The repairable file: an untrimmed name, a v1 file carrying a v2 control,
    # one unknown key and one out-of-range value.
    (presets / CLOUDY_SOURCE).write_text(json.dumps(
        {'schema_version': 1, 'name': ' Cloudy ',
         'raw': {'moire': 0.5, 'decoder': 'zzz', 'exposure': 99}}))
    # …and a file whose trimmed name is already listed, which no repair can fix.
    (presets / SUNNY_COPY).write_text(json.dumps(
        {'schema_version': 1, 'name': ' Sunny ', 'raw': {'exposure': 0.25}}))
    broken = {row['file']: row for row in app.call('list_raw_presets')['broken']}
    assert broken[CLOUDY_SOURCE]['code'] == 'invalid_schema', broken[CLOUDY_SOURCE]
    assert broken[CLOUDY_SOURCE]['repairable'] is True, broken[CLOUDY_SOURCE]
    assert broken[SUNNY_COPY]['code'] == 'invalid_schema', broken[SUNNY_COPY]
    assert broken[SUNNY_COPY]['repairable'] is False, broken[SUNNY_COPY]
    findings['broken_rows'] = [broken[CLOUDY_SOURCE], broken[SUNNY_COPY]]

    repaired = app.call('repair_raw_preset', file=CLOUDY_SOURCE)
    assert repaired['ok'], repaired
    assert repaired['dropped'] == ['decoder', 'exposure'], repaired
    assert repaired['file'] == digest_name('Cloudy'), repaired
    assert repaired['preset']['name'] == 'Cloudy', repaired
    assert repaired['preset']['schema_version'] == 4, repaired
    assert repaired['preset']['raw'] == {'moire': 0.5}, repaired
    assert not (presets / CLOUDY_SOURCE).exists()
    written = json.loads((presets / digest_name('Cloudy')).read_text())
    assert written == {'schema_version': 4, 'name': 'Cloudy', 'raw': {'moire': 0.5}}, written
    findings['repaired'] = {'reply': repaired, 'file': written}

    collision = app.call('repair_raw_preset', file=SUNNY_COPY, fail=True)
    assert 'duplicate_name' in collision, collision
    assert (presets / SUNNY_COPY).exists()
    findings['duplicate_refusal'] = collision

    names = {}
    for bad in ['sub/other.json', '..', 'plain.txt']:
        names[bad] = [app.call('repair_raw_preset', file=bad, fail=True),
                      app.call('delete_raw_preset', file=bad, fail=True)]
    findings['bad_file_names'] = names
    both = app.call('delete_raw_preset', name='Sunny', file=digest_name('Sunny'), fail=True)
    neither = app.call('delete_raw_preset', fail=True)
    assert 'exactly one' in both and 'exactly one' in neither, (both, neither)
    assert (presets / digest_name('Sunny')).exists()
    findings['exclusivity'] = [both, neither]
    receipts[gate] = findings
    print('PASS p2: bad files are removed or repaired by name, and bad names refused', flush=True)


def gate_p3(app, presets, receipts):
    gate = 'p3_migration_on_save'
    path = presets / digest_name('Sunny')
    before = json.loads(path.read_text())
    assert before['schema_version'] == 1, before
    saved = app.call('save_raw_preset', name='Sunny', raw=before['raw'], replace=True)
    assert saved['ok'], saved
    text = path.read_text()
    after = json.loads(text)
    assert after == {'schema_version': 4, 'name': 'Sunny', 'raw': before['raw']}, after
    assert '"schema_version":4' in text.replace(' ', ''), text
    row = next(r for r in app.call('list_raw_presets')['presets'] if r['name'] == 'Sunny')
    assert row['schema_version'] == 4 and row['file'] == digest_name('Sunny'), row
    receipts[gate] = {'before': before, 'after': after, 'row': row}
    print('PASS p3: a replaced schema-1 preset is rewritten as schema 4 in place', flush=True)


def gate_p4(app, presets, files, document, receipts):
    gate = 'p4_bad_file_blocks_nothing'
    findings = {}
    assert (presets / digest_name('Future')).exists()
    develop = app.call('redevelop_raw', document_id=document, layer=0, raw={'exposure': 0.75})
    assert develop['ok'], develop
    settings = app.call('apply_raw_settings', document_id=document, layer=0, raw={'exposure': 0.6})
    assert settings['ok'], settings
    second = app.call('open_document', path=str(files / 'sync-target.dng'))['document']['id']
    synced = app.call('sync_raw_settings', document_id=document, layer=0, fields=['exposure'],
                      targets=[{'document_id': second, 'layer': 0}])
    assert synced['ok'], synced
    fresh = app.call('save_raw_preset', name='Fresh', raw={'exposure': 0.1})
    assert fresh['ok'], fresh
    findings['worked'] = ['redevelop_raw', 'apply_raw_settings', 'sync_raw_settings', 'save_raw_preset']

    refused = app.call('save_raw_preset', name='Future', raw={'exposure': 0.2}, fail=True)
    assert digest_name('Future') in refused, refused
    findings['broken_slot_refusal'] = refused

    before = (presets / digest_name('Collide')).read_bytes()
    occupied = app.call('save_raw_preset', name='Collide', raw={'exposure': 0.3}, fail=True)
    assert digest_name('Collide') in occupied, occupied
    assert (presets / digest_name('Collide')).read_bytes() == before, 'Second was overwritten'
    findings['occupied_slot_refusal'] = occupied

    replaced = app.call('save_raw_preset', name='Second', raw={'exposure': -0.5}, replace=True)
    assert replaced['ok'], replaced
    row = next(r for r in app.call('list_raw_presets')['presets'] if r['name'] == 'Second')
    assert row['file'] == digest_name('Collide') and row['raw'] == {'exposure': -0.5}, row
    findings['replaced_in_place'] = row
    receipts[gate] = findings
    print('PASS p4: an unreadable file blocks no RAW work, and no slot is overwritten', flush=True)


def gate_p5(app, files, folder, receipts):
    gate = 'p5_resolved_action_immunity'
    findings = {}
    app.call('save_raw_preset', name='Replay', raw={'exposure': 1.0})
    target = app.call('open_document', path=str(files / 'replay-0.dng'))['document']['id']
    app.call('start_recording')
    applied = app.call('apply_raw_preset', document_id=target, name='Replay', layer=0)
    assert applied['result']['changed_documents'] == 1, applied
    action = app.call('stop_recording', name='Replay action')
    assert len(action['steps']) == 1, action
    assert action['steps'][0]['tool'] == 'apply_raw_settings', action
    findings['step'] = action['steps'][0]
    d1 = export_digest(app, target, folder / 'replay-authored.png')
    findings['digest'] = d1

    stages = []
    stages.append(('replaced', lambda: app.call(
        'save_raw_preset', name='Replay', raw={'exposure': -1.5}, replace=True)))
    def rename():
        app.call('save_raw_preset', name='Replay Renamed', raw={'exposure': -1.5})
        app.call('delete_raw_preset', name='Replay')
    stages.append(('renamed', rename))
    stages.append(('deleted', lambda: app.call('delete_raw_preset', name='Replay Renamed')))
    for index, (label, change) in enumerate(stages, start=1):
        change()
        document = app.call('open_document', path=str(files / f'replay-{index}.dng'))['document']['id']
        played = app.call('run_action', document_id=document, name='Replay action')
        assert played['ok'], played
        again = export_digest(app, document, folder / f'replay-{label}.png')
        assert again == d1, (label, again, d1)
        findings[label] = again
    receipts[gate] = findings
    print('PASS p5: the resolved step replays identically after edit, rename and delete', flush=True)


def gate_p6(app, folder, receipts):
    gate = 'p6_clipped_fold'
    path = folder / 'clip.png'
    fixture(path)
    document = app.client.open(path)
    app.call('convert_precision', document_id=document, mode='rgba16', flatten=True)
    for _ in range(2):
        app.call('duplicate_layer', document_id=document, index=0)
    rows = app.call('get_document', document_id=document)['layers']
    assert len(rows) == 3, rows
    assert not any(row['clipped'] for row in rows), rows
    first = app.call('set_layer_clipped', document_id=document, layer=2, clipped=True)
    assert first['ok'], first
    folded = app.call('set_layer_clipped', document_id=document, layers=[1, 2], clipped=True)
    assert folded['ok'] and folded['skipped'] == [2], folded
    rows = {row['index']: row for row in app.call('get_document', document_id=document)['layers']}
    assert rows[1]['clipped'] and rows[2]['clipped'], rows
    # ONE undo, measured: the fold minted a single step for the pair.
    app.call('undo', document_id=document)
    rows = {row['index']: row for row in app.call('get_document', document_id=document)['layers']}
    assert rows[1]['clipped'] is False and rows[2]['clipped'] is True, rows
    app.call('undo', document_id=document)
    rows = app.call('get_document', document_id=document)['layers']
    assert not any(row['clipped'] for row in rows), rows
    refusal = app.call('set_layer_clipped', document_id=document, layers=[1, 2],
                       clipped=False, fail=True)
    assert 'Release Clipping Mask' in refusal, refusal
    assert '1' in refusal and '2' in refusal, refusal
    rows = app.call('get_document', document_id=document)['layers']
    assert not any(row['clipped'] for row in rows), rows
    receipts[gate] = {'folded': folded, 'refusal': refusal}
    print('PASS p6: the clipped set folds, lists what it skipped, and still refuses a dead set',
          flush=True)


def gate_p7(app, folder, receipts):
    gate = 'p7_lock_noop_echo'
    path = folder / 'lock.png'
    fixture(path)
    document = app.client.open(path)
    app.call('duplicate_layer', document_id=document, index=0)
    first = app.call('set_layer_lock', document_id=document, layer=1, locks=['position'])
    assert first['ok'] and first['layer'] == 1, first
    again = app.call('set_layer_lock', document_id=document, layer=1, locks=['position'])
    assert again['changed'] is False, again
    assert again['layer'] == 1, again
    assert again['layers'] == [1], again
    receipts[gate] = {'first': first, 'again': again}
    print('PASS p7: a repeated single-entry lock echoes the layer it was given', flush=True)


def gate_p8(app, presets, receipts):
    """The write bound and the read bound must count the SAME thing."""
    gate = 'p8_file_bound'
    findings = {}
    assert (presets / MALFORMED).exists(), 'P8 needs a bad file still present'
    pad = []
    index = 0
    while len(list(presets.glob('*.json'))) < 128:
        name = 'Pad %04d' % index
        index += 1
        target = presets / digest_name(name)
        if target.exists():
            continue
        target.write_text(json.dumps({'schema_version': 2, 'name': name, 'raw': {'exposure': 0.1}}))
        pad.append(target)
    files = sorted(path.name for path in presets.glob('*.json'))
    assert len(files) == 128, len(files)
    listing = app.call('list_raw_presets')
    broken = [row['file'] for row in listing['broken']]
    assert MALFORMED in broken, listing['broken']
    assert len(listing['presets']) + len(listing['broken']) == 128, listing
    findings['at_bound'] = {'files': len(files), 'listed': len(listing['presets']),
                            'broken': broken}

    full = app.call('save_raw_preset', name='Overflow', raw={'exposure': 0.2}, fail=True)
    assert '128' in full, full
    assert not (presets / digest_name('Overflow')).exists(), 'a 129th file was written'
    assert sorted(path.name for path in presets.glob('*.json')) == files, 'the folder changed'
    findings['refused_when_full'] = full
    # The library is still READABLE at the bound — the failure this slice
    # removes was a 129th file that refused every later read.
    assert len(app.call('list_raw_presets')['presets']) == len(listing['presets'])

    removed = app.call('delete_raw_preset', file=MALFORMED)
    assert removed['ok'] and removed['file'] == MALFORMED, removed
    assert not (presets / MALFORMED).exists()
    assert len(list(presets.glob('*.json'))) == 127, 'the bad file was not the slot freed'
    findings['deleted_bad_file'] = removed

    saved = app.call('save_raw_preset', name='Overflow', raw={'exposure': 0.2})
    assert saved['ok'], saved
    assert (presets / digest_name('Overflow')).exists()
    findings['saved_after_free'] = saved['preset']
    app.call('delete_raw_preset', name='Overflow')
    for path in pad:
        path.unlink()
    receipts[gate] = findings
    print('PASS p8: the save bound counts files, and deleting a bad file frees the slot',
          flush=True)


def gate_p9(app, presets, receipts):
    gate = 'p9_repair_only_repairs'
    row = next(r for r in app.call('list_raw_presets')['presets'] if r['name'] == 'Sunny')
    path = presets / row['file']
    before = path.read_bytes()
    refusal = app.call('repair_raw_preset', file=row['file'], fail=True)
    assert 'nothing to repair' in refusal, refusal
    assert path.read_bytes() == before, row['file']
    assert next(r for r in app.call('list_raw_presets')['presets'] if r['name'] == 'Sunny') == row
    receipts[gate] = {'entry': row, 'refusal': refusal}
    print('PASS p9: repairing a valid entry\'s file is refused', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.receipts).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    files = folder / 'files'
    files.mkdir()
    synthetic_dng(files / 'source.dng')
    for name in ['sync-target.dng', 'replay-0.dng', 'replay-1.dng', 'replay-2.dng', 'replay-3.dng']:
        shutil.copy2(files / 'source.dng', files / name)
    presets = folder / 'raw-presets'
    seed(presets)

    receipts = {'tolerances': {'everything': EXACT},
                'note': 'Every claim in this drive is exact; there is no tolerance to widen.'}
    previous = phase5_support.APP
    previous_env = {name: os.environ.get(name) for name in
                    ('DR_EXPORT_PRESETS_DIR',
                     'DR_RAW_PRESETS_DIR', 'DR_METADATA_POLICIES_DIR')}
    phase5_support.APP = executable
    # Private libraries for every preference library the app owns, so a run can
    # neither read nor write the user's own.
    for name, leaf in [('DR_EXPORT_PRESETS_DIR', 'export-presets'),
                       ('DR_RAW_PRESETS_DIR', 'raw-presets'),
                       ('DR_METADATA_POLICIES_DIR', 'metadata-policies')]:
        os.environ[name] = str(folder / leaf)
    try:
        with App(folder / 'app', folder / 'recovery', interval=600) as app:
            document = gate_p1(app, presets, files, receipts)
            gate_p2(app, presets, receipts)
            gate_p3(app, presets, receipts)
            gate_p4(app, presets, files, document, receipts)
            gate_p5(app, files, folder, receipts)
            gate_p6(app, folder, receipts)
            gate_p7(app, folder, receipts)
            gate_p8(app, presets, receipts)
            gate_p9(app, presets, receipts)
    finally:
        phase5_support.APP = previous
        for name, value in previous_env.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
