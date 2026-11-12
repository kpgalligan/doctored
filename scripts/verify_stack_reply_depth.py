#!/usr/bin/env python3
"""MCP drive: the `depth` of the `load_files_into_stack`, `merge_panorama`,
`merge_to_hdr` and `merge_layers_to_hdr` replies names the made document's
mode and depth, the same name as the reply's `document.precision`.

The script owns its app instance (`verify_v1_stack_align.App`: the Debug app
or `DR_VERIFY_APP`, its own scratch socket, `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, the 2 GiB pin, private `DR_*_DIR`s) and terminates only
that process.

THE FILES are the panorama drive's A-20 pair (`verify_v1_merge_panorama`: the
registration photo on a cylinder of radius 600, two 600 x 400 views yawed
-10 and +10 degrees), written twice: as that drive's 8-bit RGBA PNGs, and as
16-bit Grayscale PNGs (colour type 0) whose sample is the RGBA file's grey
code x 257.

Per pair, every reply answers ok and its `depth` equals its
`document.precision`, which is:
- Grayscale: `load_files_into_stack` `gray16`; `merge_panorama` `gray16`;
  `merge_to_hdr` (evs [0, 1], align false) and `merge_layers_to_hdr` of the
  stack (layers [0, 1], evs [0, 1], align false) `gray32`;
- RGB: `rgba8`, `rgba8`, `rgba32_float`, `rgba32_float`.

Usage: python3 scripts/verify_stack_reply_depth.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
from verify_v1_stack_align import App, close  # noqa: E402
from verify_v1_merge_panorama import author, pair_files, yaw_pair  # noqa: E402
from verify_v1_stack_projection import decode_png  # noqa: E402

EXPECTED = {
    'gray': dict(load_files_into_stack='gray16', merge_panorama='gray16',
                 merge_to_hdr='gray32', merge_layers_to_hdr='gray32'),
    'rgb': dict(load_files_into_stack='rgba8', merge_panorama='rgba8',
                merge_to_hdr='rgba32_float', merge_layers_to_hdr='rgba32_float'),
}


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def gray16_copy(source, path):
    """A 16-bit Grayscale PNG of `source`'s grey codes (R = G = B), each x 257."""
    width, height, pixels = decode_png(source.read_bytes())
    rows = b''.join(b'\0' + b''.join(struct.pack('>H', p[0] * 257) for p in row)
                    for row in pixels)
    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 16, 0, 0, 0, 0))
                     + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))
    return path


def names(reply, tool):
    """The reply's `depth` and `document.precision`, after its `ok`."""
    assert reply.get('ok') is True, (tool, reply)
    return dict(depth=reply['depth'], precision=reply['document']['precision'])


def pair_case(app, files, kind):
    """All four replies are read before any is judged, so a failure lists every one."""
    paths = [str(path) for path in files]
    seen = {}
    stack = app.call('load_files_into_stack', paths=paths)
    try:
        seen['load_files_into_stack'] = names(stack, 'load_files_into_stack')
        merged = app.call('merge_layers_to_hdr', document_id=stack['document']['id'],
                          layers=[0, 1], evs=[0, 1], align=False)
        try:
            seen['merge_layers_to_hdr'] = names(merged, 'merge_layers_to_hdr')
        finally:
            close(app, merged['document']['id'])
    finally:
        close(app, stack['document']['id'])
    for tool, extra in (('merge_panorama', {}),
                        ('merge_to_hdr', dict(evs=[0, 1], align=False))):
        reply = app.call(tool, paths=paths, **extra)
        try:
            seen[tool] = names(reply, tool)
        finally:
            close(app, reply['document']['id'])
    wrong = {tool: dict(seen[tool], expected=name) for tool, name in EXPECTED[kind].items()
             if not seen[tool]['depth'] == seen[tool]['precision'] == name}
    assert not wrong, ('depth, document.precision and the expected name differ', wrong)
    return seen


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-stack-reply-depth-'))
    output.mkdir(parents=True, exist_ok=True)
    fixtures = output / 'fixtures'
    grey = author(fixtures)
    rgb = pair_files(fixtures, grey, 'A-20', yaw_pair(600, 400, 20))
    gray = [gray16_copy(path, path.with_name(path.stem + '-gray16.png')) for path in rgb]
    results, failed = {}, []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}: {json.dumps(results[name]["detail"])}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', 2 * 1024 ** 3) as app:
        run('grayscale pair', lambda: pair_case(app, gray, 'gray'))
        run('rgb pair', lambda: pair_case(app, rgb, 'rgb'))
    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
