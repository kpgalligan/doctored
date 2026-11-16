#!/usr/bin/env python3
"""MCP drive for zipped Action bundles (`package_action {format: "zip"}`, `import_action` of a
`.draction.zip`), against the Debug app.

- **Written**: an action with a `place_image` step packages to ONE `<slug>.draction.zip`; Python's
  `zipfile` (the independent reader) lists exactly `<slug>.draction/action.json` and
  `<slug>.draction/assets/logo.png`, the asset's bytes equal the source file's, and the packaged
  `action.json` carries `{"$asset": "logo.png"}`. A second package into another folder is byte
  equal. The folder form is unchanged.
- **Read**: importing the zip under a new name installs it, and running it renders exactly what
  the original action renders. An archive Python writes with the bundle at its ROOT plus Finder's
  `__MACOSX/` and `.DS_Store` junk imports and installs only the real asset. An archive whose entry
  is `../<unique>.txt` is refused by sentence and that file appears nowhere.

The app is this script's own child (`phase5_support.App`). Prints one PASS line per check;
exits non-zero unless every line is PASS.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
os.environ.setdefault('DR_BACKGROUND', '1')
from phase5_support import App  # noqa: E402
from verify_phase1 import png  # noqa: E402

RESULTS = []
NAME = 'Zip Bundle Drive'
SLUG = 'zip-bundle-drive'


def ok(label):
    RESULTS.append(True)
    print(f'PASS: {label}', flush=True)


def check(label, function):
    try:
        function()
        ok(label)
    except Exception as error:  # one failed check must not hide the others
        RESULTS.append(False)
        print(f'FAIL: {label} raised {type(error).__name__} — {str(error)[:700]}', flush=True)


def digest(app, doc):
    reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': {'document_id': doc}})
    assert not reply.get('isError'), reply
    data = next(c['data'] for c in reply['content'] if c['type'] == 'image')
    return hashlib.sha256(data.encode()).hexdigest()


def run(app, folder):
    logo = folder / 'logo.png'
    pixels = bytes(v for y in range(24) for x in range(32)
                   for v in ((x * 8) % 256, (y * 10) % 256, 160, 255))
    png(logo, 32, 24, pixels)
    app.call('save_action', name=NAME, overwrite=True, steps=[
        {'tool': 'place_image', 'arguments': {'path': str(logo)}}])
    first, second = folder / 'bundles-a', folder / 'bundles-b'
    first.mkdir()
    second.mkdir()
    state = {}

    def written():
        reply = app.call('package_action', name=NAME, destination=str(first), format='zip')
        archive = Path(reply['path'])
        assert archive.name == f'{SLUG}.draction.zip' and archive.is_file(), reply
        with zipfile.ZipFile(archive) as zipped:
            names = sorted(zipped.namelist())
            assert names == [f'{SLUG}.draction/action.json',
                             f'{SLUG}.draction/assets/logo.png'], names
            assert zipped.read(f'{SLUG}.draction/assets/logo.png') == logo.read_bytes()
            action = json.loads(zipped.read(f'{SLUG}.draction/action.json'))
            assert zipped.testzip() is None
        assert action['steps'][0]['arguments']['path'] == {'$asset': 'logo.png'}, action
        again = Path(app.call('package_action', name=NAME, destination=str(second),
                              format='zip')['path'])
        assert again.read_bytes() == archive.read_bytes(), 'a second package differs'
        refusal = app.client.call('package_action', fail=True, name=NAME,
                                  destination=str(first), format='zip')
        assert 'already exists' in refusal, refusal
        refusal = app.client.call('package_action', fail=True, name=NAME,
                                  destination=str(first), format='tar')
        assert 'format must be' in refusal, refusal
        folder_reply = app.call('package_action', name=NAME, destination=str(first))
        assert Path(folder_reply['path']).is_dir(), folder_reply
        state['archive'] = archive
    check('package_action {format: "zip"} writes one archive zipfile reads, byte equal twice; '
          'the folder form is unchanged', written)

    def round_trip():
        archive = state['archive']
        imported = app.call('import_action', path=str(archive), name='Zip Bundle Imported')
        assert imported['assets'] == ['logo.png'], imported
        renders = []
        for action in (NAME, 'Zip Bundle Imported'):
            doc = app.call('new_document', width=64, height=48, background='white')['document']['id']
            ran = app.call('run_action', document_id=doc, name=action)
            assert ran.get('ok') is True, ran
            renders.append(digest(app, doc))
            app.call('close_document', document_id=doc, discard_changes=True)
        assert renders[0] == renders[1], renders
    check('import_action of the zip installs it and it renders as the original', round_trip)

    def root_level():
        archive = folder / 'root-level.zip'
        action = {'doctored_action': 2, 'name': 'Root Level', 'steps': [
            {'tool': 'place_image', 'arguments': {'path': {'$asset': 'logo.png'}}}]}
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as zipped:
            zipped.writestr('action.json', json.dumps(action))
            zipped.writestr('assets/logo.png', logo.read_bytes())
            zipped.writestr('__MACOSX/._action.json', b'apple double')
            zipped.writestr('.DS_Store', b'finder')
            zipped.writestr('assets/.hidden', b'dot file')
        imported = app.call('import_action', path=str(archive))
        assert imported['name'] == 'Root Level' and imported['assets'] == ['logo.png'], imported
        assert 'missing_assets' not in imported, imported
    check('an archive with the bundle at its root and Finder junk imports the real asset only',
          root_level)

    def evil():
        target = f'doctored-zip-drive-{uuid.uuid4().hex}.txt'
        archive = folder / 'evil.zip'
        with zipfile.ZipFile(archive, 'w') as zipped:
            zipped.writestr('evil.draction/action.json', json.dumps(
                {'doctored_action': 1, 'name': 'Evil', 'steps': []}))
            zipped.writestr(f'../{target}', b'escaped')
        refusal = app.client.call('import_action', fail=True, path=str(archive))
        assert 'not a relative path inside the archive' in refusal, refusal
        for place in (Path(tempfile.gettempdir()), folder, folder.parent):
            assert not (place / target).exists(), place
        names = [row['name'] for row in app.call('list_actions')['actions']]
        assert 'Evil' not in names, names
    check('an archive entry naming ../ is refused by sentence and lands nowhere', evil)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-bundle-zip-'))).expanduser()
    output = output.resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    work = output / 'files'
    work.mkdir(exist_ok=True)
    with App(output / 'app', output / 'recovery', interval=600) as app:
        run(app, work)
    raise SystemExit(0 if RESULTS and all(RESULTS) else 1)


if __name__ == '__main__':
    main()
