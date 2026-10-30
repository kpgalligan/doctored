#!/usr/bin/env python3
"""The Phase 12C-1 workspace-productivity MCP drive, on a built Release app.

Five gates, FIXED before the run; a failed gate IS the receipt and no tolerance
is widened afterwards. Every instance is owned by this file (a free port, a
dummy API key and PRIVATE directories for every preference library the app
reads, the new `DR_DOCUMENT_PRESETS_DIR` included) and is terminated by its own
context manager; no Doctored process this file did not start is ever
signalled.

- **N1 New Document.** `new_document {8 × 10 in @ 300 ppi, Display P3, 16-bit,
  transparent}` is EXACTLY 2400 × 3000 with `precision == "rgba16"`, ONE layer
  named "Layer 1", alpha 0 at five points, and an exported PNG whose embedded
  ICC — extracted with `magick` and parsed HERE from the raw tag table — has
  description "Display P3" and `rXYZ`/`gXYZ`/`bXYZ` equal to
  `/System/Library/ColorSync/Profiles/Display P3.icc`'s. A 100 × 100 8-bit
  white document's layer is named "Background" and its centre pixel is opaque
  white. A saved preset replayed through `new_document {preset}` produces the
  same width, height, depth and profile. `depth: 24` (the refusal names 32
  as allowed) and a GRAY ICC path are refused BY NAME.
- **L1 labels persist.** A three-entry document (raster, group, adjustment)
  carrying three different labels survives `save_copy` to `.rz` plus
  `open_document`, and `export_psd` (layered) plus `open_document`, at 8 AND
  16 bits — `get_document` rows report the labels both times. The 8-bit case is
  repeated with `set_evaluation linear_light` on, which is the byte writer's
  41-wraps-38 path.
- **L2 find_layers.** A 60-entry document built through `new_layer`,
  `add_text_layer`, `add_shape_layer`, `add_adjustment_layer`, `group_layers`
  and `set_layer_label`, with at least one labelled entry inside a group closed
  through `set_layer_properties {open: false}`: `find_layers` by name
  substring, by EACH kind, by label and combined returns exactly the index set
  this drive computes for itself from `get_document`'s own rows.
- **L3 the recorded step and the no-op.** `start_recording` →
  `set_layer_label` → `stop_recording` / `save_action` → `run_action` on a
  fresh copy applies the same label; a repeated `set_layer_label` answers
  `changed: false` with the index in `skipped`, and `list_history` shows no new
  entry.
- **S1 the lifted per-entry diagnosis.** `DR_DOCUMENT_PRESETS_DIR` seeded
  with one VALID entry and one malformed
  file: the list tool returns the valid entry plus exactly one `broken` row
  with code `not_json`; `delete_document_preset {file}` clears it; and a save whose
  canonical digest file is already occupied is refused NAMING that file.

Nothing here derives an expectation from Doctored: the sizes, names, labels
and index sets are computed in this file, the ICC tag table is parsed here, the
preset files are written here and the digest file names come from `hashlib`.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

import phase5_support
from phase5_support import App

# ---- the tolerances, fixed here BEFORE anything is measured ----------------
# Pixel sizes, layer names, labels, index sets, ICC descriptions and file names
# are EXACT. The one numeric tolerance is on the ICC colorant matrix, which is
# stored as s15Fixed16 in both files being compared: 1/65536 is the
# representation's own step, so anything larger would be comparing two
# different profiles.
COLORANT_EPSILON = 1.0 / 65536.0
SYSTEM_DISPLAY_P3 = Path('/System/Library/ColorSync/Profiles/Display P3.icc')
SYSTEM_GRAY = Path('/System/Library/ColorSync/Profiles/Generic Gray Profile.icc')

MALFORMED = 'zz-malformed.json'


def digest_name(name):
    """A preference library's canonical file name for `name`.

    `PreferenceLibrary.url` is sha256 of the en_US_POSIX case-fold; every name
    this drive uses is ASCII, where that fold IS `str.lower`.
    """
    assert name.isascii(), name
    return hashlib.sha256(name.lower().encode()).hexdigest() + '.json'


# ------------------------------------------------------- an ICC tag reader --

def icc_tags(data):
    """The profile's tag table: signature -> body bytes. Written from the ICC
    spec's own layout (128-byte header, u32 count, then 12 bytes per tag)."""
    assert len(data) >= 132, len(data)
    count = struct.unpack('>I', data[128:132])[0]
    assert 0 < count < 256, count
    tags = {}
    for index in range(count):
        at = 132 + index * 12
        signature = data[at:at + 4]
        offset, size = struct.unpack('>II', data[at + 4:at + 12])
        assert offset + size <= len(data), (signature, offset, size)
        tags[signature] = data[offset:offset + size]
    return tags


def icc_xyz(body):
    """An `XYZ `-typed tag as three floats (s15Fixed16)."""
    assert body[:4] == b'XYZ ', body[:4]
    return tuple(value / 65536.0 for value in struct.unpack('>iii', body[8:20]))


def icc_description(body):
    """A `desc` (v2 ASCII) or `mluc` (v4 UTF-16BE) tag as text."""
    kind = body[:4]
    if kind == b'desc':
        length = struct.unpack('>I', body[8:12])[0]
        return body[12:12 + length].rstrip(b'\0').decode('ascii')
    assert kind == b'mluc', kind
    count, size = struct.unpack('>II', body[8:16])
    assert count >= 1 and size == 12, (count, size)
    length, offset = struct.unpack('>II', body[20:28])
    return body[offset:offset + length].decode('utf-16-be')


def png_icc_with_magick(png_path, icc_path):
    """The PNG's embedded profile, extracted by ImageMagick."""
    subprocess.run(['magick', str(png_path), f'icc:{icc_path}'], check=True,
                   capture_output=True)
    return Path(icc_path).read_bytes()


# --------------------------------------------------------------- utilities --

def sample_alpha(sample):
    """The alpha of a `sample_pixel` reply at either depth: a byte document
    answers r/g/b/a, a 16-bit one a typed block with `center_rgba16`."""
    if 'a' in sample:
        return sample['a'] / 255.0
    return sample['center_rgba16'][3] / float(sample['maximum_code'])


def rows_of(app, document):
    return app.call('get_document', document_id=document)['layers']


def labels_of(app, document):
    return [(row['name'], row['label']) for row in rows_of(app, document)]


def seed_library(folder, name, valid):
    """One valid entry at its canonical digest name, and one malformed file."""
    folder.mkdir(parents=True, exist_ok=True)
    (folder / digest_name(name)).write_text(json.dumps(valid))
    (folder / MALFORMED).write_text('{ this is not json')


# ------------------------------------------------------------------ gates --

def gate_n1(app, folder, receipts):
    gate = 'n1_new_document'
    findings = {}

    created = app.call('new_document', width=8, height=10, unit='in', ppi=300,
                       profile='display_p3', depth=16, background='transparent')
    document = created['document']['id']
    assert created['document']['width'] == 2400, created
    assert created['document']['height'] == 3000, created
    assert created['document']['precision'] == 'rgba16', created
    rows = rows_of(app, document)
    assert len(rows) == 1, rows
    assert rows[0]['name'] == 'Layer 1', rows[0]
    for x, y in [(0, 0), (2399, 0), (0, 2999), (2399, 2999), (1200, 1500)]:
        sample = app.call('sample_pixel', document_id=document, x=x, y=y)
        assert sample_alpha(sample) == 0.0, (x, y, sample)
    findings['transparent_16bit'] = {'summary': created['document'],
                                     'layer': rows[0]['name']}

    exported = folder / 'p3.png'
    app.call('export_image', document_id=document, path=str(exported),
             settings={'format': 'png', 'profile': 'document', 'bit_depth': 8},
             overwrite=True)
    embedded = icc_tags(png_icc_with_magick(exported, folder / 'p3.icc'))
    system = icc_tags(SYSTEM_DISPLAY_P3.read_bytes())
    assert icc_description(embedded[b'desc']) == 'Display P3', \
        icc_description(embedded[b'desc'])
    colorants = {}
    for tag in (b'rXYZ', b'gXYZ', b'bXYZ'):
        mine, theirs = icc_xyz(embedded[tag]), icc_xyz(system[tag])
        for a, b in zip(mine, theirs):
            assert abs(a - b) <= COLORANT_EPSILON, (tag, mine, theirs)
        colorants[tag.decode()] = mine
    findings['display_p3_icc'] = {'description': 'Display P3', 'colorants': colorants}

    white = app.call('new_document', width=100, height=100, depth=8, background='white')
    white_id = white['document']['id']
    white_rows = rows_of(app, white_id)
    assert len(white_rows) == 1 and white_rows[0]['name'] == 'Background', white_rows
    centre = app.call('sample_pixel', document_id=white_id, x=50, y=50)
    assert [centre['r'], centre['g'], centre['b']] == [255, 255, 255], centre
    assert sample_alpha(centre) == 1.0, centre
    findings['white_8bit'] = {'layer': white_rows[0]['name'], 'centre': centre}

    saved = app.call('save_document_preset', name='Print 8x10', width=8, height=10,
                     unit='in', ppi=300, profile='display_p3', depth=16,
                     background='transparent')
    assert saved['ok'], saved
    from_preset = app.call('new_document', preset='Print 8x10')
    for key in ('width', 'height', 'precision'):
        assert from_preset['document'][key] == created['document'][key], (key, from_preset)
    assert from_preset['profile'] == created['profile'] == 'display_p3', from_preset
    findings['preset_round_trip'] = {'saved': saved['preset'],
                                     'document': from_preset['document']}

    depth_refusal = app.call('new_document', width=10, height=10, depth=24, fail=True)
    assert '8, 16 or 32' in depth_refusal, depth_refusal
    gray_refusal = app.call('new_document', width=10, height=10,
                            profile=str(SYSTEM_GRAY), fail=True)
    assert 'RGB' in gray_refusal, gray_refusal
    findings['refusals'] = {'depth_24': depth_refusal, 'gray_profile': gray_refusal}

    receipts[gate] = findings
    print('PASS n1: a print-sized transparent Display P3 16-bit document, and its preset',
          flush=True)


def three_entry_document(app, folder, name, depth):
    """A raster + group + adjustment document carrying three labels."""
    path = folder / f'{name}-seed.png'
    phase5_support.fixture(path, width=48, height=32)
    document = app.open(str(path))
    if depth == 16:
        # Converted ONCE, before the structure is built: the conversion
        # deliberately flattens to one raster layer.
        app.call('convert_precision', document_id=document, mode='rgba16', flatten=True)
        # `new_layer` is a byte-only tool; the typed route to a second raster
        # is a duplicate plus a rename, both of which 16 bits admits.
        app.call('duplicate_layer', document_id=document, index=0)
        app.call('set_layer_properties', document_id=document, index=1, name='Sketch')
    else:
        app.call('new_layer', document_id=document, name='Sketch')
    app.call('add_adjustment_layer', document_id=document, op='invert')
    rows = rows_of(app, document)
    assert len(rows) == 3, rows
    # EVERY top-level entry: 16-bit grouping is all-or-nothing, and using the
    # same call at both depths keeps the two documents the same shape.
    app.call('group_layers', document_id=document,
             layers=[row['index'] for row in rows], name='Folder')
    rows = rows_of(app, document)
    assert len(rows) == 4, rows
    plan = {}
    for row in rows:
        label = {'Background': 'red', 'Sketch': 'green', 'Folder': 'blue'}.get(row['name'])
        if label is None:
            label = 'violet'  # the adjustment layer, whatever it was named
        app.call('set_layer_label', document_id=document, layer=row['index'], label=label)
        plan[row['name']] = label
    return document, plan


def gate_l1(app, folder, receipts):
    gate = 'l1_labels_persist'
    findings = {}
    for depth in (8, 16):
        document, plan = three_entry_document(app, folder, f'l1-{depth}', depth)
        before = dict(labels_of(app, document))
        assert before == plan, (depth, before, plan)

        native = folder / f'l1-{depth}.rz'
        app.call('save_copy', document_id=document, path=str(native), format='rz')
        reopened = app.open(str(native))
        assert dict(labels_of(app, reopened)) == plan, labels_of(app, reopened)

        psd = folder / f'l1-{depth}.psd'
        app.call('export_psd', document_id=document, path=str(psd), mode='layers')
        notice = app.call('inspect_psd', path=str(psd))
        imported = app.call('open_document', path=str(psd), psd_mode='layers',
                            psd_fingerprint=notice['source_fingerprint'])['document']['id']
        psd_labels = dict(labels_of(app, imported))
        for name, label in plan.items():
            assert psd_labels.get(name) == label, (name, label, psd_labels)
        findings[f'depth_{depth}'] = {'plan': plan, 'native': dict(labels_of(app, reopened)),
                                      'psd': psd_labels}

    # The 8-bit case again under linear_light: 41 wraps 38 in the byte writer.
    document, plan = three_entry_document(app, folder, 'l1-linear', 8)
    switched = app.call('set_evaluation', document_id=document, policy='linear_light')
    assert switched.get('ok', True), switched
    native = folder / 'l1-linear.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    reopened = app.open(str(native))
    assert dict(labels_of(app, reopened)) == plan, labels_of(app, reopened)
    evaluation = app.call('get_evaluation', document_id=reopened)
    assert evaluation['policy'] == 'linear_light', evaluation
    findings['linear_light'] = {'plan': plan, 'reopened': dict(labels_of(app, reopened)),
                                'evaluation': evaluation['policy']}

    receipts[gate] = findings
    print('PASS l1: labels survive .rz and PSD round trips at 8 and 16 bits', flush=True)


def gate_l2(app, folder, receipts):
    gate = 'l2_find_layers'
    path = folder / 'l2-seed.png'
    phase5_support.fixture(path, width=64, height=48)
    document = app.open(str(path))
    call = lambda tool, **arguments: app.call(tool, document_id=document, **arguments)

    # Sixty entries with names this drive chooses, so nothing about the
    # expectations comes from Doctored.
    for index in range(20):
        call('new_layer', name=f'Plate {index:02d}')
    for index in range(8):
        call('add_text_layer', text=f'Caption {index}', x=4, y=6 + index,
             size=10, font='Helvetica Neue', color='#204080')
    for index in range(8):
        call('add_shape_layer', kind='rect', x=2, y=2, w=12, h=9, fill='#3c7ac8')
    for index in range(8):
        call('add_adjustment_layer', op='invert')
    rows = rows_of(app, document)
    assert len(rows) >= 45, len(rows)
    # One group holding three entries, closed, with a labelled member inside.
    call('group_layers', layers=[2, 3, 4], name='Closed Folder')
    rows = rows_of(app, document)
    folder_row = next(row for row in rows if row['name'] == 'Closed Folder')
    inside = [row['index'] for row in rows
              if row['index'] < folder_row['index'] and row['depth'] > folder_row['depth']]
    assert len(inside) == 3, inside
    call('set_layer_label', layer=inside[0], label='orange')
    call('set_layer_properties', index=folder_row['index'], open=False)
    # A spread of labels over the rest, so no query is trivially everything.
    rows = rows_of(app, document)
    for position, row in enumerate(rows):
        if row['index'] == inside[0]:
            continue
        if position % 5 == 0:
            call('set_layer_label', layer=row['index'], label='yellow')
        elif position % 7 == 0:
            call('set_layer_label', layer=row['index'], label='blue')
    # Pad to sixty entries.
    while len(rows_of(app, document)) < 60:
        call('new_layer', name=f'Filler {len(rows_of(app, document)):02d}')
    rows = rows_of(app, document)
    assert len(rows) == 60, len(rows)

    def expected_kind(row):
        """This drive's OWN kind rule, read off get_document's other fields."""
        if row.get('kind') == 'group':
            return 'group'
        if row.get('is_adjustment'):
            return 'adjustment'
        if 'text' in row:
            return 'text'
        if 'shape' in row:
            return 'shape'
        if row.get('embedded_source') or row.get('image_source') or 'live_photo' in row:
            return 'embedded'
        return 'raster'

    def expect(name=None, kind=None, label=None):
        out = []
        for row in rows:
            if name is not None and name.lower() not in row['name'].lower():
                continue
            if kind is not None and expected_kind(row) != kind:
                continue
            if label is not None and row['label'] != label:
                continue
            out.append(row['index'])
        return sorted(out)

    findings = {'entries': len(rows)}
    queries = [
        ('name', {'name': 'Plate'}),
        ('name_case', {'name': 'plate 0'}),
        ('kind_raster', {'kind': 'raster'}),
        ('kind_text', {'kind': 'text'}),
        ('kind_shape', {'kind': 'shape'}),
        ('kind_adjustment', {'kind': 'adjustment'}),
        ('kind_group', {'kind': 'group'}),
        ('kind_embedded', {'kind': 'embedded'}),
        ('label_orange', {'label': 'orange'}),
        ('label_yellow', {'label': 'yellow'}),
        ('label_none', {'label': 'none'}),
        ('combined', {'name': 'Plate', 'label': 'yellow', 'kind': 'raster'}),
    ]
    for title, arguments in queries:
        answer = call('find_layers', **arguments)
        got = sorted(row['index'] for row in answer['layers'])
        assert got == expect(**arguments), (title, got, expect(**arguments))
        assert answer['count'] == len(got), (title, answer)
        findings[title] = got
    # The closed group's labelled child is reported, which is the whole point.
    orange = call('find_layers', label='orange')
    assert [row['index'] for row in orange['layers']] == [inside[0]], orange
    assert any(row['index'] == inside[0] for row in orange['layers']), orange
    findings['closed_group_child'] = {'group': folder_row['name'], 'index': inside[0]}

    receipts[gate] = findings
    print('PASS l2: find_layers answers exactly the drive\'s own index sets', flush=True)


def gate_l3(app, folder, receipts):
    gate = 'l3_recorded_and_no_op'
    path = folder / 'l3-seed.png'
    phase5_support.fixture(path, width=32, height=24)
    document = app.open(str(path))

    app.call('start_recording')
    app.call('set_layer_label', document_id=document, layer=0, label='green')
    # `stop_recording {name}` saves the recording under that name, which is
    # the UI's Stop-then-name gesture and what `save_action` takes its steps
    # from.
    action = app.call('stop_recording', name='Label Green')
    assert len(action['steps']) == 1, action
    assert action['steps'][0]['tool'] == 'set_layer_label', action
    # Saved again through `save_action`, which is the other half of the pair
    # and proves the step survives the library writer's own round trip.
    saved = app.call('save_action', name='Label Green', steps=action['steps'],
                     overwrite=True)
    assert saved.get('ok', True), saved

    replay_path = folder / 'l3-replay.png'
    shutil.copy2(path, replay_path)
    replay = app.open(str(replay_path))
    played = app.call('run_action', name='Label Green', document_id=replay)
    assert played['ok'], played
    assert rows_of(app, replay)[0]['label'] == 'green', rows_of(app, replay)

    before = len(app.call('list_history', document_id=replay)['states'])
    again = app.call('set_layer_label', document_id=replay, layer=0, label='green')
    assert again['changed'] is False, again
    assert again['layers'] == [], again
    assert again['skipped'] == [0], again
    after = len(app.call('list_history', document_id=replay)['states'])
    assert after == before, (before, after)

    receipts[gate] = {'action': action, 'replayed': rows_of(app, replay)[0]['label'],
                      'no_op': again, 'history_entries': before}
    print('PASS l3: a recorded label replays, and a repeat adds no history step', flush=True)


def gate_s1(app, documents, receipts):
    gate = 's1_per_entry_diagnosis'
    findings = {}
    for title, list_tool, delete_tool, folder, key, valid_name in [
        ('document_presets', 'list_document_presets', 'delete_document_preset',
         documents, 'presets', 'Card'),
    ]:
        listing = app.call(list_tool)
        names = [row['name'] for row in listing[key]]
        # The seeded valid entry is listed BESIDE the bad file, which is the
        # whole point; the document library also holds whatever N1 saved.
        assert valid_name in names, (title, listing)
        broken = listing['broken']
        assert len(broken) == 1, (title, broken)
        assert broken[0]['file'] == MALFORMED, (title, broken)
        assert broken[0]['code'] == 'not_json', (title, broken)
        assert broken[0]['repairable'] is False, (title, broken)
        assert broken[0]['reason'], (title, broken)
        removed = app.call(delete_tool, file=MALFORMED)
        assert removed['ok'] and removed['file'] == MALFORMED, (title, removed)
        assert not (folder / MALFORMED).exists(), title
        assert app.call(list_tool)['broken'] == [], title
        findings[title] = {'listed': names, 'broken': broken, 'deleted': removed}

    # A save whose canonical digest file is already occupied by ANOTHER name's
    # file is refused NAMING that file, rather than overwriting it.
    occupied = documents / digest_name('Collide')
    occupied.write_text(json.dumps(
        {'schema_version': 1, 'name': 'Parked', 'width': 4, 'height': 6, 'unit': 'in',
         'ppi': 300, 'profile': 'srgb', 'depth': 8, 'background': 'white'}))
    before = occupied.read_bytes()
    refusal = app.call('save_document_preset', name='Collide', width=2, height=3,
                       unit='in', ppi=300, fail=True)
    assert digest_name('Collide') in refusal, refusal
    assert occupied.read_bytes() == before, 'the occupied file was overwritten'
    findings['occupied_slot_refusal'] = refusal

    receipts[gate] = findings
    print('PASS s1: one bad file is a diagnosed row, deletable by file, and never overwritten',
          flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    assert SYSTEM_DISPLAY_P3.is_file(), SYSTEM_DISPLAY_P3
    assert SYSTEM_GRAY.is_file(), SYSTEM_GRAY
    output = Path(args.receipts).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    documents = folder / 'document-presets'
    seed_library(documents, 'Card', {
        'schema_version': 1, 'name': 'Card', 'width': 5, 'height': 7, 'unit': 'in',
        'ppi': 300, 'profile': 'srgb', 'depth': 8, 'background': 'white'})

    receipts = {'tolerances': {'colorant_s15fixed16_step': COLORANT_EPSILON,
                               'everything_else': 'exact'},
                'note': 'The one tolerance is the ICC colorant encoding\'s own step; '
                        'sizes, names, labels, index sets and file names are exact.'}
    previous_app = phase5_support.APP
    library_env = {'DR_DOCUMENT_PRESETS_DIR': documents,
                   'DR_RAW_PRESETS_DIR': folder / 'raw-presets',
                   'DR_EXPORT_PRESETS_DIR': folder / 'export-presets',
                   'DR_METADATA_POLICIES_DIR': folder / 'metadata-policies'}
    previous_env = {name: os.environ.get(name) for name in library_env}
    phase5_support.APP = executable
    for name, value in library_env.items():
        os.environ[name] = str(value)
    try:
        with App(folder / 'app', folder / 'recovery', interval=600) as app:
            gate_n1(app, folder, receipts)
            gate_l1(app, folder, receipts)
            gate_l2(app, folder, receipts)
            gate_l3(app, folder, receipts)
            gate_s1(app, documents, receipts)
    finally:
        phase5_support.APP = previous_app
        for name, value in previous_env.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
