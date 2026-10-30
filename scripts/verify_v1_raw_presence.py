#!/usr/bin/env python3
"""MCP drive for Develop Texture, Clarity and Dehaze: the `raw`
object's `texture`, `clarity` and `dehaze` on the real NEF, through
`open_document`, `redevelop_raw`, presets, Sync Settings and `.rz`.

The script owns its one app instance: the Debug app (or `DR_VERIFY_APP`) on
its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, an 8 GiB working limit and private `DR_*_DIR`s
(`verify_v1_lens.App`). It terminates only that process. The fixture is
`~/devel/doctored-fixtures/camera/camera.NEF`; an absent fixture FAILS the
run. Every document is opened from its own copy of the file, each copy named
`camera.NEF` in its own folder, so the developments differ only by their
settings.

What is measured. Pixels come from a TIFF `save_copy` (16-bit for a 16-bit
document), read here: baseline TIFF, uncompressed, LZW or Deflate, with or
without the horizontal predictor. Y is the Rec. 709 luma of the codes
(0.2126 R + 0.7152 G + 0.0722 B). box_r is the box mean of half-width r. Over
the central 512×512 pixels, E_tex = mean |box_1(Y) − box_9(Y)| (the band
between the 3008-px NEF's fine and mid Presence radii, 1 and 9) and E_cla =
mean |box_9(Y) − box_59(Y)| (mid to coarse, 9 to 59). D = mean min(R, G, B)
over the whole frame, the dark channel Dehaze works on.

Bounds, fixed before the first run and never widened:
- TEXTURE: +100 gives E_tex ≥ 1.10 × the plain development's, −100 ≤ 0.90 ×.
- CLARITY: +100 gives E_cla ≥ 1.10 × plain, −100 ≤ 0.90 ×.
- DEHAZE: +100 gives D ≤ 0.90 × plain, −100 gives D ≥ 1.10 ×.
Everything else is exact: a recipe, a file digest, a history cursor.

Cases:
- ZERO: an open with texture, clarity and dehaze all 0 has the same
  `get_raw_source` recipe as an open with no keys, with no `presence` key, and
  the same TIFF SHA-256; `redevelop_raw` with all three 0 changes no history.
- TEXTURE, CLARITY, DEHAZE: the bounds above, each slider alone.
- 8-BIT: an 8-bit open with clarity 50 changes the TIFF; its retained 16-bit
  master (`inspect_raw_development`'s `before_histogram`) differs from a plain
  8-bit open's and equals a 16-bit open's with clarity 50.
- REDEVELOP + UNDO: `redevelop_raw {clarity: 40}` adds one history step and
  changes the TIFF; `undo` restores the plain TIFF digest.
- PRESET: `save_raw_preset` from a layer with fields texture, clarity, dehaze
  writes a schema-4 file carrying the three; `apply_raw_preset` on a plain
  copy gives an equal recipe `presence`. A schema-4 file with `texture: 150`
  lists `repairable: true`; `repair_raw_preset` rewrites it at schema 4 without
  `texture`, and it then applies.
- SYNC: `sync_raw_settings` with `fields: ["clarity"]` copies only clarity.
- RZ: `save_copy` to `.rz`, close, reopen: the recipe and the TIFF digest are
  equal, and a further `redevelop_raw` works.
- OLD: a schema-3 preset without the keys applies; a `.rz` saved without
  Presence reopens with none, and `redevelop_raw {clarity: 20}` adds only
  clarity.
- REFUSAL: `texture: 101` and `clarity: true` are refused by name; a schema-3
  preset file carrying `dehaze` is listed as a broken `invalid_schema` row
  naming the file, not as a preset, and `apply_raw_preset` of its name is
  refused naming it. Nothing changes.

Usage: python3 scripts/verify_v1_raw_presence.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import array
import hashlib
import json
from pathlib import Path
import shutil
import struct
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
from verify_v1_lens import App, SlowClient  # noqa: E402

FIXTURE = Path.home() / 'devel/doctored-fixtures/camera/camera.NEF'
LIMIT = 8 * 1024 ** 3

CROP = 512
FINE, MID, COARSE = 1, 9, 59
MORE, LESS = 1.10, 0.90


# ------------------------------------------------------------ TIFF reader --

def lzw_decode(data):
    """TIFF LZW (MSB-first codes, early change)."""
    out = bytearray()
    table = [bytes([i]) for i in range(256)] + [b'', b'']
    bits, position, total = 9, 0, len(data) * 8
    previous = None
    while position + bits <= total:
        index, shift = position >> 3, position & 7
        chunk = int.from_bytes(data[index:index + 4].ljust(4, b'\0'), 'big')
        code = (chunk >> (32 - shift - bits)) & ((1 << bits) - 1)
        position += bits
        if code == 256:
            table = table[:258]
            bits, previous = 9, None
            continue
        if code == 257:
            break
        if previous is None:
            entry = table[code]
        elif code < len(table):
            entry = table[code]
            table.append(previous + entry[:1])
        else:
            entry = previous + previous[:1]
            table.append(entry)
        out += entry
        previous = entry
        if len(table) + 1 >= (1 << bits) and bits < 12:
            bits += 1
    return bytes(out)


def read_tiff(path):
    """(width, height, samples per pixel, bits, flat sample array)."""
    data = Path(path).read_bytes()
    endian = '<' if data[:2] == b'II' else '>'
    assert data[:2] in (b'II', b'MM') and struct.unpack_from(endian + 'H', data, 2)[0] == 42, path
    offset = struct.unpack_from(endian + 'I', data, 4)[0]
    count = struct.unpack_from(endian + 'H', data, offset)[0]
    tags = {}
    for i in range(count):
        at = offset + 2 + i * 12
        tag, kind, length = struct.unpack_from(endian + 'HHI', data, at)
        size = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 7: 1, 16: 8}.get(kind, 1) * length
        start = at + 8 if size <= 4 else struct.unpack_from(endian + 'I', data, at + 8)[0]
        raw = data[start:start + size]
        if kind == 3:
            tags[tag] = struct.unpack(endian + f'{length}H', raw)
        elif kind == 4:
            tags[tag] = struct.unpack(endian + f'{length}I', raw)
        else:
            tags[tag] = raw
    width, height = tags[256][0], tags[257][0]
    spp = tags.get(277, (1,))[0]
    depth = tags[258][0]
    assert all(b == depth for b in tags[258]) and depth in (8, 16), tags[258]
    assert tags.get(284, (1,)) == (1,), 'planar TIFF'
    compression = tags.get(259, (1,))[0]
    predictor = tags.get(317, (1,))[0]
    rows_per_strip = tags.get(278, (height,))[0]
    row_bytes = width * spp * depth // 8
    pieces = []
    for start, length in zip(tags[273], tags[279]):
        strip = data[start:start + length]
        if compression == 1:
            pieces.append(strip)
        elif compression in (8, 32946):
            pieces.append(zlib.decompress(strip))
        elif compression == 5:
            pieces.append(lzw_decode(strip))
        else:
            raise AssertionError(f'TIFF compression {compression}')
    raw = b''.join(pieces)
    assert len(raw) >= row_bytes * height, (len(raw), row_bytes * height, rows_per_strip)
    samples = array.array('H' if depth == 16 else 'B', raw[:row_bytes * height])
    if depth == 16 and (endian == '<') != (sys.byteorder == 'little'):
        samples.byteswap()
    if predictor == 2:
        modulus = 1 << depth
        stride = width * spp
        for y in range(height):
            base = y * stride
            for i in range(base + spp, base + stride):
                samples[i] = (samples[i] + samples[i - spp]) % modulus
    else:
        assert predictor == 1, predictor
    return width, height, spp, depth, samples


# --------------------------------------------------------------- measures --

def measures(path):
    """E_tex, E_cla over the central CROP×CROP and D over the frame."""
    width, height, spp, _, samples = read_tiff(path)
    assert spp in (3, 4), spp
    r, g, b = samples[0::spp], samples[1::spp], samples[2::spp]
    dark = sum(map(min, r, g, b)) / (width * height)
    margin = COARSE
    side = CROP + 2 * margin
    x0, y0 = (width - CROP) // 2 - margin, (height - CROP) // 2 - margin
    assert x0 >= 0 and y0 >= 0, (width, height)
    # The integral image of Y over the crop and its margin.
    integral = [[0.0] * (side + 1) for _ in range(side + 1)]
    for y in range(side):
        row = (y0 + y) * width + x0
        above, here = integral[y], integral[y + 1]
        running = 0.0
        for x in range(side):
            i = row + x
            running += 0.2126 * r[i] + 0.7152 * g[i] + 0.0722 * b[i]
            here[x + 1] = above[x + 1] + running

    def box(x, y, radius):
        a, c = y - radius, y + radius + 1
        left, right = x - radius, x + radius + 1
        total = integral[c][right] - integral[a][right] - integral[c][left] + integral[a][left]
        return total / ((2 * radius + 1) ** 2)

    texture = clarity = 0.0
    for y in range(margin, margin + CROP):
        for x in range(margin, margin + CROP):
            mid = box(x, y, MID)
            texture += abs(box(x, y, FINE) - mid)
            clarity += abs(mid - box(x, y, COARSE))
    n = CROP * CROP
    return dict(texture=texture / n, clarity=clarity / n, dark=dark)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def digest_name(name):
    """The RAW preset library's canonical file name for an ASCII `name`."""
    assert name.isascii(), name
    return hashlib.sha256(name.lower().encode()).hexdigest() + '.json'


# --------------------------------------------------------------- plumbing --

class Run:
    def __init__(self, app, folder):
        self.app = app
        self.folder = Path(folder)
        self.presets = self.folder / 'instance' / 'raw-presets'
        self.serial = 0
        self.plain = None
        self.plain_tiff = None
        self.plain_measures = None

    def next_path(self, tag, suffix):
        self.serial += 1
        return self.folder / f'{self.serial:02d}-{tag}{suffix}'

    def open(self, tag, precision='preserve', **development):
        folder = self.next_path(tag, '')
        folder.mkdir(parents=True)
        path = folder / 'camera.NEF'
        shutil.copyfile(FIXTURE, path)
        arguments = dict(path=str(path), precision=precision)
        if development:
            arguments['raw'] = development
        return self.app.call('open_document', **arguments)['document']['id']

    def close(self, document):
        self.app.call('close_document', document_id=document, discard_changes=True)

    def recipe(self, document):
        source = self.app.call('get_raw_source', document_id=document, layer=0)['source']
        return source['raw']['recipe']

    def presence(self, document):
        return (self.recipe(document).get('enhancements') or {}).get('presence')

    def cursor(self, document):
        history = self.app.call('list_history', document_id=document)
        return history['cursor'], history['undo_count'], history['redo_count']

    def tiff(self, document, tag):
        path = self.next_path(tag, '.tiff')
        self.app.call('save_copy', document_id=document, path=str(path), format='tiff')
        return path

    def master_histogram(self, document):
        report = self.app.call('inspect_raw_development', document_id=document, layer=0)
        return report['before_histogram']


# ----------------------------------------------------------------- cases --

def zero_case(run):
    run.plain = run.open('plain')
    zero = run.open('zero', texture=0, clarity=0, dehaze=0)
    plain_recipe, zero_recipe = run.recipe(run.plain), run.recipe(zero)
    assert zero_recipe == plain_recipe, (zero_recipe, plain_recipe)
    assert 'presence' not in (plain_recipe.get('enhancements') or {}), plain_recipe
    run.plain_tiff = run.tiff(run.plain, 'plain')
    zero_tiff = run.tiff(zero, 'zero')
    assert digest(zero_tiff) == digest(run.plain_tiff), 'all-zero Presence changed the pixels'
    before = run.cursor(run.plain)
    reply = run.app.call('redevelop_raw', document_id=run.plain, layer=0,
                         raw=dict(texture=0, clarity=0, dehaze=0))
    assert reply['ok'] is True, reply
    assert run.cursor(run.plain) == before, (run.cursor(run.plain), before)
    assert run.recipe(run.plain) == plain_recipe, 'an all-zero redevelop changed the recipe'
    run.close(zero)
    run.plain_measures = measures(run.plain_tiff)
    return dict(tiff_sha256=digest(run.plain_tiff), history=list(before), plain=run.plain_measures)


def slider_case(run, key, measure, sign_more):
    """+100 and −100 of `key` against the plain development's `measure`."""
    plain = run.plain_measures[measure]
    found = {}
    for amount in (100, -100):
        document = run.open(f'{key}{amount:+d}', **{key: amount})
        presence = run.presence(document)
        expected = dict(texture=0, clarity=0, dehaze=0)
        expected[key] = amount
        assert presence == expected, presence
        value = measures(run.tiff(document, f'{key}{amount:+d}'))[measure]
        run.close(document)
        found[amount] = value / plain
    up, down = (found[100], found[-100]) if sign_more else (found[-100], found[100])
    assert up >= MORE, (key, measure, found)
    assert down <= LESS, (key, measure, found)
    return dict(measure=measure, plain=plain, ratio_plus_100=found[100], ratio_minus_100=found[-100])


def eight_bit_case(run):
    plain8 = run.open('plain8', precision='rgba8')
    clarity8 = run.open('clarity8', precision='rgba8', clarity=50)
    clarity16 = run.open('clarity16', clarity=50)
    assert run.presence(clarity8) == dict(texture=0, clarity=50, dehaze=0), run.presence(clarity8)
    assert digest(run.tiff(plain8, 'plain8')) != digest(run.tiff(clarity8, 'clarity8')), \
        'clarity 50 left the 8-bit pixels unchanged'
    histogram8 = run.master_histogram(clarity8)
    assert histogram8 != run.master_histogram(plain8), 'the 8-bit master does not carry clarity'
    assert histogram8 == run.master_histogram(clarity16), 'the 8-bit and 16-bit masters differ'
    for document in (plain8, clarity8, clarity16):
        run.close(document)
    return dict(master_carries_clarity=True)


def redevelop_case(run):
    document = run.open('redevelop')
    before = run.cursor(document)
    reply = run.app.call('redevelop_raw', document_id=document, layer=0, raw=dict(clarity=40))
    assert reply['ok'] is True, reply
    after = run.cursor(document)
    assert after[0] == before[0] + 1, (before, after)
    assert run.presence(document) == dict(texture=0, clarity=40, dehaze=0), run.presence(document)
    changed = run.tiff(document, 'redeveloped')
    assert digest(changed) != digest(run.plain_tiff), 'redevelop_raw left the pixels unchanged'
    run.app.call('undo', document_id=document)
    assert run.presence(document) is None, run.presence(document)
    assert digest(run.tiff(document, 'undone')) == digest(run.plain_tiff), 'undo did not restore'
    run.close(document)
    return dict(history_before=list(before), history_after=list(after))


def preset_case(run):
    source = run.open('preset-source', texture=30, clarity=-20, dehaze=15)
    wanted = dict(texture=30, clarity=-20, dehaze=15)
    assert run.presence(source) == wanted, run.presence(source)
    name = 'v1 presence'
    saved = run.app.call('save_raw_preset', name=name, document_id=source, layer=0,
                         fields=['texture', 'clarity', 'dehaze'], replace=True)
    assert saved['ok'] is True, saved
    written = json.loads((run.presets / digest_name(name)).read_text())
    assert written['schema_version'] == 4, written
    assert written['raw'] == wanted, written
    target = run.open('preset-target')
    applied = run.app.call('apply_raw_preset', name=name, document_id=target, layer=0)
    assert applied['ok'] is True, applied
    assert run.presence(target) == run.presence(source), (run.presence(target), run.presence(source))
    run.close(target)

    broken_file = 'zz-broken-texture.json'
    (run.presets / broken_file).write_text(json.dumps(
        dict(schema_version=4, name='Broken Texture', raw=dict(texture=150, clarity=10))))
    rows = {row['file']: row for row in run.app.call('list_raw_presets')['broken']}
    assert rows[broken_file]['repairable'] is True, rows[broken_file]
    repaired = run.app.call('repair_raw_preset', file=broken_file)
    assert repaired['ok'] is True and repaired['dropped'] == ['texture'], repaired
    assert repaired['preset']['schema_version'] == 4, repaired
    assert repaired['preset']['raw'] == dict(clarity=10), repaired
    rewritten = json.loads((run.presets / repaired['file']).read_text())
    assert rewritten == dict(schema_version=4, name='Broken Texture', raw=dict(clarity=10)), rewritten
    target = run.open('repaired-target')
    applied = run.app.call('apply_raw_preset', name='Broken Texture', document_id=target, layer=0)
    assert applied['ok'] is True, applied
    assert run.presence(target) == dict(texture=0, clarity=10, dehaze=0), run.presence(target)
    run.close(target)
    run.close(source)
    return dict(file=written, repaired=rewritten)


def sync_case(run):
    source = run.open('sync-source', texture=30, clarity=-20, dehaze=15)
    target = run.open('sync-target')
    reply = run.app.call('sync_raw_settings', document_id=source, layer=0, fields=['clarity'],
                         targets=[dict(document_id=target, layer=0)])
    assert reply['ok'] is True, reply
    assert reply['raw'] == dict(clarity=-20), reply['raw']
    assert run.presence(target) == dict(texture=0, clarity=-20, dehaze=0), run.presence(target)
    run.close(source)
    run.close(target)
    return dict(raw=reply['raw'])


def rz_case(run):
    document = run.open('rz', texture=25, clarity=35, dehaze=-15)
    recipe = run.recipe(document)
    pixels = digest(run.tiff(document, 'rz-before'))
    path = run.next_path('presence', '.rz')
    run.app.call('save_copy', document_id=document, path=str(path))
    run.close(document)
    reopened = run.app.call('open_document', path=str(path))['document']['id']
    assert run.recipe(reopened) == recipe, 'the recipe differs after the reopen'
    assert digest(run.tiff(reopened, 'rz-after')) == pixels, 'the pixels differ after the reopen'
    reply = run.app.call('redevelop_raw', document_id=reopened, layer=0, raw=dict(dehaze=30))
    assert reply['ok'] is True, reply
    assert run.presence(reopened) == dict(texture=25, clarity=35, dehaze=30), run.presence(reopened)
    run.close(reopened)
    return dict(bytes=path.stat().st_size)


def old_case(run):
    name = 'v1 old exposure'
    (run.presets / digest_name(name)).write_text(json.dumps(
        dict(schema_version=3, name=name, raw=dict(exposure=0.2))))
    document = run.open('old-preset', clarity=30)
    applied = run.app.call('apply_raw_preset', name=name, document_id=document, layer=0)
    assert applied['ok'] is True, applied
    recipe = run.recipe(document)
    assert abs(recipe['values']['exposure'] - 0.2) < 1e-6, recipe['values']
    assert run.presence(document) == dict(texture=0, clarity=30, dehaze=0), run.presence(document)
    run.close(document)

    plain = run.open('old-rz')
    path = run.next_path('plain', '.rz')
    run.app.call('save_copy', document_id=plain, path=str(path))
    run.close(plain)
    reopened = run.app.call('open_document', path=str(path))['document']['id']
    assert run.presence(reopened) is None, run.presence(reopened)
    reply = run.app.call('redevelop_raw', document_id=reopened, layer=0, raw=dict(clarity=20))
    assert reply['ok'] is True, reply
    assert run.presence(reopened) == dict(texture=0, clarity=20, dehaze=0), run.presence(reopened)
    run.close(reopened)
    return dict(preset_schema=3)


def refusal_case(run):
    document = run.plain
    recipe, history = run.recipe(document), run.cursor(document)
    texts = [
        run.app.refused('redevelop_raw', 'raw.texture', '-100', '100', document_id=document, layer=0,
                        raw=dict(texture=101)),
        run.app.refused('redevelop_raw', 'raw.clarity must be a number', document_id=document, layer=0,
                        raw=dict(clarity=True)),
    ]
    assert run.recipe(document) == recipe and run.cursor(document) == history
    name = 'v1 old dehaze'
    file = digest_name(name)
    (run.presets / file).write_text(json.dumps(
        dict(schema_version=3, name=name, raw=dict(dehaze=20))))
    # The codec refuses the file ("…schema 3 with texture, clarity or dehaze, which requires
    # schema 4."), so the shared library lists it as a broken row naming the file, never as a
    # preset, and applying it by name is refused naming it.
    listing = run.app.call('list_raw_presets')
    rows = {row['file']: row for row in listing['broken']}
    assert rows[file]['code'] == 'invalid_schema' and file in rows[file]['reason'], rows[file]
    assert name not in [row['name'] for row in listing['presets']], listing['presets']
    texts.append(rows[file]['reason'])
    texts.append(run.app.refused('apply_raw_preset', name, document_id=document, layer=0, name=name))
    assert run.recipe(document) == recipe and run.cursor(document) == history
    run.close(document)
    return dict(refusals=texts)


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, the copies and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-raw-presence-'))
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
            run_case('zero', lambda: zero_case(run))
            run_case('texture', lambda: slider_case(run, 'texture', 'texture', True))
            run_case('clarity', lambda: slider_case(run, 'clarity', 'clarity', True))
            run_case('dehaze', lambda: slider_case(run, 'dehaze', 'dark', False))
            run_case('8-bit', lambda: eight_bit_case(run))
            run_case('redevelop + undo', lambda: redevelop_case(run))
            run_case('preset', lambda: preset_case(run))
            run_case('sync', lambda: sync_case(run))
            run_case('rz', lambda: rz_case(run))
            run_case('old', lambda: old_case(run))
            run_case('refusal', lambda: refusal_case(run))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
