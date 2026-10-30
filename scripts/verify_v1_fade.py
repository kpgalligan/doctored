#!/usr/bin/env python3
"""MCP drive for Edit > Fade (v1, `designs/v1-shortcuts-fade.md`): the `fade`
tool end to end at 8, 16 and 32 bits.

The script owns one app instance (`verify_v1_lens.App`): the Debug app (or
`DR_VERIFY_APP`) on its own scratch MCP socket with `DR_BACKGROUND=1`, a
dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s.
It terminates only that process. Every expected number is computed here, by
this file's own oracle, never read back from the app.

The fixture is a 64x48 opaque ramp, R = 40 + x, G = 30 + 2y, B = 100 + x - y;
at 16 bits the file is promoted (`convert_precision rgba16`), every code
widened by 257. The faded step is `apply_filter invert`, whose result at a
pixel is 255 - v per channel.

At 8 and 16 bits:
- HALF: `fade` Normal at 0.5 lands every sampled channel on v + 0.5 * (after -
  v) within one 8-bit level; the reply names the step ("Fade <step>"), the
  history keeps its entry count (the fade REPLACES the filter's step) and one
  `undo` restores the ramp.
- MULTIPLY: `fade` Multiply at 1 is the W3C Multiply of the inverse over the
  ramp, v * (255 - v) / 255, within one level.
- REFUSALS: `fade` after `select_rect`, and on a fresh document, refuses with
  "There is nothing to fade".
- REPLAY: a recording of [apply_filter, fade] holds those two steps and
  `run_action` replays them onto a second copy as ONE step with identical
  samples.

At 32 bits (the float fixture): after `apply_filter lens_correction`, `fade`
in Screen refuses by name ("the screen blend mode is unavailable in a 32-bit
float document") and Multiply at 0.5 answers changed: true.

Usage: python3 scripts/verify_v1_fade.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS` when every case passed.
"""
import argparse
from pathlib import Path
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_phase1 import png  # noqa: E402
from verify_v1_lens import App  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
W, H = 64, 48
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
NOTHING = 'There is nothing to fade'
POINTS = [(x, y) for y in range(1, H, 7) for x in range(2, W, 9)]


def ramp_at(x, y):
    return [40 + x, 30 + 2 * y, 100 + x - y]


def ramp_png(folder, tag):
    pixels = bytearray()
    for y in range(H):
        for x in range(W):
            pixels += bytes(ramp_at(x, y) + [255])
    path = Path(folder) / f'ramp-{tag}.png'
    png(path, W, H, bytes(pixels))
    return path


def opened(app, folder, tag, depth):
    document = app.call('open_document', path=str(ramp_png(folder, tag)))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    return document


def history(app, document):
    return app.call('list_history', document_id=document)


def levels(app, document, x, y, depth):
    codes = app.call('sample_pixel', document_id=document, source='layer', layer=0, x=x,
                     y=y)['center_rgba_codes']
    return [c / 257 for c in codes] if depth == 16 else list(codes)


def invert(app, document):
    reply = app.call('apply_filter', document_id=document, filter='invert')
    assert reply.get('ok', True), reply
    return reply


def half(app, depth, folder):
    document = opened(app, folder, f'half-{depth}', depth)
    invert(app, document)
    states = len(history(app, document)['states'])
    reply = app.call('fade', document_id=document, opacity=0.5)
    assert reply['ok'] and reply['changed'] is True, reply
    assert reply['name'] == f"Fade {reply['faded']}", reply
    after = history(app, document)
    assert len(after['states']) == states, ('the fade replaces the step', after['states'])
    assert after['states'][after['cursor']]['name'] == reply['name'], after['states']
    worst = 0.0
    for x, y in POINTS:
        got = levels(app, document, x, y, depth)
        for c, v in enumerate(ramp_at(x, y)):
            want = v + 0.5 * ((255 - v) - v)
            worst = max(worst, abs(got[c] - want))
            assert abs(got[c] - want) <= 1.0, ('half', depth, x, y, c, got, want)
    app.call('undo', document_id=document)
    for x, y in POINTS:
        got = levels(app, document, x, y, depth)
        for c, v in enumerate(ramp_at(x, y)):
            assert abs(got[c] - v) <= 0.01, ('undo restores the ramp', x, y, got)
    return dict(faded=reply['faded'], worst_levels=round(worst, 3))


def multiply(app, depth, folder):
    document = opened(app, folder, f'multiply-{depth}', depth)
    invert(app, document)
    reply = app.call('fade', document_id=document, opacity=1, mode='Multiply')
    assert reply['changed'] is True, reply
    worst = 0.0
    for x, y in POINTS:
        got = levels(app, document, x, y, depth)
        for c, v in enumerate(ramp_at(x, y)):
            # W3C Multiply of an opaque source over an opaque backdrop: cb * cs.
            want = v * (255 - v) / 255
            worst = max(worst, abs(got[c] - want))
            assert abs(got[c] - want) <= 1.0, ('multiply', depth, x, y, c, got, want)
    return dict(worst_levels=round(worst, 3))


def bucket(app, depth, folder):
    """A Paint Bucket step fades by the pixels it changed: Multiply at full
    opacity is the W3C product inside the flood, and every pixel outside it
    stays the ramp."""
    document = opened(app, folder, f'bucket-{depth}', depth)
    ink = (0xc0, 0x60, 0x20)
    app.call('fill', document_id=document, layer=0, x=W // 2, y=H // 2, color='#c06020',
             tolerance=24, contiguous=True)
    flooded = {(x, y) for x, y in POINTS
               if any(abs(g - v) > 0.5 for g, v in zip(levels(app, document, x, y, depth),
                                                        ramp_at(x, y)))}
    assert flooded and len(flooded) < len(POINTS), ('a partial flood', len(flooded))
    reply = app.call('fade', document_id=document, opacity=1, mode='Multiply')
    assert reply['changed'] is True, reply
    for x, y in POINTS:
        got = levels(app, document, x, y, depth)
        for c, v in enumerate(ramp_at(x, y)):
            want = v * ink[c] / 255 if (x, y) in flooded else v
            assert abs(got[c] - want) <= 1.0, ('bucket', depth, x, y, c, got, want)
    return dict(flooded=len(flooded), of=len(POINTS))


def refusals(app, depth, folder):
    fresh = opened(app, folder, f'fresh-{depth}', depth)
    app.refused('fade', NOTHING, document_id=fresh, opacity=0.5)
    document = opened(app, folder, f'refusal-{depth}', depth)
    invert(app, document)
    app.call('select_rect', document_id=document, x=4, y=4, width=20, height=10)
    held = history(app, document)['cursor']
    app.refused('fade', NOTHING, document_id=document, opacity=0.5)
    assert history(app, document)['cursor'] == held, 'a refusal adds no step'
    return dict(refused=True)


def replay(app, depth, folder):
    document = opened(app, folder, f'replay-{depth}', depth)
    second = opened(app, folder, f'replay-copy-{depth}', depth)
    name = f'v1 fade {depth}'
    app.call('start_recording', discard=True)
    invert(app, document)
    assert app.call('fade', document_id=document, opacity=0.25, mode='Multiply')['changed']
    action = app.call('stop_recording', name=name)
    assert [step['tool'] for step in action['steps']] == ['apply_filter', 'fade'], action
    recorded = [levels(app, document, x, y, depth) for x, y in POINTS]
    start = history(app, second)['cursor']
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert history(app, second)['cursor'] == start + 1, 'the replay is one step'
    replayed = [levels(app, second, x, y, depth) for x, y in POINTS]
    assert replayed == recorded, 'the replay differs'
    return dict(samples=len(POINTS))


def float_case(app, folder):
    scene = Path(folder) / 'float.tiff'
    shutil.copyfile(FLOAT_FIXTURE, scene)
    document = app.call('open_document', path=str(scene))['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    app.refused('fade', NOTHING, document_id=document, opacity=0.5)
    reply = app.call('apply_filter', document_id=document, filter='lens_correction',
                     distortion=20)
    assert reply['changed'] is True, reply
    screen = app.refused('fade', 'the screen blend mode is unavailable in a 32-bit float document',
                         document_id=document, opacity=0.5, mode='Screen')
    states = len(history(app, document)['states'])
    faded = app.call('fade', document_id=document, opacity=0.5, mode='Multiply')
    assert faded['changed'] is True, faded
    assert len(history(app, document)['states']) == states, 'the float fade replaces the step'
    return dict(screen=screen[:70], faded=faded['faded'])


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='keep the run folder here')
    args = parser.parse_args()
    folder = Path(args.output) if args.output else Path(tempfile.mkdtemp(prefix='rz-v1-fade-'))
    folder.mkdir(parents=True, exist_ok=True)
    failures = 0
    with App(folder / 'instance', 2 * 1024 ** 3) as app:
        cases = []
        for depth in (8, 16):
            for case in (half, multiply, bucket, refusals, replay):
                cases.append((f'{case.__name__} {depth}-bit',
                              lambda case=case, depth=depth: case(app, depth, folder)))
        cases.append(('float 32-bit', lambda: float_case(app, folder)))
        for name, run in cases:
            try:
                detail = run()
                print(f'PASS {name}: {detail}', flush=True)
            except Exception as error:  # noqa: BLE001 - every case reports
                failures += 1
                print(f'FAIL {name}: {error!r}', flush=True)
    if failures:
        print(f'{failures} CASE(S) FAILED; the run folder is {folder}')
        return 1
    print('ALL CASES PASS')
    return 0


if __name__ == '__main__':
    sys.exit(main())
