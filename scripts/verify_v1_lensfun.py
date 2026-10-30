#!/usr/bin/env python3
"""MCP drive for Lensfun lens profiles (v1-lensfun): Filter > Lens Correction's
`profile` (`apply_filter` lens_correction, `list_lens_profiles`) on a JPEG, and
Develop RAW's Enable Profile Corrections with a Lensfun profile when the decoder
carries none.

The script owns its one app instance: the Debug app (or `DR_VERIFY_APP`) on
its own scratch MCP socket with `DR_BACKGROUND=1`, a
dummy `ANTHROPIC_API_KEY`, an 8 GiB working limit and private `DR_*_DIR`s
(`verify_v1_lens.App`). It terminates only that process. The fixtures are
`~/devel/doctored-fixtures/camera/camera.NEF` and the `raw-pixls` camera
files; every RAW is opened from its own copy in its own folder.

Pixels are compared by the SHA-256 of a `save_copy` (PNG for the 8-bit
documents, TIFF for the 16-bit RAW developments, read with
`verify_v1_raw_presence.read_tiff` where codes are compared).

Cases:
- CREDITS: the app bundle's Credits.html and Acknowledgements.txt both name
  Lensfun and CC BY-SA 3.0.
- SURVEY: every RAW fixture opens and reports `lens_profile` with `source`
  ("decoder", "lensfun" or null) and `lensfun`; at least one is "lensfun".
  When none is, the files and the camera and lens strings checked are printed.
- RAW: the first "lensfun" fixture (camera.NEF, else D850, else any): its
  development differs from one opened with `lens_correction: false`, whose
  source is null; an exposure-only `redevelop_raw` keeps `get_raw_source`'s
  `lens_profile.source` "lensfun" and still differs from the uncorrected
  development redeveloped the same way; a `.rz` round trip keeps the recipe and
  the pixels; a tone local confined to the centre leaves the top and bottom
  strips (8% of the height each) within 1 code of the render before it (the
  Lensfun stage is not applied twice).
- JPEG: Z6.NEF opened at 8 bits without profile corrections, scaled to a
  quarter and saved as a JPEG (its EXIF keeps the camera, lens and the frame
  size). On it: `list_lens_profiles` selects NIKKOR Z 24-70mm f/4 S;
  `apply_filter` lens_correction with profile auto changes the pixels in one
  undo step and undo restores them; the lens id gives the same pixels as auto;
  "none" changes nothing; an unknown id is refused naming it; a new document
  with no EXIF refuses auto; the JPEG cropped (deleting the cropped pixels) to
  a square refuses auto with the frame reason, and the lens id still applies.

Usage: python3 scripts/verify_v1_lensfun.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verify_v1_lens import App, SlowClient  # noqa: E402
from verify_v1_raw_presence import read_tiff  # noqa: E402

FIXTURES = Path.home() / 'devel/doctored-fixtures'
RAW_FIXTURES = [
    ('camera.NEF', FIXTURES / 'camera/camera.NEF'),
    ('D850', FIXTURES / 'raw-pixls/nikon-d850/D850.NEF'),
    ('Z6', FIXTURES / 'raw-pixls/nikon-z6/Z6.NEF'),
    ('R5', FIXTURES / 'raw-pixls/canon-r5/R5.CR3'),
    ('K1M2', FIXTURES / 'raw-pixls/pentax-k1m2/K1D.DNG'),
    ('A7M3', FIXTURES / 'raw-pixls/sony-a7m3/A73.ARW'),
    ('A7RM3', FIXTURES / 'raw-pixls/sony-a7rm3/A7R3.ARW'),
    ('A7RM4', FIXTURES / 'raw-pixls/sony-a7rm4/A7R4.ARW'),
]
Z6 = FIXTURES / 'raw-pixls/nikon-z6/Z6.NEF'
Z6_LENS = 'NIKKOR Z 24-70mm f/4 S'
LIMIT = 8 * 1024 ** 3
STRIP = 0.08


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Run:
    def __init__(self, app, folder):
        self.app = app
        self.folder = Path(folder)
        self.serial = 0
        self.survey = {}

    def next_path(self, tag, suffix):
        self.serial += 1
        return self.folder / f'{self.serial:02d}-{tag}{suffix}'

    def open_raw(self, tag, source, **arguments):
        folder = self.next_path(tag, '')
        folder.mkdir(parents=True)
        path = folder / source.name
        shutil.copyfile(source, path)
        reply = self.app.call('open_document', path=str(path), **arguments)
        return reply['document']['id'], reply

    def close(self, document):
        self.app.call('close_document', document_id=document, discard_changes=True)

    def raw_source(self, document):
        return self.app.call('get_raw_source', document_id=document, layer=0)['source']

    def save(self, document, tag, suffix, fmt):
        path = self.next_path(tag, suffix)
        self.app.call('save_copy', document_id=document, path=str(path), format=fmt)
        return path

    def cursor(self, document):
        history = self.app.call('list_history', document_id=document)
        return history['cursor'], history['undo_count'], history['redo_count']


# ----------------------------------------------------------------- cases --

def credits_case(run):
    executable = app_under_test.default_app_executable()
    resources = executable.parents[1] / 'Resources'
    found = {}
    for name in ('Credits.html', 'Acknowledgements.txt'):
        text = (resources / name).read_text()
        for needle in ('Lensfun', 'CC BY-SA 3.0', 'https://creativecommons.org/licenses/by-sa/3.0/'):
            assert needle in text, (name, needle)
        found[name] = len(text)
    return found


def survey_case(run):
    rows = {}
    for label, path in RAW_FIXTURES:
        if not path.is_file():
            rows[label] = dict(missing=str(path))
            continue
        document, reply = run.open_raw(f'survey-{label}', path)
        profile = reply.get('lens_profile')
        assert isinstance(profile, dict) and 'source' in profile and 'lensfun' in profile, (label, reply)
        assert profile['source'] in ('decoder', 'lensfun', None), profile
        rows[label] = profile
        run.close(document)
    run.survey = rows
    for label, row in rows.items():
        print(f'  {label:10s} {json.dumps(row)}', flush=True)
    lensfun = [label for label, row in rows.items() if row.get('source') == 'lensfun']
    if not lensfun:
        checked = {label: dict(file=str(path), lens=rows[label].get('lens')) for label, path in RAW_FIXTURES}
        print('  no fixture took a Lensfun profile; checked: ' + json.dumps(checked), flush=True)
    assert lensfun, 'no RAW fixture developed with a Lensfun profile'
    return rows


def raw_case(run):
    lensfun = [label for label, row in run.survey.items() if row.get('source') == 'lensfun']
    assert lensfun, 'the survey found no Lensfun fixture'
    label = next((name for name in ('camera.NEF', 'D850') if name in lensfun), lensfun[0])
    source = dict(RAW_FIXTURES)[label]
    corrected, reply = run.open_raw('lensfun', source)
    assert reply['lens_profile']['source'] == 'lensfun', reply['lens_profile']
    plain, reply = run.open_raw('no-lens', source, raw=dict(lens_correction=False))
    assert reply['lens_profile']['source'] is None, reply['lens_profile']
    assert reply['lens_profile']['enabled'] is False, reply['lens_profile']
    on = run.save(corrected, 'lensfun', '.tiff', 'tiff')
    off = run.save(plain, 'no-lens', '.tiff', 'tiff')
    assert digest(on) != digest(off), 'the Lensfun development equals the uncorrected one'
    report = run.raw_source(corrected)['lens_profile']
    assert report['source'] == 'lensfun' and report['lensfun']['id'], report

    # An exposure-only redevelop keeps the Lensfun toggle.
    for document in (corrected, plain):
        reply = run.app.call('redevelop_raw', document_id=document, layer=0, raw=dict(exposure=0.3))
        assert reply['ok'] is True, reply
    after = run.raw_source(corrected)
    assert after['lens_profile']['source'] == 'lensfun', after['lens_profile']
    assert after['raw']['recipe']['values']['lensCorrection'] is True, after['raw']['recipe']['values']
    assert run.raw_source(plain)['lens_profile']['source'] is None
    on2 = run.save(corrected, 'lensfun-exposure', '.tiff', 'tiff')
    off2 = run.save(plain, 'no-lens-exposure', '.tiff', 'tiff')
    assert digest(on2) != digest(off2), 'after redevelop the Lensfun development equals the uncorrected one'
    assert digest(on2) != digest(on), 'the exposure redevelop changed nothing'
    run.close(plain)

    # .rz round trip.
    recipe = after['raw']['recipe']
    path = run.next_path('lensfun', '.rz')
    run.app.call('save_copy', document_id=corrected, path=str(path))
    reopened = run.app.call('open_document', path=str(path))['document']['id']
    again = run.raw_source(reopened)
    assert again['raw']['recipe'] == recipe, 'the recipe differs after the reopen'
    assert again['lens_profile']['source'] == 'lensfun', again['lens_profile']
    assert digest(run.save(reopened, 'rz-after', '.tiff', 'tiff')) == digest(on2), 'the pixels differ after the reopen'
    reply = run.app.call('redevelop_raw', document_id=reopened, layer=0, raw=dict(exposure=0.0))
    assert reply['ok'] is True, reply
    run.close(reopened)

    # A centre-confined local: the border strips stay within one code.
    width, height, spp, depth, before = read_tiff(on2)
    radial = dict(op='radial', center=[width / 2, height / 2], radius=[width * 0.12, height * 0.12],
                  hardness=0.8)
    reply = run.app.call('edit_raw_development', document_id=corrected, layer=0, operations=[
        dict(op='create', id='centre', tone=dict(exposure=1), mask=radial)])
    assert reply.get('unchanged') is False, reply
    assert run.raw_source(corrected)['lens_profile']['source'] == 'lensfun'
    local = run.save(corrected, 'lensfun-local', '.tiff', 'tiff')
    w2, h2, spp2, depth2, after_local = read_tiff(local)
    assert (w2, h2, spp2, depth2) == (width, height, spp, depth), (w2, h2, spp2, depth2)
    strip = max(1, int(height * STRIP))
    worst = 0
    for rows in (range(0, strip), range(height - strip, height)):
        for y in rows:
            base = y * width * spp
            for i in range(base, base + width * spp):
                if spp == 4 and i % 4 == 3:
                    continue
                worst = max(worst, abs(after_local[i] - before[i]))
    assert worst <= 1, f'the border strips moved by {worst} codes after a centre local'
    centre = ((height // 2) * width + width // 2) * spp
    assert after_local[centre + 1] > before[centre + 1], 'the centre local changed nothing'
    run.close(corrected)
    return dict(fixture=label, border_worst=worst, recipe_version=recipe.get('version'))


def jpeg_case(run):
    assert Z6.is_file(), f'missing {Z6}'
    source, reply = run.open_raw('z6', Z6, precision='rgba8', raw=dict(lens_correction=False))
    info = run.app.call('get_document', document_id=source)
    width, height = info['width'], info['height']
    small = (width // 4, height // 4)
    run.app.call('image_size', document_id=source, width=small[0], height=small[1])
    jpeg = run.save(source, 'z6', '.jpg', 'jpeg')
    run.close(source)

    document = run.app.call('open_document', path=str(jpeg))['document']['id']
    listing = run.app.call('list_lens_profiles', document_id=document, all=True)
    assert listing['selected']['id'] == Z6_LENS, listing
    assert listing['profile'] is not None, listing
    assert listing['identity']['pixel_width'] == small[0], listing['identity']
    assert listing['database']['license'] == 'CC BY-SA 3.0', listing['database']
    assert 'Lensfun v0.3.4' in listing['summary'], listing['summary']
    original = digest(run.save(document, 'jpeg-original', '.png', 'png'))
    before = run.cursor(document)
    run.app.call('apply_filter', document_id=document, filter='lens_correction', profile='auto')
    assert run.cursor(document)[0] == before[0] + 1, (before, run.cursor(document))
    auto = digest(run.save(document, 'jpeg-auto', '.png', 'png'))
    assert auto != original, 'profile auto changed nothing'
    run.app.call('undo', document_id=document)
    assert digest(run.save(document, 'jpeg-undone', '.png', 'png')) == original, 'undo did not restore'
    run.app.call('apply_filter', document_id=document, filter='lens_correction', profile=Z6_LENS)
    assert digest(run.save(document, 'jpeg-id', '.png', 'png')) == auto, 'the lens id differs from auto'
    run.app.call('undo', document_id=document)
    cursor = run.cursor(document)
    run.app.call('apply_filter', document_id=document, filter='lens_correction', profile='none')
    assert run.cursor(document) == cursor, 'profile none added a step'
    assert digest(run.save(document, 'jpeg-none', '.png', 'png')) == original, 'profile none changed pixels'
    texts = [run.app.refused('apply_filter', 'unknown lens id No Such Lens', document_id=document,
                             filter='lens_correction', profile='No Such Lens')]

    blank = run.app.call('new_document', width=300, height=200)['document']['id']
    texts.append(run.app.refused('apply_filter', 'No Lensfun profile matches this document',
                                 document_id=blank, filter='lens_correction', profile='auto'))
    run.close(blank)

    side = min(small)
    run.app.call('crop', document_id=document, x=0, y=0, width=side, height=side, delete_cropped=True)
    texts.append(run.app.refused('apply_filter', 'the image is not the camera frame',
                                 document_id=document, filter='lens_correction', profile='auto'))
    run.app.call('apply_filter', document_id=document, filter='lens_correction', profile=Z6_LENS)
    run.close(document)
    return dict(size=list(small), refusals=texts, lens=listing['selected'])


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, the copies and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-lensfun-'))
    output.mkdir(parents=True, exist_ok=True)

    results, failed = {}, []

    def run_case(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', LIMIT, SlowClient) as app:
        run = Run(app, output)
        run_case('credits', lambda: credits_case(run))
        run_case('survey', lambda: survey_case(run))
        run_case('raw', lambda: raw_case(run))
        run_case('jpeg', lambda: jpeg_case(run))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
