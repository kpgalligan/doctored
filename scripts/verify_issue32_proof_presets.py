#!/usr/bin/env python3
"""Saved soft-proof presets: the MCP drive, on a built Release app.

Ten gates, FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no tolerance is widened afterwards. The instance is owned by this
file (`HostApp`: a free port, a dummy API key and a PRIVATE directory for
every preference library the app reads — `DR_PROOF_PRESETS_DIR` included) and
is terminated by its own context manager; no Doctored process this file did
not start is ever signalled. Export `DR_BACKGROUND=1` before running.

The fixture is a temp COPY of macOS's own Generic CMYK profile, asserted
`proofable` through `describe_profile`; it lives outside every installed
profile folder, so a preset saved from it carries a `warning`.

- **P1 save.** `set_proof_setup` from the copy, then `save_proof_preset
  "Paper A"`: the library file is at most 4096 bytes and `warning` is
  present. A second save without `replace` is refused. `"  Paper B  "` saves
  as `"Paper B"`, lists as an available entry with no broken row, and is
  deleted again.
- **P2 list.** "Paper A" lists `available: true`; a seeded malformed `.json`
  is exactly one `not_json` broken row, which `delete_proof_preset {file}`
  clears.
- **P3 apply.** On a second document `set_proof_setup preset:"Paper A"` reads
  back the saved fields with `proof_colors: true`; `proof_colors:false` is
  honoured; `preset` beside `profile` is refused.
- **P4 render.** `render_proof preset:"Paper A"` on a window with no setup
  answers the same `out_of_gamut` as the explicit `profile:"file"` call, and
  leaves the window without a setup.
- **P5 default.** After `set_default_proof_preset "Paper A"`, a fresh
  document's `get_proof_setup` reports it, a bare `render_proof` there
  succeeds with P4's count, an unknown name is refused and `null` clears it.
- **P6 installed fallback.** The copy is overwritten with Generic RGB bytes;
  `set_proof_setup preset:"Paper A"` still resolves, and its `path` is the
  INSTALLED Generic CMYK file `list_installed_profiles` names.
- **P7 stale.** A seeded "Stale" preset with a wrong `profile_sha256` is
  refused by `set_proof_setup` with the "changed or moved" sentence and the
  window's setup is unchanged; it lists `available: false`; as the default, a
  bare `render_proof` on a fresh document refuses with that sentence.
- **P8 delete the default.** Deleting the default preset by name clears
  `default_preset`; deleting the default by `file` clears it too.
- **P9 view state.** The `render` PNG's SHA-256, `unsaved_changes` and the
  `list_history` count are unchanged across applying a preset.
- **P10 depth.** P3 and P4 pass on a `new_document depth:16`. With the float
  fixture `core/tests/fixtures/float_11f/scene.tiff` open last, the three
  document-free tools answer without a document_id and `save_proof_preset`
  on it is refused.

Menus and the Proof Setup sheet cannot be driven over MCP, and the
`scripts/*_ui.py` probes are out of bounds here, so the UI is covered by the
build only.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

import app_under_test
from support import HostApp

# ---- fixed BEFORE anything is measured -------------------------------------
# There are no tolerances: every count compared is an integer the app reports
# twice for the same pixels and profile, and every field is echoed back.
GENERIC_CMYK = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
GENERIC_RGB = Path('/System/Library/ColorSync/Profiles/Generic RGB Profile.icc')
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
MALFORMED = 'zz-malformed.json'
MAX_FILE_BYTES = 4096
STALE_SENTENCE = 'has changed or moved since proof preset'
# The saved condition: deliberately NOT the defaults, so a read-back that
# fell back to them cannot pass.
CONDITION = {'intent': 'perceptual', 'black_point_compensation': False,
             'simulate_paper_white': True, 'simulate_ink_black': False,
             'gamut_color': '#FF00FF'}
CANVAS = dict(width=96, height=64, background='#00FF40')
FIELDS = ('intent', 'black_point_compensation', 'simulate_paper_white',
          'simulate_ink_black', 'gamut_color')


def digest_name(name):
    """A preference library's canonical file name for `name` — sha256 of the
    en_US_POSIX case-fold, which for ASCII IS `str.lower`."""
    assert name.isascii(), name
    return hashlib.sha256(name.lower().encode()).hexdigest() + '.json'


def render_digest(app, document):
    result = app.client.rpc('tools/call', {'name': 'render',
                                           'arguments': {'document_id': document}})
    assert not result.get('isError'), result
    data = next(c['data'] for c in result['content'] if c['type'] == 'image')
    return hashlib.sha256(data.encode()).hexdigest()


def history_length(app, document):
    return len(app.call('list_history', document_id=document)['states'])


def new_document(app, depth=8):
    created = app.call('new_document', depth=depth, **CANVAS)
    return created['document']['id']


def listed(app, name):
    rows = [row for row in app.call('list_proof_presets')['presets'] if row['name'] == name]
    assert len(rows) == 1, (name, rows)
    return rows[0]


def assert_condition(reply, preset, where):
    for key in FIELDS:
        assert reply[key] == preset[key], (where, key, reply[key], preset[key])
    assert reply['profile_name'] == preset['profile_name'], (where, reply)


# ------------------------------------------------------------------ gates --

def gate_p1(app, copy, presets_dir, receipts):
    findings = {}
    described = app.call('describe_profile', path=str(copy))
    assert described['proofable'] is True, described
    findings['fixture'] = {'path': str(copy), 'name': described['name'],
                           'proofable': described['proofable']}
    document = new_document(app)
    app.call('set_proof_setup', document_id=document, profile='file', path=str(copy),
             proof_colors=True, **CONDITION)
    saved = app.call('save_proof_preset', document_id=document, name='Paper A')
    assert saved['ok'] is True, saved
    preset = saved['preset']
    assert preset['name'] == 'Paper A', preset
    assert preset['profile_path'] == str(copy), preset
    assert preset['profile_sha256'] == hashlib.sha256(copy.read_bytes()).hexdigest(), preset
    for key in FIELDS:
        assert preset[key] == CONDITION[key], (key, preset)
    assert 'warning' in saved, saved
    size = (presets_dir / saved['file']).stat().st_size
    assert size <= MAX_FILE_BYTES, size
    findings['saved'] = saved
    findings['file_bytes'] = size

    again = app.call('save_proof_preset', document_id=document, name='Paper A', fail=True)
    assert 'already exists' in again, again
    findings['second_save_refused'] = again

    padded = app.call('save_proof_preset', document_id=document, name='  Paper B  ')
    assert padded['preset']['name'] == 'Paper B', padded
    listing = app.call('list_proof_presets')
    row = [r for r in listing['presets'] if r['name'] == 'Paper B']
    assert len(row) == 1 and row[0]['available'] is True, listing
    assert listing['broken'] == [], listing['broken']
    removed = app.call('delete_proof_preset', name='Paper B')
    assert removed['ok'] is True, removed
    assert [r['name'] for r in app.call('list_proof_presets')['presets']] == ['Paper A']
    findings['padded_name'] = padded['preset']['name']
    receipts['p1_save'] = findings
    print('PASS p1: a preset saves bounded, warns, refuses a duplicate and normalizes', flush=True)
    return document, preset


def gate_p2(app, presets_dir, receipts):
    row = listed(app, 'Paper A')
    assert row['available'] is True and row['unavailable_reason'] is None, row
    (presets_dir / MALFORMED).write_text('{ this is not json')
    broken = app.call('list_proof_presets')['broken']
    assert len(broken) == 1, broken
    assert broken[0]['file'] == MALFORMED and broken[0]['code'] == 'not_json', broken
    cleared = app.call('delete_proof_preset', file=MALFORMED)
    assert cleared['file'] == MALFORMED, cleared
    assert app.call('list_proof_presets')['broken'] == []
    receipts['p2_list'] = {'available_row': row, 'broken_row': broken[0]}
    print('PASS p2: list reports availability and a broken row', flush=True)


def gate_p3(app, preset, depth, receipts, key):
    document = new_document(app, depth)
    applied = app.call('set_proof_setup', document_id=document, preset='Paper A')
    assert applied['proof_colors'] is True, applied
    read = app.call('get_proof_setup', document_id=document)
    assert_condition(read, preset, 'p3 read-back')
    assert read['proof_colors'] is True, read
    off = app.call('set_proof_setup', document_id=document, preset='Paper A',
                   proof_colors=False)
    assert off['proof_colors'] is False, off
    both = app.call('set_proof_setup', document_id=document, preset='Paper A',
                    profile='file', path=str(GENERIC_CMYK), fail=True)
    assert 'preset' in both and 'profile' in both, both
    receipts[key] = {'depth': depth, 'read_back': read, 'proof_colors_false': off,
                     'preset_and_profile_refused': both}
    print(f'PASS {key}: a preset applies its saved fields at {depth} bits', flush=True)


def gate_p4(app, profile_path, depth, receipts, key):
    document = new_document(app, depth)
    by_preset = app.call('render_proof', document_id=document, preset='Paper A')
    explicit = app.call('render_proof', document_id=document, profile='file',
                        path=str(profile_path), **CONDITION)
    assert by_preset['out_of_gamut'] == explicit['out_of_gamut'], (by_preset, explicit)
    assert by_preset['profile_name'] == explicit['profile_name'], (by_preset, explicit)
    untouched = app.call('get_proof_setup', document_id=document)
    assert untouched['profile_name'] is None, untouched
    receipts[key] = {'depth': depth, 'by_preset': by_preset, 'explicit': explicit}
    print(f'PASS {key}: render_proof preset matches the explicit profile at {depth} bits',
          flush=True)
    return by_preset['out_of_gamut']


def gate_p5(app, out_of_gamut, receipts):
    set_reply = app.call('set_default_proof_preset', name='Paper A')
    assert set_reply['default_preset'] == 'Paper A', set_reply
    document = new_document(app)
    read = app.call('get_proof_setup', document_id=document)
    assert read['default_preset'] == 'Paper A' and read['profile_name'] is None, read
    bare = app.call('render_proof', document_id=document)
    assert bare['out_of_gamut'] == out_of_gamut, (bare, out_of_gamut)
    unknown = app.call('set_default_proof_preset', name='No Such Preset', fail=True)
    assert 'No Such Preset' in unknown, unknown
    cleared = app.call('set_default_proof_preset', name=None)
    assert cleared['default_preset'] is None, cleared
    assert app.call('get_proof_setup', document_id=document)['default_preset'] is None
    assert app.call('list_proof_presets')['default_preset'] is None
    receipts['p5_default'] = {'read': read, 'bare_render': bare, 'unknown_refused': unknown}
    print('PASS p5: the default preset seeds a bare render and clears', flush=True)


def gate_p9(app, receipts):
    document = new_document(app)
    before = (render_digest(app, document),
              app.call('get_document', document_id=document)['unsaved_changes'],
              history_length(app, document))
    app.call('set_proof_setup', document_id=document, preset='Paper A')
    app.call('render_proof', document_id=document, preset='Paper A')
    after = (render_digest(app, document),
             app.call('get_document', document_id=document)['unsaved_changes'],
             history_length(app, document))
    assert before == after, (before, after)
    receipts['p9_view_state'] = {'render_sha256': before[0], 'unsaved_changes': before[1],
                                 'history_states': before[2]}
    print('PASS p9: applying a preset writes no document', flush=True)


def gate_p10_float(app, output, receipts):
    source = new_document(app)
    app.call('set_proof_setup', document_id=source, profile='file', path=str(GENERIC_CMYK))
    app.call('save_proof_preset', document_id=source, name='Temp')
    fixture = output / 'scene.tiff'
    shutil.copyfile(FLOAT_FIXTURE, fixture)
    floating = app.call('open_document', path=str(fixture))['document']['id']
    bits = app.call('get_precision', document_id=floating).get('bits_per_channel')
    assert bits == 32, bits
    listing = app.call('list_proof_presets')
    assert 'Temp' in [row['name'] for row in listing['presets']], listing
    default = app.call('set_default_proof_preset', name=None)
    assert default['ok'] is True, default
    deleted = app.call('delete_proof_preset', name='Temp')
    assert deleted['ok'] is True, deleted
    refused = app.call('save_proof_preset', document_id=floating, name='Float', fail=True)
    assert 'Float' not in [r['name'] for r in app.call('list_proof_presets')['presets']]
    receipts['p10_float'] = {'bits': bits, 'save_refused': refused}
    print('PASS p10: the document-free tools answer beside a float document; save refuses',
          flush=True)


def gate_p6(app, copy, receipts):
    shutil.copyfile(GENERIC_RGB, copy)
    installed = [p for p in app.call('list_installed_profiles', kinds=['cmyk'])['profiles']
                 if p['name'] == 'Generic CMYK Profile']
    assert len(installed) == 1, installed
    document = new_document(app)
    applied = app.call('set_proof_setup', document_id=document, preset='Paper A')
    assert applied['path'] == installed[0]['path'], (applied, installed)
    assert applied['path'] != str(copy), applied
    row = listed(app, 'Paper A')
    assert row['available'] is True, row
    receipts['p6_installed_fallback'] = {'installed_path': installed[0]['path'],
                                         'applied': applied}
    print('PASS p6: a replaced file resolves through the installed profile', flush=True)


def gate_p7(app, copy, presets_dir, receipts):
    stale = {'schema_version': 1, 'name': 'Stale', 'profile_name': 'Generic CMYK Profile',
             'profile_path': str(copy), 'profile_sha256': '0' * 64,
             'intent': 'relative', 'black_point_compensation': True,
             'simulate_paper_white': False, 'simulate_ink_black': False,
             'gamut_color': '#808080'}
    (presets_dir / digest_name('Stale')).write_text(json.dumps(stale, sort_keys=True))
    document = new_document(app)
    app.call('set_proof_setup', document_id=document, profile='file', path=str(GENERIC_CMYK),
             intent='saturation')
    before = app.call('get_proof_setup', document_id=document)
    refused = app.call('set_proof_setup', document_id=document, preset='Stale', fail=True)
    assert STALE_SENTENCE in refused, refused
    after = app.call('get_proof_setup', document_id=document)
    assert before == after, (before, after)
    row = listed(app, 'Stale')
    assert row['available'] is False and STALE_SENTENCE in row['unavailable_reason'], row
    app.call('set_default_proof_preset', name='Stale')
    fresh = new_document(app)
    bare = app.call('render_proof', document_id=fresh, fail=True)
    assert STALE_SENTENCE in bare, bare
    app.call('set_default_proof_preset', name=None)
    receipts['p7_stale'] = {'refusal': refused, 'row': row, 'bare_render_refusal': bare}
    print('PASS p7: a stale preset refuses by name and changes nothing', flush=True)


def gate_p8(app, source, receipts):
    app.call('set_default_proof_preset', name='Paper A')
    by_name = app.call('delete_proof_preset', name='Paper A')
    assert by_name['default_preset'] is None, by_name
    assert app.call('list_proof_presets')['default_preset'] is None
    app.call('save_proof_preset', document_id=source, name='Paper C')
    app.call('set_default_proof_preset', name='Paper C')
    assert app.call('list_proof_presets')['default_preset'] == 'Paper C'
    file = listed(app, 'Paper C')['file']
    by_file = app.call('delete_proof_preset', file=file)
    assert by_file['default_preset'] is None, by_file
    assert app.call('list_proof_presets')['default_preset'] is None
    receipts['p8_delete_default'] = {'by_name': by_name, 'by_file': by_file}
    print('PASS p8: deleting the default by name or by file clears it', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', default=str(app_under_test.default_app_bundle()))
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.receipts).expanduser() if args.receipts else Path(
        tempfile.mkdtemp(prefix='rz-issue32-'))
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)
    copy = folder / 'fixture' / 'Paper Proof.icc'
    copy.parent.mkdir(parents=True)
    shutil.copyfile(GENERIC_CMYK, copy)
    presets_dir = folder / 'app' / 'proof-presets'

    receipts = {'tolerances': 'none'}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        source, preset = gate_p1(app, copy, presets_dir, receipts)
        gate_p2(app, presets_dir, receipts)
        gate_p3(app, preset, 8, receipts, 'p3_apply')
        out_of_gamut = gate_p4(app, copy, 8, receipts, 'p4_render')
        gate_p5(app, out_of_gamut, receipts)
        gate_p9(app, receipts)
        gate_p3(app, preset, 16, receipts, 'p10_apply16')
        gate_p4(app, copy, 16, receipts, 'p10_render16')
        gate_p10_float(app, folder, receipts)
        gate_p6(app, copy, receipts)
        gate_p7(app, copy, presets_dir, receipts)
        gate_p8(app, source, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
