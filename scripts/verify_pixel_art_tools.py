#!/usr/bin/env python3
"""MCP drive for the pixel-art tools: `put_pixels`, `close_document` and
`render`'s integer `scale`.

The script owns its app instance (`HostApp` from
`scripts/host-source-probe/support.py`): a free port, a dummy API key, private
`DR_*_DIR`s, and it terminates only its own process. Every expected pixel is
computed here, by this file's own oracle; PNGs are decoded by
`verify_phase2.read_png`.

- P1: new_document 64x96 transparent, new_layer, put_pixels with an
  overlapping buffer and runs (alpha 255, 128 and 0). The native `render
  layer=` PNG matches the oracle at alpha 255/0, `sample_pixel source:"layer"`
  is exact at alpha 128, list_history grows by exactly 1, one undo restores the
  render, and repeating the write answers unchanged: true with history
  unchanged.
- P2: refusals by name — a run off the canvas, a bad buffer length, a group
  target and color_space "bogus".
- P3: growth — a layer smaller than the canvas (a 32x48 document whose canvas
  canvas_size enlarges to 64x96, centred; layer_via_copy keeps a canvas-sized
  layer, so it cannot make one); put_pixels outside it grows it to the canvas.
- R1: render scale=8 is 512x768 and every 8x8 block equals the native pixel;
  scale with max_side, scale 2.5 and a scale past 4096 px are refused by name.
- C1: a dirty document without discard_changes is refused and still listed;
  with discard_changes:true it is gone; a clean document closes without the
  flag; a missing document_id is refused; close_second_view's last-view refusal
  names close_document.
- C2: save_action with a single close_document step is refused naming
  close_document and "cannot be a step"; no action file is written.
- REF: `--reference write|check` runs scripts/generate_mcp_reference.py
  against this instance (`--check` for check).
"""
import argparse
import base64
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]

_spec = importlib.util.spec_from_file_location(
    'pixel_art_support', ROOT / 'scripts/host-source-probe/support.py')
support = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(support)

from verify_phase1 import png  # noqa: E402  (support put scripts/ on sys.path)
from verify_phase2 import read_png  # noqa: E402

WIDTH = 64
HEIGHT = 96


class Drive:
    def __init__(self, app):
        self.app = app
        self.gates = {}

    def call(self, tool, fail=False, **arguments):
        return self.app.client.call(tool, fail=fail, **arguments)

    def raw(self, tool, **arguments):
        return self.app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})

    def render(self, **arguments):
        result = self.raw('render', **arguments)
        assert not result.get('isError'), result
        data = next(item['data'] for item in result['content'] if item['type'] == 'image')
        text = next(item['text'] for item in result['content'] if item['type'] == 'text')
        return read_png(base64.b64decode(data)), text, data

    def refused(self, tool, needles, **arguments):
        text = self.call(tool, fail=True, **arguments)
        for needle in needles:
            assert needle in text, (tool, needle, text)
        return text

    def new_document(self, width=WIDTH, height=HEIGHT):
        reply = self.call('new_document', width=width, height=height, background='transparent')
        return reply['document']['id']

    def history_count(self, document):
        return len(self.call('list_history', document_id=document)['states'])

    def gate(self, name, step):
        try:
            detail = step()
            self.gates[name] = dict(passed=True, detail=detail)
        except BaseException as error:
            self.gates[name] = dict(passed=False, detail=dict(error=repr(error)[:1200]))


def hexa(rgba):
    return '#' + ''.join(f'{v:02X}' for v in rgba)


def buffer_pixels(width, height):
    """Distinct samples whose alpha cycles 255, 128, 0 (latent RGB under 0)."""
    out = []
    for i in range(width * height):
        alpha = (255, 128, 0)[i % 3]
        out.append((i * 37 % 251, i * 11 % 241, 200 - i % 50, alpha))
    return out


def p1(drive):
    document = drive.new_document()
    layer = drive.call('new_layer', document_id=document)['layer']
    before_image, _, before_data = drive.render(document_id=document, layer=layer)
    history = drive.history_count(document)

    bx, by, bw, bh = 4, 6, 9, 7
    pixels = buffer_pixels(bw, bh)
    data = base64.b64encode(bytes(v for p in pixels for v in p)).decode()
    runs = [
        dict(x=2, y=8, length=20, color='#FF0000'),          # overlaps the buffer
        dict(x=10, y=40, length=5, color='#00FF0080'),       # alpha 128
        dict(x=11, y=40, color='#12345600'),                 # alpha 0 over the 128 run
        dict(x=30, y=90, length=34, color='#0A0B0C'),        # reaches the right edge
    ]

    # The oracle: a transparent canvas-sized layer, the buffer, then the runs.
    expected = [(0, 0, 0, 0)] * (WIDTH * HEIGHT)
    for row in range(bh):
        for column in range(bw):
            expected[(by + row) * WIDTH + bx + column] = pixels[row * bw + column]

    def parse(color):
        text = color[1:]
        values = [int(text[i:i + 2], 16) for i in range(0, len(text), 2)]
        return tuple(values + [255] * (4 - len(values)))

    for run in runs:
        for x in range(run['x'], run['x'] + run.get('length', 1)):
            expected[run['y'] * WIDTH + x] = parse(run['color'])
    written = {(bx + c, by + r) for r in range(bh) for c in range(bw)}
    for run in runs:
        written |= {(x, run['y']) for x in range(run['x'], run['x'] + run.get('length', 1))}

    reply = drive.call('put_pixels', document_id=document, layer=layer,
                       buffer=dict(x=bx, y=by, width=bw, height=bh, data=data), runs=runs)
    assert reply['ok'] and reply['layer'] == layer, reply
    assert reply['pixels'] == len(written), (reply, len(written))
    assert reply['bounds'] == dict(x=2, y=6, width=62, height=85), reply
    assert reply['grew_layer'] is False, reply
    assert drive.history_count(document) == history + 1, 'history grew by exactly one'

    (width, height, got), _, after_data = drive.render(document_id=document, layer=layer)
    assert (width, height) == (WIDTH, HEIGHT), (width, height)
    checked = {255: 0, 0: 0}
    for i, want in enumerate(expected):
        if want[3] == 255:
            assert got[i] == want, (i % WIDTH, i // WIDTH, got[i], want)
            checked[255] += 1
        elif want[3] == 0:
            assert got[i][3] == 0, (i % WIDTH, i // WIDTH, got[i], want)
            checked[0] += 1
    partial = [(i % WIDTH, i // WIDTH) for i, want in enumerate(expected) if want[3] == 128]
    assert partial, 'the oracle has alpha-128 pixels'
    for x, y in partial:
        sample = drive.call('sample_pixel', document_id=document, source='layer', layer=layer,
                            x=x, y=y)
        want = list(expected[y * WIDTH + x])
        assert sample['center_rgba_codes'] == want, (x, y, sample['center_rgba_codes'], want)

    drive.call('undo', document_id=document)
    _, _, undone_data = drive.render(document_id=document, layer=layer)
    assert undone_data == before_data, 'one undo restores the render'
    drive.call('redo', document_id=document)
    history_after = drive.history_count(document)
    again = drive.call('put_pixels', document_id=document, layer=layer,
                       buffer=dict(x=bx, y=by, width=bw, height=bh, data=data), runs=runs)
    assert again == dict(ok=True, layer=layer, unchanged=True), again
    assert drive.history_count(document) == history_after, 'an unchanged write adds no step'
    assert after_data != before_data
    return dict(document=document, layer=layer, opaque_checked=checked[255],
                transparent_checked=checked[0], alpha128_checked=len(partial),
                reply=reply)


def p2(drive):
    document = drive.new_document()
    drive.call('new_layer', document_id=document)
    off = drive.refused('put_pixels', ['runs[1]', 'canvas'], document_id=document, layer=1,
                        runs=[dict(x=0, y=0, color='#fff'),
                              dict(x=60, y=0, length=8, color='#fff')])
    short = drive.refused('put_pixels', ['buffer.data', '16'], document_id=document, layer=1,
                          buffer=dict(x=0, y=0, width=2, height=2,
                                      data=base64.b64encode(bytes(10)).decode()))
    group = drive.call('group_layers', document_id=document, layers=[1])['group']
    grouped = drive.refused('put_pixels', ['is a group'], document_id=document, layer=group,
                            runs=[dict(x=0, y=0, color='#fff')])
    bogus = drive.refused('put_pixels', ['color_space must be srgb, document or display_p3'],
                          document_id=document, layer=0, color_space='bogus',
                          runs=[dict(x=0, y=0, color='#fff')])
    fractional = drive.refused('put_pixels', ['runs[0].x must be an integer'],
                               document_id=document, layer=0,
                               runs=[dict(x=1.5, y=0, color='#fff')])
    return dict(off_canvas=off, bad_length=short, group=grouped, color_space=bogus,
                fractional=fractional)


def p3(drive):
    document = drive.new_document(WIDTH // 2, HEIGHT // 2)
    block = base64.b64encode(bytes([200, 100, 50, 255] * 100)).decode()
    drive.call('put_pixels', document_id=document, layer=0,
               buffer=dict(x=2, y=2, width=10, height=10, data=block))
    drive.call('canvas_size', document_id=document, width=WIDTH, height=HEIGHT, anchor='center')
    trimmed = drive.call('get_document', document_id=document)['layers'][0]
    assert (trimmed['width'], trimmed['height']) == (WIDTH // 2, HEIGHT // 2), trimmed
    assert (trimmed['offset_x'], trimmed['offset_y']) == (WIDTH // 4, HEIGHT // 4), trimmed
    reply = drive.call('put_pixels', document_id=document, layer=0,
                       runs=[dict(x=1, y=1, length=3, color='#00FF00')])
    assert reply['grew_layer'] is True, reply
    grown = drive.call('get_document', document_id=document)['layers'][0]
    assert (grown['width'], grown['height']) == (WIDTH, HEIGHT), grown
    sample = drive.call('sample_pixel', document_id=document, source='layer', layer=0, x=2, y=1)
    assert sample['center_rgba_codes'] == [0, 255, 0, 255], sample
    inside = drive.call('sample_pixel', document_id=document, source='layer', layer=0,
                        x=WIDTH // 4 + 4, y=HEIGHT // 4 + 4)
    assert inside['center_rgba_codes'] == [200, 100, 50, 255], inside
    return dict(trimmed=dict(width=trimmed['width'], height=trimmed['height']),
                grown=dict(width=grown['width'], height=grown['height']), reply=reply)


def r1(drive):
    document = drive.new_document()
    pixels = [((x * 4) % 256, (y * 2) % 256, (x * y) % 256, 255 if (x + y) % 2 else 128)
              for y in range(HEIGHT) for x in range(WIDTH)]
    data = base64.b64encode(bytes(v for p in pixels for v in p)).decode()
    drive.call('put_pixels', document_id=document, layer=0,
               buffer=dict(x=0, y=0, width=WIDTH, height=HEIGHT, data=data))
    (nw, nh, native), _, _ = drive.render(document_id=document, max_side=4096)
    assert (nw, nh) == (WIDTH, HEIGHT), (nw, nh)
    (sw, sh, scaled), text, _ = drive.render(document_id=document, scale=8)
    assert (sw, sh) == (WIDTH * 8, HEIGHT * 8), (sw, sh)
    assert 'scale 8× nearest-neighbour' in text, text
    for y in range(sh):
        for x in range(sw):
            assert scaled[y * sw + x] == native[(y // 8) * WIDTH + x // 8], (x, y)
    both = drive.refused('render', ['scale', 'max_side'], document_id=document, scale=2,
                         max_side=512)
    fractional = drive.refused('render', ['scale must be an integer'], document_id=document,
                               scale=2.5)
    huge = drive.refused('render', ['4096'], document_id=document, scale=100)
    return dict(scaled=[sw, sh], text=text, both=both, fractional=fractional, huge=huge)


def listed(drive):
    return {row['id'] for row in drive.call('list_documents')['documents']}


def c1(drive, folder):
    dirty = drive.new_document()
    drive.call('put_pixels', document_id=dirty, layer=0, runs=[dict(x=0, y=0, color='#000')])
    assert drive.call('get_document', document_id=dirty)['unsaved_changes'] is True
    kept = drive.refused('close_document', ['unsaved changes', 'discard_changes', 'save_copy'],
                         document_id=dirty)
    assert dirty in listed(drive), 'a refused close leaves the document open'
    closed = drive.call('close_document', document_id=dirty, discard_changes=True)
    assert closed['ok'] and closed['closed']['id'] == dirty, closed
    assert closed['discarded_changes'] is True, closed
    assert dirty not in listed(drive) and dirty not in closed['open_documents'], closed

    fixture = folder / 'clean.png'
    png(fixture, 8, 8, bytes([10, 20, 30, 255] * 64))
    clean = drive.call('open_document', path=str(fixture))['document']['id']
    assert drive.call('get_document', document_id=clean)['unsaved_changes'] is False
    tidy = drive.call('close_document', document_id=clean)
    assert tidy['discarded_changes'] is False and clean not in listed(drive), tidy

    missing = drive.refused('close_document', ['requires document_id'])
    anchor = drive.new_document()
    last_view = drive.refused('close_second_view', ['close_document'], document_id=anchor,
                              view=1)
    assert anchor in listed(drive)
    return dict(refused=kept, closed=closed, clean=tidy, missing=missing, last_view=last_view)


def c2(drive, folder):
    actions = folder / 'actions'
    before = sorted(p.name for p in actions.iterdir()) if actions.exists() else []
    text = drive.refused('save_action', ['close_document', 'cannot be a step'],
                         name='pixel-art-close-step',
                         steps=[dict(tool='close_document', arguments={})])
    after = sorted(p.name for p in actions.iterdir()) if actions.exists() else []
    assert before == after, (before, after)
    listing = drive.call('list_actions')
    names = json.dumps(listing)
    assert 'pixel-art-close-step' not in names, listing
    return dict(refusal=text, files=after)


def reference(drive, mode):
    command = [sys.executable, str(ROOT / 'scripts/generate_mcp_reference.py'),
               '--socket', drive.app.client.socket_path]
    if mode == 'check':
        command.append('--check')
    done = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    assert done.returncode == 0, (done.returncode, done.stdout, done.stderr)
    return dict(mode=mode, stdout=done.stdout.strip(), stderr=done.stderr.strip())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path,
                        default=app_under_test.default_app_bundle())
    parser.add_argument('--receipts', type=Path, required=True)
    parser.add_argument('--reference', choices=['write', 'check'])
    arguments = parser.parse_args()
    os.environ['PYTHONDONTWRITEBYTECODE'] = '1'
    output = arguments.receipts.expanduser().resolve()
    output.mkdir(parents=True, exist_ok=True)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    receipt = dict(app=str(executable), error=None)
    drive = None
    try:
        folder = output / 'app'
        with support.HostApp(folder, output / 'recovery', executable) as app:
            drive = Drive(app)
            drive.gate('P1 exact write, one undo step, unchanged repeat', lambda: p1(drive))
            drive.gate('P2 refusals by name', lambda: p2(drive))
            drive.gate('P3 growth past a trimmed layer', lambda: p3(drive))
            drive.gate('R1 render scale', lambda: r1(drive))
            drive.gate('C1 close_document', lambda: c1(drive, folder))
            drive.gate('C2 close_document is not a step', lambda: c2(drive, folder))
            if arguments.reference:
                drive.gate('REF mcp reference', lambda: reference(drive, arguments.reference))
    except BaseException as error:
        receipt['error'] = f'{type(error).__name__}: {error}'
    if drive is not None:
        receipt['gates'] = drive.gates
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2, default=str) + '\n')
    gates = {name: gate['passed'] for name, gate in receipt.get('gates', {}).items()}
    failed = {name: receipt['gates'][name]['detail'] for name, ok in gates.items() if not ok}
    print(json.dumps(dict(error=receipt['error'], gates=gates, failed=failed,
                          receipt=str(output / 'receipt.json')), indent=2))
    passed = not receipt['error'] and gates and all(gates.values())
    if passed:
        print('ALL GATES PASS')
    raise SystemExit(0 if passed else 1)


if __name__ == '__main__':
    main()
