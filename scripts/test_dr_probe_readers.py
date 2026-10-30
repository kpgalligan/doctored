#!/usr/bin/env python3
"""The independent Python `.rz` readers against files the core writes.

`raw-probe/native.py` (`identity_body`, `source_profile_refusals`,
`warp_entries`, `knockout_word`) and the precision-probe readers
(`fixtures.identity_tables`, `retained_linked_fixtures.read_native_sources`)
peel the source profile-refusal word 56/57 and its `RSPR` table and the
projective word 51/52 and its `RSPJ` table; `native.py` also peels the warp
word 53/54 and its `WARP` table, the byte knockout word 49 and the typed
global-light word 58 and its `LGLT` table, which the precision-probe readers
refuse. The committed fixtures always run. The samples
are written by the core's ignored generator; point `DR_REFUSAL_SAMPLES` at
them to run that case:

    mkdir -p "$DIR" && DR_OUT="$DIR" make t F=source_profile_refusal \\
        T=write_refusal_samples_for_python_readers A="--ignored --exact"
    DR_REFUSAL_SAMPLES="$DIR" python3 -B scripts/test_dr_probe_readers.py -v
"""
import importlib.util
import os
from pathlib import Path
import struct
import sys
import unittest

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts/precision-probe'))
import fixtures  # noqa: E402
import retained_linked_fixtures  # noqa: E402

for _module in (fixtures, retained_linked_fixtures):
    assert Path(_module.__file__).resolve().parent == ROOT / 'scripts/precision-probe', _module.__file__
_spec = importlib.util.spec_from_file_location('raw_probe_native', ROOT / 'scripts/raw-probe/native.py')
native = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(native)

# The sentence the generator places, restated here rather than read back.
S = ('The embedded ICC profile “Generic Gray Gamma 2.2 Profile” is not a supported '
     'RGB matrix/TRC profile; the image was opened as sRGB.')
SECOND = 'Second.'
STORED = ROOT / 'core/tests/fixtures'
# file -> (version words from offset 4, refusals, RSPJ layers, closed groups, inner body word,
#          source assets or None, WARP layers, knockout word)
SAMPLES = {
    'byte.rz': ((45, 56, 16), {0: S}, [], None, 16, 1, [], False),
    'typed.rz': ((46, 57, 17), {0: S}, [], None, 17, 1, [], False),
    'byte_projective.rz': ((45, 56, 51, 16), {0: S}, [1], None, 16, 1, [], False),
    'typed_projective.rz': ((46, 57, 52, 17), {0: S}, [1], None, 17, 1, [], False),
    'typed_closed_group.rz': ((46, 57, 55, 17), {0: S}, [], [1], 17, None, [], False),
    'byte_second_only.rz': ((45, 56, 16), {1: SECOND}, [], None, 16, 2, [], False),
    'typed_two.rz': ((46, 57, 17), {0: S, 1: SECOND}, [], None, 17, 2, [], False),
    'byte_warp.rz': ((45, 56, 53, 16), {0: S}, [], None, 16, None, [0], False),
    'typed_warp.rz': ((46, 57, 54, 17), {0: S}, [], None, 17, None, [0], False),
    'byte_knockout.rz': ((45, 49, 16), {}, [], None, 16, None, [], True),
    'byte_all.rz': ((45, 56, 53, 51, 49, 16), {0: S}, [1], None, 16, None, [0], True),
    'typed_all.rz': ((46, 57, 55, 54, 52, 17), {0: S}, [2], [1], 17, None, [0], False),
}
# The precision-probe readers do not learn 53/54/49: the body version word refuses these.
PRECISION_REFUSED = {'byte_warp.rz', 'typed_warp.rz', 'byte_knockout.rz', 'byte_all.rz', 'typed_all.rz'}
# `warp_fixture::field(3, 3, ..)`, restated: the generator's displacement lattice.
FIELD = [(0.35 + 0.58 * fx - 0.31 * fy + 0.12 * fx * fy, -0.27 + 0.33 * fy + 0.19 * fx - 0.17 * fx * fy)
         for fy, fx in ((y / 2, x / 2) for y in range(3) for x in range(3))]


def words(data):
    return struct.unpack_from('<6I', data, 4)


def rspr(data):
    """(table start, table end) found from the format alone: RSPR ends where LIDS starts."""
    end = data.rfind(b'LIDS')
    return data.rfind(b'RSPR', 0, end), end


class DrProbeReaderTests(unittest.TestCase):
    def test_committed_refusal_free_files_read_as_before(self):
        for folder, outer, inner in [('rz_byte_53', 45, 16), ('rz_typed_55', 46, 17)]:
            with self.subTest(folder=folder):
                path = STORED / folder / 'placed_source.rz'
                data = path.read_bytes()
                self.assertEqual(words(data)[:2], (outer, inner))
                body, shift, ids, word, projective = native.identity_body(data, inner={outer: (inner,)})
                self.assertEqual((struct.unpack_from('<I', body, 4)[0], shift, word, projective),
                                 (inner, 4, outer, {}))
                self.assertEqual(native.source_profile_refusals(data), {})
                self.assertEqual(fixtures.identity_tables(data), (8, data.rfind(b'LIDS'), {}, {}))
                sources = retained_linked_fixtures.read_native_sources(path)
                self.assertEqual((sources['version'], len(sources['assets']), sources['profile_refusals'],
                                  sources['projective']), (inner, 1, {}, {}))
        # Warp-free files from the 51/52-era writers: no 51/52/53/54 word, a negative control.
        for folder, outer, inner in [('rz_byte_51', 45, 10), ('rz_typed_52', 46, 13)]:
            with self.subTest(folder=folder):
                data = (STORED / folder / 'warp_fixture.rz').read_bytes()
                self.assertEqual(words(data)[:2], (outer, inner))
                body, shift, _, word, projective = native.identity_body(data, inner={outer: (inner,)})
                self.assertEqual((struct.unpack_from('<I', body, 4)[0], shift, word, projective),
                                 (inner, 4, outer, {}))
                self.assertEqual((native.warp_entries(data), native.knockout_word(data)), ({}, False))
                self.assertEqual(fixtures.identity_tables(data), (8, data.rfind(b'LIDS'), {}, {}))

    def test_typed_global_light_word(self):
        # Word 58 first inside 46 and its fixed 16-byte LGLT table just
        # before LIDS, assembled here around a committed refusal-free file.
        source = STORED / 'rz_typed_55' / 'placed_source.rz'
        data = source.read_bytes()
        lids = data.rfind(b'LIDS')
        table = b'LGLT' + struct.pack('<Iff', 1, 45.0, 60.0)
        lit = data[:8] + struct.pack('<I', 58) + data[8:lids] + table + data[lids:]
        self.assertEqual(words(lit)[:3], (46, 58, 17))
        body, shift, ids, word, projective = native.identity_body(lit, inner={46: (17,)})
        self.assertEqual((struct.unpack_from('<I', body, 4)[0], shift, word, projective), (17, 8, 46, {}))
        self.assertEqual(body, native.identity_body(data)[0])
        self.assertEqual(ids, native.identity_body(data)[2])
        at = lit.rfind(b'LGLT')
        with self.assertRaises(AssertionError):
            native.identity_body(lit[:at + 4] + struct.pack('<I', 2) + lit[at + 8:])
        # The precision-probe readers keep refusing the unknown word by name.
        self.assertEqual(struct.unpack_from('<I', lit, fixtures.identity_tables(lit)[0])[0], 58)

    @unittest.skipUnless(os.environ.get('DR_REFUSAL_SAMPLES'), 'set DR_REFUSAL_SAMPLES to the generator output')
    def test_core_written_refusal_samples(self):
        folder = Path(os.environ['DR_REFUSAL_SAMPLES'])
        self.assertEqual(sorted(p.name for p in folder.glob('*.rz')), sorted(SAMPLES))
        for name, (expected_words, refusals, rspj, closed, inner, assets, warp, knockout) in SAMPLES.items():
            with self.subTest(sample=name):
                data = (folder / name).read_bytes()
                self.assertEqual(words(data)[:len(expected_words)], expected_words)
                body, shift, _, word, projective = native.identity_body(
                    data, inner={45: (16,), 46: (17,)})
                self.assertEqual((struct.unpack_from('<I', body, 4)[0], word), (inner, expected_words[0]))
                self.assertEqual(sorted(projective), rspj)
                self.assertEqual(native.source_profile_refusals(data), refusals)
                self.assertEqual(fixtures.identity_tables(data)[2], refusals)
                entries = native.warp_entries(data)
                self.assertEqual((sorted(entries), native.knockout_word(data)), (warp, knockout))
                for entry in entries.values():
                    self.assertEqual((entry['target'], entry['filter'], entry['columns'], entry['rows'],
                                      entry['displacements'], entry['mask']), ('pixels', 'bilinear', 3, 3, FIELD, None))
                    self.assertIsNotNone(entry['pixels'])
                if refusals:
                    at, end = rspr(data)
                    self.assertEqual(native.refusal_table(data, 12, end), (at, refusals))
                    self.assertEqual(fixtures.refusal_table(data, 12, end), (at, refusals))
                else:
                    self.assertFalse({56, 57} & set(words(data)[:len(expected_words)]))
                    at = data.rfind(b'LIDS')
                    # The tables sit between the inner body's end and LIDS; RSPR would be the last one.
                    self.assertEqual(data.find(b'RSPR', len(body) + shift, at), -1)
                if closed is not None:
                    self.assertEqual(native.group_open_table(data, 16, at)[1], closed)
                if assets is not None:
                    sources = retained_linked_fixtures.read_native_sources(folder / name)
                    self.assertEqual((sources['version'], len(sources['assets']), sources['profile_refusals']),
                                     (inner, assets, refusals))
                    self.assertEqual(sources['projective'], projective)
                if name in PRECISION_REFUSED:
                    # They stop at the first 53/54/49 word and name it as the refused version.
                    stop = next(w for w in expected_words if w in (53, 54, 49))
                    self.assertEqual(struct.unpack_from('<I', data, fixtures.identity_tables(data)[0])[0], stop)
                    with self.assertRaisesRegex(AssertionError, rf'^{stop}$'):
                        retained_linked_fixtures.read_native_sources(folder / name)

        byte = (folder / 'byte.rz').read_bytes()
        at, end = rspr(byte)
        zero_length = bytearray(byte)
        zero_length[at + 16:at + 20] = struct.pack('<I', 0)
        trailing = byte[:end] + b'\0' + byte[end:]
        two = (folder / 'typed_two.rz').read_bytes()
        at, end = rspr(two)
        first_length = struct.unpack_from('<I', two, at + 16)[0]
        first = two[at + 12:at + 20 + first_length]
        second = two[at + 20 + first_length:end]
        self.assertEqual(struct.unpack_from('<2I', second), (1, len(SECOND)))
        swapped = two[:at + 12] + second + first + two[end:]
        duplicate = bytearray(two)
        duplicate[at + 20 + first_length:at + 24 + first_length] = struct.pack('<I', 0)

        def patched(offset, value):
            mutated = bytearray(byte)
            mutated[offset:offset + len(value)] = value
            return bytes(mutated)

        at, end = rspr(byte)
        for case, data in [('entry length 0', bytes(zero_length)), ('indices out of order', swapped),
                           ('trailing byte before LIDS', trailing), ('schema 2', patched(at + 4, struct.pack('<I', 2))),
                           ('count 0', patched(at + 8, struct.pack('<I', 0))), ('duplicate index', bytes(duplicate)),
                           ('entry length 4097', patched(at + 16, struct.pack('<I', 4097))),
                           ('NUL in sentence', patched(at + 20, b'\0')),
                           ('invalid UTF-8', patched(at + 20, b'\xff'))]:
            with self.subTest(mutation=case):
                for reader in (native.source_profile_refusals, native.identity_body, fixtures.identity_tables):
                    with self.assertRaisesRegex(AssertionError, 'Malformed RSPR table'):
                        reader(data)

        warped = (folder / 'byte_warp.rz').read_bytes()
        end, _ = rspr(warped)
        at = warped.rfind(b'WARP', 0, end)
        lattice = at + 28  # the first entry's 3 x 3 (dx, dy) pairs follow its 16 header bytes
        plane = lattice + 9 * 16  # then the pixel plane: presence, x, y, w, h, u64 length
        length = struct.unpack_from('<Q', warped, plane + 17)[0]

        def warp_patched(offset, value):
            mutated = bytearray(warped)
            mutated[offset:offset + len(value)] = value
            return bytes(mutated)

        for case, data in [('schema 2', warp_patched(at + 4, struct.pack('<I', 2))),
                           ('count 0', warp_patched(at + 8, struct.pack('<I', 0))),
                           ('filter 2', warp_patched(at + 17, b'\x02')),
                           ('zeroed lattice', warp_patched(lattice, bytes(9 * 16))),
                           ('plane length + 1', warp_patched(plane + 17, struct.pack('<Q', length + 1))),
                           ('trailing byte before RSPR', warped[:end] + b'\0' + warped[end:])]:
            with self.subTest(mutation=case):
                for reader in (native.warp_entries, native.identity_body):
                    with self.assertRaisesRegex(AssertionError, 'Malformed WARP table'):
                        reader(data)

        projected = (folder / 'byte_projective.rz').read_bytes()
        end, _ = rspr(projected)
        at = projected.rfind(b'RSPJ', 0, end)
        with self.subTest(mutation='RSPJ schema 2'):
            with self.assertRaisesRegex(AssertionError, 'Malformed RSPJ table'):
                fixtures.identity_tables(projected[:at + 4] + struct.pack('<I', 2) + projected[at + 8:])


if __name__ == '__main__':
    unittest.main()
