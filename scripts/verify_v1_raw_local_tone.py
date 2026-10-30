#!/usr/bin/env python3
"""MCP drive for the Camera RAW local tone path ("More local sliders" and
"More than 4 locals"): `edit_raw_development` operations carrying `tone`,
on the real NEF.

The script owns its one app instance: the Debug app (or `DR_VERIFY_APP`) on
its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, an 8 GiB working limit and private `DR_*_DIR`s
(`verify_v1_lens.App`). It terminates only that process. The fixture is
`~/devel/doctored-fixtures/camera/camera.NEF`; an absent fixture FAILS the
run. Every document is opened from its own copy of the file, at 16 bits.

What is measured. A tone local adjusts the global development inside its
coverage, so a toned document is compared with a PLAIN development of the same
file, point by point, on a lattice read with `sample_pixel` (`source: layer`).

Bounds, fixed before the first run and never widened:
- the lattice is 32×20 (640 points);
- a +60 Contrast radial (hardness 0.9): every point outside the ellipse
  (normalized distance > 1.01, coverage 0) is code-identical to the plain
  development; in the hard core (distance < 0.89) every UNCLIPPED point
  (every plain code in 256…65279) moves each channel away from 32768 or stays
  within 1 code of it (|new − 32768| ≥ |old − 32768| − 1), and at least
  MIN_MOVED = 20 points move some channel by at least MOVED = 64 codes;
- an all-zero tone is `unchanged` and leaves the recipe at version 1;
- 16 tone locals are accepted, a 17th and a 5th camera local are refused; one
  call of four `update`s setting `exposure_offset` 1 on four of the sixteen is
  admitted, develops and reports four camera locals;
- undo/redo, a preset, redevelop_raw and an `.rz` round trip keep the tone
  (get_raw_locals equal) and, where the development is the same, the lattice
  codes;
- a local with only `exposure_offset` stays version 1 and reopens from `.rz`
  code-identical;
- DETAIL: a radial (hardness 0.9) with tone {sharpness 80,
  noise_reduction 40}: every point outside the ellipse is code-identical to
  the plain development and at least MIN_MOVED core points changed some
  channel; get_raw_locals reports the tone with all eleven keys; undo restores
  the plain lattice and redo the detailed one; an `.rz` round trip keeps
  get_raw_locals and the lattice codes.

Usage: python3 scripts/verify_v1_raw_local_tone.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import json
from pathlib import Path
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_v1_lens import App, SlowClient  # noqa: E402

FIXTURE = Path.home() / 'devel/doctored-fixtures/camera/camera.NEF'
LIMIT = 8 * 1024 ** 3

LATTICE = (32, 20)
MOVED = 64
MIN_MOVED = 20
MIN_CLASS = 20
HARDNESS = 0.9
REDEVELOP_EXPOSURE = -0.25
TONE_KEYS = {'exposure', 'contrast', 'highlights', 'shadows', 'whites', 'blacks', 'saturation',
             'clarity', 'texture', 'sharpness', 'noise_reduction'}


def unclipped(codes):
    return all(256 <= c <= 65279 for c in codes[:3])


def moved(a, b):
    return max(abs(p - q) for p, q in zip(a[:3], b[:3]))


def tone(**values):
    full = {key: 0 for key in TONE_KEYS}
    full.update(values)
    return full


class Run:
    def __init__(self, app, folder):
        self.app = app
        self.folder = Path(folder)
        self.serial = 0
        self.size = None
        self.points = None
        self.plain = None
        self.plain_lattice = None
        self.toned = None
        self.toned_lattice = None
        self.classes = None

    def copy(self, tag):
        self.serial += 1
        path = self.folder / f'{self.serial:02d}-{tag}.NEF'
        shutil.copyfile(FIXTURE, path)
        return path

    def open(self, tag, **development):
        arguments = dict(path=str(self.copy(tag)), precision='preserve')
        if development:
            arguments['raw'] = development
        document = self.app.call('open_document', **arguments)['document']
        if self.size is None:
            self.size = (document['width'], document['height'])
            columns, rows = LATTICE
            self.points = [(int((i + 0.5) * self.size[0] / columns),
                            int((j + 0.5) * self.size[1] / rows))
                           for j in range(rows) for i in range(columns)]
        assert (document['width'], document['height']) == self.size, document
        return document['id']

    def close(self, document):
        self.app.call('close_document', document_id=document, discard_changes=True)

    def source(self, document):
        return self.app.call('get_raw_source', document_id=document, layer=0)['source']

    def locals(self, document):
        return self.app.call('get_raw_locals', document_id=document, layer=0)

    def lattice(self, document):
        return [self.app.call('sample_pixel', document_id=document, source='layer', layer=0,
                              x=x, y=y)['center_rgba_codes'] for x, y in self.points]

    def edit(self, document, *operations, **raw):
        arguments = dict(document_id=document, layer=0, operations=list(operations))
        if raw:
            arguments['raw'] = raw
        reply = self.app.call('edit_raw_development', **arguments)
        assert reply['ok'] is True, reply
        return reply


def enhancement_version(source):
    enhancements = source['raw']['recipe'].get('enhancements')
    return None if enhancements is None else enhancements['version']


def radial(run):
    width, height = run.size
    return dict(center=[width / 2, height / 2], radius=[width * 0.4, height * 0.4])


def ellipse_classes(run):
    """Lattice indices outside the ellipse (coverage 0) and in its hard core."""
    shape = radial(run)
    outside, core = [], []
    for index, (x, y) in enumerate(run.points):
        d = (((x + 0.5 - shape['center'][0]) / shape['radius'][0]) ** 2
             + ((y + 0.5 - shape['center'][1]) / shape['radius'][1]) ** 2) ** 0.5
        if d > 1.01:
            outside.append(index)
        elif d < HARDNESS - 0.01:
            core.append(index)
    return outside, core


def assert_contrast(lattice, plain, outside, core, label):
    for index in outside:
        assert lattice[index] == plain[index], (label, 'outside', index, lattice[index], plain[index])
    checked, strong = 0, 0
    for index in core:
        if not unclipped(plain[index]):
            continue
        checked += 1
        for new, old in zip(lattice[index][:3], plain[index][:3]):
            assert abs(new - 32768) >= abs(old - 32768) - 1, (label, 'toward mid-grey', index,
                                                               lattice[index], plain[index])
        if moved(lattice[index], plain[index]) >= MOVED:
            strong += 1
    assert len(outside) >= MIN_CLASS and strong >= MIN_MOVED, (label, len(outside), checked, strong)
    return checked, strong


# ----------------------------------------------------------------- cases --

def setup(run):
    run.plain = run.open('plain')
    source = run.source(run.plain)
    assert source['depth'] == 16, source
    run.plain_lattice = run.lattice(run.plain)
    run.classes = ellipse_classes(run)
    return dict(size=list(run.size), outside=len(run.classes[0]), core=len(run.classes[1]),
                unclipped=sum(unclipped(codes) for codes in run.plain_lattice))


def contrast_case(run):
    run.toned = run.open('contrast')
    assert run.lattice(run.toned) == run.plain_lattice, 'two plain developments differ'
    reply = run.edit(run.toned, dict(op='create', id='contrast1', tone=dict(contrast=60),
                                     mask=dict(op='radial', hardness=HARDNESS, **radial(run))))
    assert reply['unchanged'] is False, reply
    reported = run.locals(run.toned)
    assert reported['maximum_locals'] == 16 and reported['maximum_camera_locals'] == 4, reported
    node = reported['nodes'][0]
    assert node['tone'] == tone(contrast=60), node
    assert node['exposure_offset'] == 0 and node['white_balance'] == 'inherit', node
    assert enhancement_version(run.source(run.toned)) == 3
    run.toned_lattice = run.lattice(run.toned)
    checked, strong = assert_contrast(run.toned_lattice, run.plain_lattice, *run.classes, 'contrast')
    return dict(outside_exact=len(run.classes[0]), core_unclipped=checked, core_moved=strong)


def zero_case(run):
    document = run.open('zero')
    run.edit(document, dict(op='create', id='zero1', mask=dict(op='full')))
    assert enhancement_version(run.source(document)) == 1
    lattice = run.lattice(document)
    reply = run.edit(document, dict(op='update', id='zero1', tone=tone()))
    assert reply['unchanged'] is True, reply
    assert enhancement_version(run.source(document)) == 1
    assert 'tone' not in run.locals(document)['nodes'][0], run.locals(document)
    reply = run.edit(document, dict(op='update', id='zero1', tone=dict(exposure=0, texture=0)))
    assert reply['unchanged'] is True, reply
    assert run.lattice(document) == lattice
    run.close(document)
    return dict(version=1)


def sixteen_case(run):
    document = run.open('sixteen')
    width, height = run.size
    creates = []
    for index in range(16):
        column, row = index % 4, index // 4
        center = [width * (column + 0.5) / 4, height * (row + 0.5) / 4]
        creates.append(dict(op='create', id=f'tone{index}', tone=dict(contrast=20, saturation=10),
                            mask=dict(op='radial', center=center, radius=[width / 7, height / 7],
                                      hardness=0.5)))
    reply = run.edit(document, *creates)
    assert reply['unchanged'] is False, reply
    nodes = run.locals(document)['nodes']
    assert len(nodes) == 16 and all(n['tone'] == tone(contrast=20, saturation=10) for n in nodes)
    assert enhancement_version(run.source(document)) == 3
    seventeenth = run.app.refused(
        'edit_raw_development', 'sixteen', document_id=document, layer=0,
        operations=[dict(op='create', id='tone16', tone=dict(contrast=20), mask=dict(op='full'))])
    assert len(run.locals(document)['nodes']) == 16
    lattice = run.lattice(document)
    reply = run.edit(document, *[dict(op='update', id=f'tone{index}', exposure_offset=1)
                                 for index in range(4)])
    assert reply['unchanged'] is False, reply
    nodes = run.locals(document)['nodes']
    cameras = [n['id'] for n in nodes if n['exposure_offset'] != 0 or n['white_balance'] != 'inherit']
    assert cameras == ['tone0', 'tone1', 'tone2', 'tone3'], cameras
    assert run.lattice(document) != lattice, 'the camera locals changed nothing'
    fifth = run.app.refused(
        'edit_raw_development', 'at most four locals that change white balance or camera exposure',
        document_id=document, layer=0, operations=[dict(op='update', id='tone4', exposure_offset=1)])
    assert [n['exposure_offset'] for n in run.locals(document)['nodes']] == [1] * 4 + [0] * 12
    run.close(document)
    return dict(seventeenth=seventeenth, fifth=fifth, cameras=cameras)


def undo_case(run):
    run.app.call('undo', document_id=run.toned)
    assert run.locals(run.toned)['nodes'] == [], 'undo did not remove the local'
    assert run.lattice(run.toned) == run.plain_lattice, 'undo did not restore the plain lattice'
    run.app.call('redo', document_id=run.toned)
    assert run.locals(run.toned)['nodes'][0]['tone'] == tone(contrast=60)
    assert run.lattice(run.toned) == run.toned_lattice, 'redo did not restore the contrast lattice'
    return dict(steps=2)


def preset_case(run):
    name = 'v1 tone exposure'
    saved = run.app.call('save_raw_preset', name=name, raw=dict(exposure=0.3), replace=True)
    assert saved['ok'] is True, saved
    before = run.locals(run.toned)
    applied = run.app.call('apply_raw_preset', name=name, document_id=run.toned, layer=0)
    assert applied['ok'] is True, applied
    source = run.source(run.toned)
    assert abs(source['raw']['recipe']['values']['exposure'] - 0.3) < 1e-6, source['raw']['recipe']
    assert enhancement_version(source) == 3
    assert run.locals(run.toned) == before, 'apply_raw_preset changed the locals'
    return dict(tone=before['nodes'][0]['tone'])


def redevelop_case(run):
    before = run.locals(run.toned)
    reply = run.app.call('redevelop_raw', document_id=run.toned, layer=0,
                         raw=dict(exposure=REDEVELOP_EXPOSURE))
    assert reply['ok'] is True, reply
    assert run.locals(run.toned) == before, 'redevelop_raw changed the locals'
    reference = run.open('plain-exposure', exposure=REDEVELOP_EXPOSURE)
    plain = run.lattice(reference)
    run.close(reference)
    assert plain != run.plain_lattice, 'the exposure did not change the plain development'
    run.toned_lattice = run.lattice(run.toned)
    checked, strong = assert_contrast(run.toned_lattice, plain, *run.classes, 'after redevelop_raw')
    return dict(core_unclipped=checked, core_moved=strong)


def rz_case(run):
    before = run.locals(run.toned)
    path = run.folder / 'toned.rz'
    run.app.call('save_copy', document_id=run.toned, path=str(path))
    run.close(run.toned)
    reopened = run.app.call('open_document', path=str(path))['document']['id']
    assert run.locals(reopened) == before, 'get_raw_locals differs after the reopen'
    assert enhancement_version(run.source(reopened)) == 3
    assert run.lattice(reopened) == run.toned_lattice, 'the lattice codes differ after the reopen'
    reply = run.edit(reopened, dict(op='update', id='contrast1', tone=dict(contrast=30)))
    assert reply['unchanged'] is False, reply
    assert run.locals(reopened)['nodes'][0]['tone'] == tone(contrast=30)
    run.close(reopened)
    return dict(bytes=path.stat().st_size)


def camera_only_case(run):
    document = run.open('camera-only')
    run.edit(document, dict(op='create', id='camera1', exposure_offset=-1,
                            mask=dict(op='radial', hardness=HARDNESS, **radial(run))))
    assert enhancement_version(run.source(document)) == 1
    assert 'tone' not in run.locals(document)['nodes'][0]
    lattice = run.lattice(document)
    assert lattice != run.plain_lattice
    path = run.folder / 'camera-only.rz'
    run.app.call('save_copy', document_id=document, path=str(path))
    run.close(document)
    reopened = run.app.call('open_document', path=str(path))['document']['id']
    assert enhancement_version(run.source(reopened)) == 1
    assert run.lattice(reopened) == lattice, 'the exposure_offset local reopened differently'
    run.close(reopened)
    return dict(version=1)


def detail_case(run):
    document = run.open('detail')
    assert run.lattice(document) == run.plain_lattice, 'two plain developments differ'
    values = dict(sharpness=80, noise_reduction=40)
    reply = run.edit(document, dict(op='create', id='detail1', tone=values,
                                    mask=dict(op='radial', hardness=HARDNESS, **radial(run))))
    assert reply['unchanged'] is False, reply
    node = run.locals(document)['nodes'][0]
    assert node['tone'] == tone(**values) and len(node['tone']) == 11, node
    assert enhancement_version(run.source(document)) == 3
    lattice = run.lattice(document)
    outside, core = run.classes
    for index in outside:
        assert lattice[index] == run.plain_lattice[index], ('outside', index, lattice[index])
    changed = sum(lattice[index] != run.plain_lattice[index] for index in core)
    assert len(outside) >= MIN_CLASS and changed >= MIN_MOVED, (len(outside), len(core), changed)
    run.app.call('undo', document_id=document)
    assert run.locals(document)['nodes'] == [], 'undo did not remove the local'
    assert run.lattice(document) == run.plain_lattice, 'undo did not restore the plain lattice'
    run.app.call('redo', document_id=document)
    assert run.lattice(document) == lattice, 'redo did not restore the detailed lattice'
    before = run.locals(document)
    path = run.folder / 'detail.rz'
    run.app.call('save_copy', document_id=document, path=str(path))
    run.close(document)
    reopened = run.app.call('open_document', path=str(path))['document']['id']
    assert run.locals(reopened) == before, 'get_raw_locals differs after the reopen'
    assert run.lattice(reopened) == lattice, 'the lattice codes differ after the reopen'
    run.close(reopened)
    return dict(outside_exact=len(outside), core_changed=changed)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, the copies and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-raw-local-tone-'))
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

    if not FIXTURE.is_file():
        results['fixture'] = dict(passed=False, error=f'missing {FIXTURE}')
        failed.append('fixture')
        print(f'FAIL fixture: missing {FIXTURE}', flush=True)
    else:
        with App(output / 'instance', LIMIT, SlowClient) as app:
            run = Run(app, output)
            run_case('plain reference', lambda: setup(run))
            run_case('contrast radial', lambda: contrast_case(run))
            run_case('zero tone', lambda: zero_case(run))
            run_case('sixteen locals', lambda: sixteen_case(run))
            run_case('undo', lambda: undo_case(run))
            run_case('preset', lambda: preset_case(run))
            run_case('redevelop', lambda: redevelop_case(run))
            run_case('rz', lambda: rz_case(run))
            run_case('camera only', lambda: camera_only_case(run))
            run_case('detail', lambda: detail_case(run))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
