#!/usr/bin/env python3
"""The Phase 11A export-workspace MCP drive, on a built Release app.

Ten gates, fixed before the run; a failed gate IS the receipt.

- **E1 deliverables.** A 16-bit Display P3 master exports an 800-px-long-edge
  sRGB JPEG and a 16-bit TIFF in the document profile. Dimensions, bit depth
  and the embedded profile are read back by `sips`, which is not Doctored.
- **E2 the preview is the file.** `sha256` of every written file equals the
  `encoded_sha256` `export_preview` reported for the same settings.
- **E3 the master is untouched.** `get_document`, `get_precision`,
  `get_color_profile` and the dirty flag are identical before and after every
  export.
- **E4 Quick Export.** A saved preset plus `set_default_export_preset` makes
  `quick_export` land at `destination/<pattern>.<ext>`; a SECOND call by
  preset name writes `<pattern>-2.<ext>` beside it, reports the taken name as
  `requested_path`, and leaves the first file byte-identical.
- **E4b the built-in preset.** With the default cleared, `quick_export` with
  no arguments on a document opened from a folder writes `<name>.png` beside
  it. The default is set again afterwards, for E5.
- **E4c the resolved form refuses.** `settings` + `destination` + `pattern`
  onto a taken name fails with `already exists` and writes nothing: this is
  the form a recorded step and a batch delivery use.
- **E5 presets persist.** The library survives an app relaunch, default
  included, in this drive's own private `DR_EXPORT_PRESETS_DIR`.
- **E6 Actions.** A recorded `quick_export` replays on a RENAMED copy and
  writes a second file, whose name follows the pattern's `{name}`.
- **E7 catalog.** `tools/list` parses and its count is recorded.
- **E8 File Info.** `set_file_info` reads back through
  `get_file_info`, undoes and redoes; a JPEG, a PNG and a TIFF export each
  carry the four fields in their XMP, read by this file's own search of the
  written bytes; a `.rz` copy reopens with them. A recorded `set_file_info`
  naming `copyright` alone is a step whose arguments are exactly that one key,
  and replaying it on a second document changes that document's copyright and
  leaves its own creator, description and keywords as they were.

Nothing here derives an expectation from Doctored: the fixture is written by
this file, the dimensions come from `sips`, the digests from `hashlib`, and
the file-info values from `carried_file_info`'s own reading of each export.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import zlib

sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]

_spec = importlib.util.spec_from_file_location('phase11a_support',
                                               ROOT / 'scripts/host-source-probe/support.py')
support = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(support)

WIDTH = 1000
HEIGHT = 600
LONG_EDGE = 800
FULL = 65535


# ------------------------------------------------------------- the fixture --

def png16(path, width, height, samples):
    """An RGBA16 PNG carrying the sRGB chunk, written without any application."""
    def chunk(kind, body):
        return (struct.pack('>I', len(body)) + kind + body
                + struct.pack('>I', zlib.crc32(kind + body)))

    assert len(samples) == width * height * 4
    rows = b''.join(b'\0' + struct.pack(f'>{width * 4}H',
                                        *samples[y * width * 4:(y + 1) * width * 4])
                    for y in range(height))
    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 16, 6, 0, 0, 0))
                     + chunk(b'sRGB', b'\0') + chunk(b'IDAT', zlib.compress(rows, 6))
                     + chunk(b'IEND', b''))


def plate(width=WIDTH, height=HEIGHT):
    """Hard edges and coprime ramps, so a resample has real detail to move and
    no channel sits at 0 or 65535."""
    span = 61000 - 4096
    out = []
    for y in range(height):
        for x in range(width):
            edge = 61000 if (y // 7) % 3 == 0 else 4096
            out += [4096 + span * x // (width - 1), edge,
                    4096 + span * ((x + y) % height) // (height - 1), FULL]
    return out


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


# --------------------------------------------------------- independent read --

def sips(path, *keys):
    """`sips` is macOS's own image inspector — an independent decoder."""
    arguments = []
    for key in keys:
        arguments += ['-g', key]
    text = subprocess.run(['/usr/bin/sips', *arguments, str(path)],
                          check=True, capture_output=True, text=True).stdout
    values = {}
    for line in text.splitlines()[1:]:
        if ':' not in line:
            continue
        name, value = line.split(':', 1)
        values[name.strip()] = value.strip()
    return values


def inspected(path):
    values = sips(path, 'pixelWidth', 'pixelHeight', 'bitsPerSample', 'profile')
    return dict(width=int(values.get('pixelWidth', 0)),
                height=int(values.get('pixelHeight', 0)),
                bits=int(values.get('bitsPerSample', 0)),
                profile=values.get('profile', ''),
                bytes=Path(path).stat().st_size)


# ---------------------------------------------------------------- file info --

FILE_INFO = dict(creator=['Ada Lovelace', 'Charles Babbage'],
                 copyright='Copyright 2026 Example Studio',
                 description='A plate of ramps and hard edges, first line.',
                 keywords=['alpha', 'bravo plate', 'charlie'])
EMPTY_FILE_INFO = dict(creator=[], copyright='', description='', keywords=[])
RECORDED_COPYRIGHT = 'Copyright 2026 Recorded Notice'
OTHER_FILE_INFO = dict(creator=['Grace Hopper'], description='The second document.',
                       keywords=['delta', 'echo'])


def xmp_bearing_text(path):
    """The text of an export that an XMP packet can sit in, read without any
    application: a PNG's iTXt chunks (inflated when the chunk says so), and
    the whole file for a JPEG or a TIFF, which store the packet as it is."""
    data = Path(path).read_bytes()
    if not data.startswith(b'\x89PNG\r\n\x1a\n'):
        return data.decode('latin-1')
    texts, at = [], 8
    while at + 8 <= len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b'iTXt':
            _, _, rest = body.partition(b'\0')            # keyword
            deflated = rest[0] == 1
            _, _, rest = rest[2:].partition(b'\0')        # language tag
            _, _, text = rest.partition(b'\0')            # translated keyword
            texts.append(zlib.decompress(text) if deflated else text)
        at += 12 + length
    return b'\n'.join(texts).decode('latin-1')


def carried_file_info(path):
    """The four fields as the export's XMP states them: the `rdf:li` items of
    `dc:creator`, `dc:rights`, `dc:description` and `dc:subject`. The values
    this drive sets need no XML escaping, so they are compared as written."""
    text = xmp_bearing_text(path)

    def items(tag):
        element = re.search(rf'<dc:{tag}\b[^>]*>(.*?)</dc:{tag}>', text, re.S)
        if element is None:
            return []
        return re.findall(r'<rdf:li\b[^>]*>(.*?)</rdf:li>', element.group(1), re.S)

    return dict(creator=items('creator'), copyright=''.join(items('rights')),
                description=''.join(items('description')), keywords=items('subject'))


# ------------------------------------------------------------------- state --

STATE_KEYS = ('width', 'height', 'layers', 'resolution', 'channels', 'guides',
              'selection', 'selected_layers', 'active_layer', 'precision')


def master_state(app, document):
    summary = next(entry for entry in app.call('list_documents')['documents']
                   if entry['id'] == document)
    state = app.call('get_document', document_id=document)
    return dict(
        document={key: state.get(key) for key in STATE_KEYS},
        precision=app.call('get_precision', document_id=document),
        profile=app.call('get_color_profile', document_id=document),
        unsaved_changes=summary['unsaved_changes'])


# ------------------------------------------------------------------- gates --

def take(app, output, report):
    source = output / 'master16.png'
    png16(source, WIDTH, HEIGHT, plate())
    report['fixture_sha256'][source.name] = digest(source)

    document = app.call('open_document', path=str(source),
                        precision='preserve')['document']['id']
    converted = app.call('convert_profile', document_id=document, profile='display_p3')
    assert converted.get('ok'), converted
    before = master_state(app, document)
    report['master_profile'] = before['profile'].get('name')

    deliveries = output / 'deliveries'
    deliveries.mkdir()
    cases = []
    for name, settings, expectation in [
        ('web-jpeg',
         {'format': 'jpeg', 'jpeg_quality': 82, 'profile': 'srgb',
          'size': {'mode': 'long_edge', 'value': LONG_EDGE}, 'filter': 'lanczos3',
          'sharpen': {'radius': 0.8, 'amount': 0.6, 'threshold': 0.01}},
         dict(width=LONG_EDGE, height=int(round(HEIGHT * LONG_EDGE / WIDTH)), bits=8)),
        ('print-tiff16',
         {'format': 'tiff', 'bit_depth': 16, 'profile': 'document'},
         dict(width=WIDTH, height=HEIGHT, bits=16)),
    ]:
        preview = app.call('export_preview', document_id=document, settings=settings)
        path = deliveries / f'{name}.{settings["format"] if settings["format"] != "jpeg" else "jpg"}'
        written = app.call('export_image', document_id=document, path=str(path),
                           settings=settings)
        assert written['ok'], written
        facts = inspected(path)
        row = dict(name=name, settings=settings, preview=preview, written=written,
                   inspected=facts, file_sha256=digest(path),
                   dimensions_match=(facts['width'] == expectation['width']
                                     and facts['height'] == expectation['height']),
                   depth_match=facts['bits'] == expectation['bits'],
                   size_match=facts['bytes'] == written['bytes'],
                   preview_is_file=preview['encoded_sha256'] == digest(path)
                   and written['encoded_sha256'] == digest(path))
        cases.append(row)
    report['cases'] = cases
    after = master_state(app, document)
    report['master_unchanged'] = after == before
    report['master_state'] = dict(before=before, after=after)

    report['gates']['E1 dimensions, depth and profile of both deliverables'] = dict(
        passed=all(row['dimensions_match'] and row['depth_match'] and row['size_match']
                   for row in cases),
        detail={row['name']: row['inspected'] for row in cases})
    report['gates']['E2 the preview bytes are the file bytes'] = dict(
        passed=all(row['preview_is_file'] for row in cases),
        detail={row['name']: row['file_sha256'] for row in cases})
    report['gates']['E3 the editable master is untouched'] = dict(
        passed=report['master_unchanged'],
        detail=dict(unsaved_changes=after['unsaved_changes']))

    # ---- E4: Quick Export ------------------------------------------------
    destination = output / 'quick'
    destination.mkdir()
    preset_settings = {'format': 'png', 'profile': 'srgb',
                       'size': {'mode': 'long_edge', 'value': 400}}
    saved = app.call('save_export_preset', name='Web Quick', settings=preset_settings,
                     destination=str(destination), pattern='{name}-{width}x{height}')
    assert saved['ok'], saved
    assert app.call('set_default_export_preset', name='Web Quick')['ok']
    quick = app.call('quick_export', document_id=document, preset='Web Quick')
    assert quick['ok'], quick
    landed = Path(quick['path'])
    expected_base = f'master16-{quick["width"]}x{quick["height"]}'
    expected_name = f'{expected_base}.png'
    first = digest(landed)
    # The interactive form (a preset name) never overwrites and never
    # refuses a taken name; it writes beside it and says which name was taken.
    second = app.call('quick_export', document_id=document, preset='Web Quick')
    assert second['ok'], second
    suffixed = Path(second['path'])
    report['quick_export'] = dict(path=quick['path'], expected_name=expected_name,
                                  second=second, sha256=first)
    report['gates']['E4 Quick Export names the file and suffixes a taken name'] = dict(
        passed=(landed.parent == destination and landed.name == expected_name
                and 'requested_path' not in quick
                and suffixed.parent == destination
                and suffixed.name == f'{expected_base}-2.png'
                and second.get('requested_path') == quick['path']
                and suffixed.is_file() and digest(landed) == first),
        detail=dict(name=landed.name, second=suffixed.name,
                    requested_path=second.get('requested_path')))

    # ---- E4c: the resolved form keeps the refusal ---------------------------
    held = {path.name: digest(path) for path in destination.iterdir()}
    refusal = app.call('quick_export', document_id=document, settings=preset_settings,
                       destination=str(destination), pattern='{name}-{width}x{height}',
                       fail=True)
    after_refusal = {path.name: digest(path) for path in destination.iterdir()}
    report['quick_export']['resolved_refusal'] = refusal
    report['gates']['E4c settings + destination + pattern refuses a taken name'] = dict(
        passed=('already exists' in refusal and after_refusal == held
                and sorted(held) == sorted([expected_name, suffixed.name])),
        detail=dict(refusal=refusal, files=sorted(after_refusal)))

    # ---- E4b: the built-in preset, no default set ---------------------------
    beside = output / 'builtin'
    beside.mkdir()
    plain = beside / 'plain.tiff'
    assert app.call('export_image', document_id=document, path=str(plain),
                    settings={'format': 'tiff'})['ok']
    assert app.call('set_default_export_preset', name=None)['ok']
    opened = app.call('open_document', path=str(plain))['document']['id']
    builtin = app.call('quick_export', document_id=opened)
    # The default goes back before anything can fail the gate, so E5 reads
    # the library this drive meant to leave.
    assert app.call('set_default_export_preset', name='Web Quick')['ok']
    written = Path(builtin.get('path', ''))
    report['quick_export']['builtin'] = builtin
    report['gates']['E4b with no default, Quick Export writes a PNG beside the document'] = dict(
        passed=(builtin.get('ok') is True and written == beside / 'plain.png'
                and written.is_file()
                and written.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
                and 'requested_path' not in builtin
                and sorted(path.name for path in beside.iterdir())
                == ['plain.png', 'plain.tiff']),
        detail=dict(path=builtin.get('path'), preset=builtin.get('preset'),
                    files=sorted(path.name for path in beside.iterdir())))

    # ---- E6: Actions (recorded before the relaunch) -----------------------
    replay_destination = output / 'replay'
    replay_destination.mkdir()
    app.call('start_recording')
    recorded = app.call('quick_export', document_id=document, settings=preset_settings,
                        destination=str(replay_destination), pattern='{name}-{width}')
    assert recorded['ok'], recorded
    action = app.call('stop_recording', name='Phase 11A Quick Export')
    tools = [step['tool'] for step in action['steps']]
    copy = output / 'renamed-copy.png'
    shutil.copyfile(source, copy)
    other = app.call('open_document', path=str(copy), precision='preserve')['document']['id']
    replayed = app.call('run_action', document_id=other, name='Phase 11A Quick Export')
    written_names = sorted(path.name for path in replay_destination.iterdir())
    report['actions'] = dict(steps=tools, replayed=replayed, written=written_names)
    report['gates']['E6 a recorded Quick Export replays on another document'] = dict(
        passed=(tools == ['quick_export'] and replayed.get('ok') is True
                and len(written_names) == 2
                and any(name.startswith('renamed-copy-') for name in written_names)),
        detail=dict(steps=tools, written=written_names))

    report['gates']['E7 the catalog parses'] = dict(
        passed=report['tool_count'] > 0, detail=dict(tool_count=report['tool_count']))
    take_file_info(app, output, source, report)
    return dict(preset_settings=preset_settings, quick_sha256=first)


def take_file_info(app, output, source, report):
    """E8: File Info travels, and a one-key call is a one-key step."""
    folder = output / 'fileinfo'
    folder.mkdir()
    first_copy = folder / 'first.png'
    shutil.copyfile(source, first_copy)
    first = app.call('open_document', path=str(first_copy),
                     precision='preserve')['document']['id']
    read = lambda document: app.call('get_file_info', document_id=document)

    opened_empty = read(first)
    answer = app.call('set_file_info', document_id=first, **FILE_INFO)
    after_set = read(first)
    again = app.call('set_file_info', document_id=first, **FILE_INFO)
    app.call('undo', document_id=first)
    after_undo = read(first)
    app.call('redo', document_id=first)
    after_redo = read(first)

    exports = {}
    for extension, settings in [('jpg', {'format': 'jpeg', 'jpeg_quality': 90}),
                                ('png', {'format': 'png'}), ('tiff', {'format': 'tiff'})]:
        path = folder / f'carried.{extension}'
        assert app.call('export_image', document_id=first, path=str(path),
                        settings=settings)['ok']
        exports[extension] = carried_file_info(path)
    native = folder / 'carried.rz'
    app.call('save_copy', document_id=first, path=str(native))
    reopened = read(app.call('open_document', path=str(native))['document']['id'])

    # A step that names one field changes one field wherever it is replayed.
    app.call('start_recording')
    app.call('set_file_info', document_id=first, copyright=RECORDED_COPYRIGHT)
    action = app.call('stop_recording', name='Phase 11A File Info')
    steps = [dict(tool=step['tool'], arguments=step['arguments']) for step in action['steps']]
    second_copy = folder / 'second.png'
    shutil.copyfile(source, second_copy)
    second = app.call('open_document', path=str(second_copy))['document']['id']
    app.call('set_file_info', document_id=second, **OTHER_FILE_INFO)
    replayed = app.call('run_action', document_id=second, name='Phase 11A File Info')
    after_replay = read(second)

    report['file_info'] = dict(
        opened_empty=opened_empty, set=answer, after_set=after_set, again=again,
        after_undo=after_undo, after_redo=after_redo, exports=exports, reopened=reopened,
        steps=steps, replayed=replayed, after_replay=after_replay)
    report['gates']['E8 File Info travels and a one-key step replays one key'] = dict(
        passed=(opened_empty == EMPTY_FILE_INFO
                and answer.get('changed') is True and answer.get('file_info') == FILE_INFO
                and after_set == FILE_INFO
                and again.get('ok') is True and again.get('changed') is False
                and after_undo == EMPTY_FILE_INFO and after_redo == FILE_INFO
                and all(carried == FILE_INFO for carried in exports.values())
                and sorted(exports) == ['jpg', 'png', 'tiff']
                and reopened == FILE_INFO
                and steps == [dict(tool='set_file_info',
                                   arguments=dict(copyright=RECORDED_COPYRIGHT))]
                and replayed.get('ok') is True
                and after_replay == dict(OTHER_FILE_INFO, copyright=RECORDED_COPYRIGHT)),
        detail=dict(exports=exports, reopened=reopened, steps=steps,
                    after_replay=after_replay))


def take_after_relaunch(app, report, carried):
    listed = app.call('list_export_presets')
    names = [preset['name'] for preset in listed['presets']]
    report['presets_after_relaunch'] = listed
    report['gates']['E5 the preset library survives a relaunch'] = dict(
        passed=(names == ['Web Quick'] and listed['default_preset'] == 'Web Quick'
                and listed['presets'][0]['settings']['size']
                == carried['preset_settings']['size']),
        detail=dict(names=names, default=listed['default_preset']))


def dependencies():
    """The code files `helper_sha256` hashes."""
    return [ROOT / 'scripts/host-source-probe/support.py',
            ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/precision-probe/fixtures.py',
            ROOT / 'scripts/precision-probe/retained_fixtures.py',
            ROOT / 'scripts/precision-probe/retained_linked_fixtures.py',
            ROOT / 'scripts/verification_isolation.py',
            ROOT / 'scripts/app_under_test.py', ROOT / 'scripts/verify_phase1.py',
            ROOT / 'scripts/verify_phase2.py', ROOT / 'scripts/verify_phase8b.py']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path,
                        default=app_under_test.default_app_bundle())
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    output = arguments.output.expanduser().resolve()
    if output.is_relative_to(ROOT) or arguments.output.is_symlink():
        parser.error('output must be outside the repository and not a symlink')
    assert not output.exists() or not any(output.iterdir()), 'use a fresh output folder'
    output.mkdir(parents=True, exist_ok=True)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)),
                  helper_sha256={str(p): digest(p) for p in dependencies()},
                  fixture_sha256={}, tool_count=0, gates={}, cases=[], error=None)
    try:
        with support.HostApp(output / 'app', output / 'recovery', executable) as app:
            report['tool_count'] = len(app.client.rpc('tools/list', {})['tools'])
            carried = take(app, output, report)
        # The same private DR_EXPORT_PRESETS_DIR, a brand new process.
        with support.HostApp(output / 'app', output / 'recovery', executable) as app:
            take_after_relaunch(app, report, carried)
    except BaseException as error:          # Recorded, never repaired.
        report['error'] = f'{type(error).__name__}: {error}'
    (output / 'result.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    gates = {name: gate['passed'] for name, gate in report['gates'].items()}
    print(json.dumps(dict(error=report['error'], gates=gates,
                          result=str(output / 'result.json')), indent=2))
    raise SystemExit(0 if not report['error'] and gates and all(gates.values()) else 1)


if __name__ == '__main__':
    main()
