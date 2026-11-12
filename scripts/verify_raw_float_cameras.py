#!/usr/bin/env python3
"""MCP drive: three raw.pixls.us camera files open as a 32-bit float (linear)
Camera RAW development, each at the size of its 16-bit development.

The script owns its one app instance: the Debug app (or `DR_VERIFY_APP`) on
its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, a 6 GiB working limit (a 24 to 36 MP float development
with its scratch does not fit the 2 GiB pin) and private `DR_*_DIR`s
(`verify_v1_lens.App`). It terminates only that process. The fixtures are
`~/devel/doctored-fixtures/raw-pixls/{nikon-z6/Z6.NEF, sony-a7m3/A73.ARW,
pentax-k1m2/K1D.DNG}`; an absent fixture FAILS its case. Every document is
opened from its own copy of the file, and closed before the next opens.

Per file, the call as a person makes it, with the default recipe:
- `open_document` with `raw: {output_encoding: linear_float}` answers ok, the
  document's precision is `rgba32_float` and `get_raw_source`'s
  `master_depth` is 32;
- `open_document` with `raw: {output_encoding: sdr16}` answers ok at
  `rgba16`, with the float development's width and height;
- `sample_pixel` (`source: layer`) at the four corners and the four edge
  midpoints: every alpha of the float development equals the 16-bit
  development's alpha code / 65535, the two encodings agreeing on coverage.

`Z6.NEF` is the file the decoder renders at float with 61 % of its alphas a
few ulps either side of 1, all within 4e-7 (measured at its default recipe),
which `RetainedRawDecoder.normalizeOpaqueAlpha` stores as exactly 1; `A73.ARW`
(whose default development has transparent left and right edge strips, after
the opacity check, in both encodings)
and `K1D.DNG` are the controls.

Usage: python3 scripts/verify_raw_float_cameras.py [--output DIR] [--only NAME]
Prints `PASS <file>` per file and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from verify_v1_lens import App, SlowClient  # noqa: E402

FIXTURES = Path.home() / 'devel/doctored-fixtures/raw-pixls'
FILES = (
    FIXTURES / 'nikon-z6/Z6.NEF',
    FIXTURES / 'sony-a7m3/A73.ARW',
    FIXTURES / 'pentax-k1m2/K1D.DNG',
)
LIMIT = 6 * 1024 ** 3
BUSY = 'Another background operation is running.'


def edge_points(width, height):
    """The four corners and the four edge midpoints."""
    right, bottom = width - 1, height - 1
    return [(0, 0), (right, 0), (0, bottom), (right, bottom),
            (width // 2, 0), (width // 2, bottom), (0, height // 2), (right, height // 2)]


def sample(app, document, x, y):
    # A reply can return while the document's own background step still runs;
    # a read then answers BUSY. Wait it out with a ceiling only a stuck job reaches.
    deadline = time.monotonic() + 60
    while True:
        try:
            return app.call('sample_pixel', document_id=document, source='layer', layer=0,
                            x=x, y=y)
        except AssertionError as error:
            if BUSY not in str(error) or time.monotonic() >= deadline:
                raise
            time.sleep(0.05)


def close(app, document):
    app.call('close_document', document_id=document, discard_changes=True)


def camera_case(app, folder, fixture):
    assert fixture.is_file(), f'missing {fixture}'
    copies = []
    for encoding in ('linear_float', 'sdr16'):
        copy = folder / f'{encoding}-{fixture.name}'
        shutil.copyfile(fixture, copy)
        copies.append(copy)

    reply = app.call('open_document', path=str(copies[0]),
                     raw=dict(output_encoding='linear_float'))
    document = reply['document']
    try:
        assert document['precision'] == 'rgba32_float', document
        source = app.call('get_raw_source', document_id=document['id'], layer=0)['source']
        assert source['master_depth'] == 32, source
        size = (document['width'], document['height'])
        points = edge_points(*size)
        float_alpha = [sample(app, document['id'], x, y)['center_rgba32'][3] for x, y in points]
    finally:
        close(app, document['id'])

    reply = app.call('open_document', path=str(copies[1]), raw=dict(output_encoding='sdr16'))
    document = reply['document']
    try:
        assert document['precision'] == 'rgba16', document
        assert (document['width'], document['height']) == size, (document, size)
        codes = [sample(app, document['id'], x, y)['center_rgba_codes'][3] for x, y in points]
    finally:
        close(app, document['id'])

    disagree = [(point, a, code) for point, a, code in zip(points, float_alpha, codes)
                if a != code / 65535]
    assert not disagree, ('float alpha != 16-bit alpha / 65535', disagree)
    return dict(size=list(size), edge_alpha=float_alpha, edge_codes=codes)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, the copies and the receipt')
    parser.add_argument('--only', help='one fixture file name (Z6.NEF, A73.ARW or K1D.DNG)')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-raw-float-cameras-'))
    output.mkdir(parents=True, exist_ok=True)
    files = [path for path in FILES if arguments.only in (None, path.name)]
    if not files:
        parser.error(f'--only names no fixture: {arguments.only}')

    results, failed = {}, []
    with App(output / 'instance', LIMIT, SlowClient) as app:
        for fixture in files:
            try:
                results[fixture.name] = dict(passed=True,
                                             detail=camera_case(app, output, fixture))
                print(f'PASS {fixture.name}: {json.dumps(results[fixture.name]["detail"])}',
                      flush=True)
            except BaseException as error:  # noqa: BLE001 - every case reports
                results[fixture.name] = dict(passed=False, error=repr(error)[:2000])
                failed.append(fixture.name)
                print(f'FAIL {fixture.name}: {error!r}'[:2400], flush=True)

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
