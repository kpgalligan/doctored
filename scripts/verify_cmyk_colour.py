#!/usr/bin/env python3
"""The #1 phase-5 MCP drive: a CMYK document's COLOUR workflow over MCP —
File ▸ New in CMYK, Assign / Convert to Profile between CMYK profiles and the
`.rz` round trip of a converted document, 8/16 Bits/Channel, the CMYK proof
(Working CMYK, Preserve CMYK Numbers, a plate) and an RGB window's proof
reconciled across Image ▸ Mode, Levels and Curves Auto, the gamut warning of an
RGB document against the working CMYK, `separate_color`, and the exports that
convert to sRGB or keep CMYK.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App`: its own scratch MCP socket, a dummy API key and a PRIVATE
directory for every preference library, Color Settings' among them —
`DR_COLOR_SETTINGS_DIR`, an empty store, so the defaults) and is
terminated by its own context manager; no Doctored process this file did not
start is signalled.

Every fixture is written HERE in pure Python. The document profile is macOS's
Generic CMYK Profile; the second press is an installed Adobe profile
(Coated FOGRA39 when present, else the first other proofable CMYK profile —
the drive FAILS naming the folder when there is none); the RGB proof profile is
a synthetic RGB LUT profile written below from the ICC specification.

- **C1 new.** `new_document` color_mode cmyk, depth 16, white: CMYK at 16 bits
  in the working CMYK, every sampled ink16 [0, 0, 0, 0] at alpha 65535; black:
  K above 50 %.
- **C2 assign / convert / .rz.** On a Generic CMYK fixture: `assign_profile`
  to the second press keeps every probed ink16 exactly and changes
  `cmyk_profile`; `undo` restores it. `convert_profile` to it changes the ink,
  keeps the view closer than Assign did (max view drift at most 0.05 and below
  Assign's), and its `.rz` reopens with the same profile, depth and ink16.
- **C3 depth.** `convert_precision cmyk8`: 8 bits, every probed ink16 on the
  8-bit grid (round(v/257)·257 of the 16-bit value); `cmyk16`: 16 bits, the same
  samples; `convert_precision rgba8` refuses naming cmyk8/cmyk16.
- **C4 proof.** `set_proof_setup profile working_cmyk proof_colors true`, then
  `render_proof` (space display-p3), with `preserve_numbers` and with `plate
  cyan` (space display-p3 each); `set_proof_setup gamut_warning true` is
  admitted on a CMYK document (#1's remaining-gaps batch) and `render_proof`
  then reports gamut_warning true, and with `preserve_numbers` out_of_gamut 0
  (the numbers go to the press unconverted). An RGB window proofing through the
  synthetic RGB profile WITH Gamut Warning, after `convert_mode cmyk`, still has
  Proof Colors on, no failure, Gamut Warning on and the working CMYK as its proof
  profile.
- **C5 Auto.** Per-channel Auto holds the black plate:
  `add_adjustment_layer levels auto {per_channel, 0.5, 0.5}` writes c/m/y
  stages, moves C, M or Y at some probe and leaves every probed K exactly;
  `apply_filter curves auto {per_channel}` does the same; `apply_filter curves
  auto {monochromatic}` (one pair for all four plates, K too) changes the
  ink; `histogram` reports cmyk, c, m, y, k of 256 bins each
  summing to the pixel count. Nothing in the drive writes Color Settings, so the
  instance's private store (`color-settings/color-settings.plist` under its
  folder) is never created.
- **C6 gamut warning.** An RGB document of pure blue beside mid grey, proofed
  through `working_cmyk` with `gamut_warning true`: `render_proof`'s
  out_of_gamut is at least the blue half and below the whole canvas.
- **C7 separate.** `separate_color` on the RGB document: rgb [0, 0, 255] is
  out of gamut with a 3-component in_gamut, rgb [128, 128, 128] is in gamut;
  on the CMYK document color "#808080" answers ink of four components.
- **C8 exports.** `export_image png profile srgb` writes an 8-bit sRGB PNG;
  `export_image tiff bit_depth 16` (profile document) reopens as CMYK with the
  same ink16 at every probe; `export_image jpeg` reopens as CMYK; `export_image
  tiff` at bit_depth 8 on the 16-bit document reopens at 8 bits with every
  probed ink16 round(v/257)·257 of the document's.
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App
from verify_phase1 import png

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
SECOND_PRESS = 'Coated FOGRA39 (ISO 12647-2:2004)'
W, H = 24, 16
CONVERT_VIEW_BOUND = 0.05
PROBES = [(2, 2), (8, 5), (14, 9), (20, 13)]
INKS = [[20, 30, 40, 10], [50, 20, 10, 0], [10, 10, 10, 60], [0, 70, 80, 5]]


def codes(percent):
    return [int(p / 100 * 65535 + 0.5) for p in percent]


def cmyk_tiff(width, height, samples, icc):
    """A little-endian classic chunky 16-bit CMYK TIFF (Photometric 5)."""
    data = struct.pack(f'<{len(samples)}H', *samples)
    body = bytearray(data)

    def place(chunk):
        if len(body) % 2:
            body.append(0)
        at = 8 + len(body)
        body.extend(chunk)
        return at

    bits_at = place(struct.pack('<4H', 16, 16, 16, 16))
    icc_at = place(icc)
    if len(body) % 2:
        body.append(0)

    def short(v):
        return struct.pack('<HH', v, 0)

    entries = [
        (256, 4, 1, struct.pack('<I', width)), (257, 4, 1, struct.pack('<I', height)),
        (258, 3, 4, struct.pack('<I', bits_at)), (259, 3, 1, short(1)),
        (262, 3, 1, short(5)), (273, 4, 1, struct.pack('<I', 8)),
        (277, 3, 1, short(4)), (278, 4, 1, struct.pack('<I', height)),
        (279, 4, 1, struct.pack('<I', len(data))), (284, 3, 1, short(1)),
        (34675, 7, len(icc), struct.pack('<I', icc_at)),
    ]
    out = bytearray(b'II*\x00') + struct.pack('<I', 8 + len(body)) + body
    out += struct.pack('<H', len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack('<HHI', tag, kind, count) + value
    out += struct.pack('<I', 0)
    return bytes(out)


def rgb_lut_profile(name):
    """A v2 `mntr` RGB profile with Lab PCS and 2-node `mft2` A2B0/B2A0 tables
    (ICC.1:2001-04 §6.5.7) — proofable, so it can be a proof profile, and RGB, so
    it has no plates."""
    def mft2(inputs, outputs, clut):
        t = bytearray(b'mft2\0\0\0\0') + bytes([inputs, outputs, 2, 0])
        for v in (1, 0, 0, 0, 1, 0, 0, 0, 1):
            t += struct.pack('>i', v * 65536)
        t += struct.pack('>HH', 2, 2)
        for _ in range(inputs):
            t += struct.pack('>HH', 0, 65535)
        for index in range(2 ** inputs):
            coords = [(index >> (inputs - 1 - axis)) & 1 for axis in range(inputs)]
            for value in clut(coords):
                t += struct.pack('>H', round(min(max(value, 0), 1) * 65535))
        for _ in range(outputs):
            t += struct.pack('>HH', 0, 65535)
        return bytes(t)

    desc = b'desc\0\0\0\0' + struct.pack('>I', len(name) + 1) + name.encode() + b'\0'
    wtpt = b'XYZ \0\0\0\0' + b''.join(struct.pack('>i', round(v * 65536)) for v in (0.9642, 1.0, 0.8249))
    a2b = mft2(3, 3, lambda c: [0.2 + 0.7 * c[1], 0.4 + 0.2 * c[0], 0.4 + 0.2 * c[2]])
    b2a = mft2(3, 3, lambda lab: [lab[1], lab[0], lab[2]])
    tags = [(b'desc', desc), (b'wtpt', wtpt), (b'A2B0', a2b), (b'B2A0', b2a)]
    start = 132 + 12 * len(tags)
    table, payload = bytearray(), bytearray()
    for sig, data in tags:
        table += sig + struct.pack('>II', start + len(payload), len(data))
        payload += data
        while len(payload) % 4:
            payload.append(0)
    header = bytearray(128)
    header[0:4] = struct.pack('>I', start + len(payload))
    header[8:12] = bytes([2, 0x10, 0, 0])
    header[12:16], header[16:20], header[20:24], header[36:40] = b'mntr', b'RGB ', b'Lab ', b'acsp'
    return bytes(header) + struct.pack('>I', len(tags)) + bytes(table) + bytes(payload)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-colour-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_colour.py before any measurement'}

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def sample(doc, x, y):
            reply = call('sample_color', document_id=doc, x=x, y=y)
            assert reply.get('color_mode') == 'cmyk', reply
            return reply

        def ink(doc, x, y):
            return sample(doc, x, y)['ink16']

        def cmyk_profile(doc):
            return call('get_color_profile', document_id=doc).get('cmyk_profile') \
                or call('get_precision', document_id=doc).get('cmyk_profile')

        presses = call('list_installed_profiles', kinds=['cmyk'])['profiles']
        second = next((p for p in presses if p['name'] == SECOND_PRESS and p['proofable']), None) \
            or next((p for p in presses if p['proofable'] and p['name'] != 'Generic CMYK Profile'), None)
        assert second, f'no second proofable CMYK profile is installed: {presses}'
        receipts['second_press'] = second['name']

        # ---- C1: File ▸ New in CMYK ---------------------------------------------------
        made = call('new_document', width=32, height=20, unit='px', color_mode='cmyk', depth=16,
                    background='white')
        new = made['document']['id']
        precision = call('get_precision', document_id=new)
        assert precision.get('color_mode') == 'cmyk' and precision.get('bits_per_channel') == 16, precision
        for point in [(0, 0), (16, 10), (31, 19)]:
            reply = sample(new, *point)
            assert reply['ink16'] == [0, 0, 0, 0] and reply['alpha16'] == 65535, reply
        dark = call('new_document', width=8, height=8, unit='px', color_mode='cmyk', depth=8,
                    background='black')['document']['id']
        black = ink(dark, 4, 4)
        assert black[3] > 0.5 * 65535, black
        receipts['c1'] = {'profile': precision.get('cmyk_profile'), 'black_ink16': black}
        call('close_document', document_id=dark, discard_changes=True)
        call('close_document', document_id=new, discard_changes=True)
        print('PASS c1: File ▸ New makes CMYK paper; black is rich black', flush=True)

        # ---- C2: assign / convert / .rz -------------------------------------------
        samples = []
        for y in range(H):
            for x in range(W):
                samples += codes(INKS[(x // 6 + y // 4) % len(INKS)])
        source = folder / 'grid.tif'
        source.write_bytes(cmyk_tiff(W, H, samples, profile))
        doc = call('open_document', path=str(source))['document']['id']
        before = {p: sample(doc, *p) for p in PROBES}
        original_profile = cmyk_profile(doc)
        assigned = call('assign_profile', document_id=doc, profile='installed', name=second['name'])
        assert assigned.get('changed') is True, assigned
        assign_drift = 0.0
        for p in PROBES:
            got = sample(doc, *p)
            assert got['ink16'] == before[p]['ink16'], (p, got, before[p])
            assign_drift = max(assign_drift, max(abs(a - b) for a, b in
                                                 zip(got['normalized_rgba'][:3], before[p]['normalized_rgba'][:3])))
        assert cmyk_profile(doc) == second['name'], cmyk_profile(doc)
        call('undo', document_id=doc)
        assert cmyk_profile(doc) == original_profile, cmyk_profile(doc)
        converted = call('convert_profile', document_id=doc, profile='installed', name=second['name'])
        assert converted.get('changed') is True, converted
        convert_drift, moved = 0.0, False
        for p in PROBES:
            got = sample(doc, *p)
            moved = moved or got['ink16'] != before[p]['ink16']
            convert_drift = max(convert_drift, max(abs(a - b) for a, b in
                                                   zip(got['normalized_rgba'][:3], before[p]['normalized_rgba'][:3])))
        assert moved, 'Convert re-separates the ink'
        assert convert_drift <= CONVERT_VIEW_BOUND and convert_drift < assign_drift, (convert_drift, assign_drift)
        rz = folder / 'converted.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        again = call('open_document', path=str(rz))['document']['id']
        assert cmyk_profile(again) == second['name'], cmyk_profile(again)
        assert call('get_precision', document_id=again).get('bits_per_channel') == 16
        for p in PROBES:
            assert ink(again, *p) == ink(doc, *p), p
        call('close_document', document_id=again, discard_changes=True)
        receipts['c2'] = {'assign_view_drift': assign_drift, 'convert_view_drift': convert_drift}
        print(f'PASS c2: assign keeps ink, convert keeps appearance ({convert_drift:.4f} < '
              f'{assign_drift:.4f}), and the .rz reopens bit-exact', flush=True)

        # ---- C3: 8/16 Bits/Channel --------------------------------------------------
        sixteen = {p: ink(doc, *p) for p in PROBES}
        call('convert_precision', document_id=doc, mode='cmyk8')
        assert call('get_precision', document_id=doc).get('bits_per_channel') == 8
        eight = {p: ink(doc, *p) for p in PROBES}
        for p in PROBES:
            assert eight[p] == [round(v / 257) * 257 for v in sixteen[p]], (p, eight[p], sixteen[p])
        call('convert_precision', document_id=doc, mode='cmyk16')
        assert call('get_precision', document_id=doc).get('bits_per_channel') == 16
        assert {p: ink(doc, *p) for p in PROBES} == eight
        refused = call('convert_precision', fail=True, document_id=doc, mode='rgba8', flatten=True)
        assert 'cmyk8' in refused and 'cmyk16' in refused, refused
        call('undo', document_id=doc)
        call('undo', document_id=doc)
        receipts['c3'] = {str(p): eight[p] for p in PROBES}
        print('PASS c3: 16 → 8 rounds to the 8-bit grid and 8 → 16 keeps it', flush=True)

        # ---- C4: the CMYK proof and the reconciled RGB window -------------------------
        setup = call('set_proof_setup', document_id=doc, profile='working_cmyk', proof_colors=True)
        assert setup.get('proof_colors') is True, setup
        proofs = {}
        for name, extra in [('working', {}), ('preserve', {'preserve_numbers': True}),
                            ('plate', {'plate': 'cyan'})]:
            reply = call('render_proof', document_id=doc, **extra)
            assert reply.get('space') == 'display-p3', (name, reply)
            proofs[name] = reply
        warned = call('set_proof_setup', document_id=doc, gamut_warning=True)
        assert warned.get('gamut_warning') is True, warned
        flagged = call('render_proof', document_id=doc)
        assert flagged.get('gamut_warning') is True, flagged
        kept = call('render_proof', document_id=doc, preserve_numbers=True)
        assert kept.get('out_of_gamut') == 0, kept
        proofs['gamut_warning'] = flagged
        call('set_proof_setup', document_id=doc, gamut_warning=False)
        rgb_lut = folder / 'rgb-proof.icc'
        rgb_lut.write_bytes(rgb_lut_profile('Drive RGB Proof'))
        refusal = call('render_proof', fail=True, document_id=doc, profile='file', path=str(rgb_lut))
        assert 'is not CMYK' in refusal, refusal
        rgb_path = folder / 'rgb.png'
        png(rgb_path, W, H, bytes([0, 0, 255, 255] * (W * H // 2) + [128, 128, 128, 255] * (W * H // 2)))
        rgb = call('open_document', path=str(rgb_path))['document']['id']
        call('set_proof_setup', document_id=rgb, profile='file', path=str(rgb_lut), proof_colors=True,
             gamut_warning=True)
        call('convert_mode', document_id=rgb, mode='cmyk')
        state = call('get_proof_setup', document_id=rgb)
        assert state.get('proof_colors') is True and state.get('failure') is None, state
        assert state.get('gamut_warning') is True, state
        working = call('render_proof', document_id=rgb)
        assert working.get('space') == 'display-p3', working
        # The canvas proof lands asynchronously: it must not turn proofing off.
        for _ in range(20):
            time.sleep(0.1)
            state = call('get_proof_setup', document_id=rgb)
            assert state.get('failure') is None and state.get('proof_colors') is True, state
        assert state.get('profile_name') != 'Drive RGB Proof', state
        receipts['c4'] = {'proofs': proofs, 'reconciled': state}
        call('close_document', document_id=rgb, discard_changes=True)
        print('PASS c4: the CMYK proof, its plate, and an RGB window reconciled across Mode', flush=True)

        # ---- C5: Levels and Curves Auto, K held ------------------------------------------
        before = [ink(doc, *p) for p in PROBES]
        call('add_adjustment_layer', document_id=doc, op='levels',
             auto={'algorithm': 'per_channel', 'shadows': 0.5, 'highlights': 0.5})
        layers = call('get_document', document_id=doc)['layers']
        params = json.dumps(layers[-1])
        assert any(f'"{key}"' in params for key in ('c', 'm', 'y')), layers[-1]
        leveled = [ink(doc, *p) for p in PROBES]
        assert all(a[3] == b[3] for a, b in zip(leveled, before)), ('Levels Auto holds K', leveled, before)
        assert any(a[:3] != b[:3] for a, b in zip(leveled, before)), ('Levels Auto moves C/M/Y', leveled)
        call('delete_layer', document_id=doc, index=len(layers) - 1)
        call('apply_filter', document_id=doc, layer=0, filter='curves', auto={'algorithm': 'per_channel'})
        curved = [ink(doc, *p) for p in PROBES]
        assert curved != before, 'Curves Auto changed the ink'
        assert all(a[3] == b[3] for a, b in zip(curved, before)), ('Curves Auto holds K', curved, before)
        call('undo', document_id=doc)
        call('apply_filter', document_id=doc, layer=0, filter='curves', auto={'algorithm': 'monochromatic'})
        assert [ink(doc, *p) for p in PROBES] != before, 'monochromatic Curves Auto changed the ink'
        call('undo', document_id=doc)
        assert not (folder / 'app' / 'color-settings' / 'color-settings.plist').exists(), \
            'the private Color Settings store was written'
        histogram = call('histogram', document_id=doc)
        assert histogram.get('color_mode') == 'cmyk', histogram
        for key in ('cmyk', 'c', 'm', 'y', 'k'):
            bins = histogram['channels'][key]
            assert len(bins) == 256, key
            assert abs(sum(bins) - W * H * (4 if key == 'cmyk' else 1)) < 1e-6, (key, sum(bins))
        receipts['c5'] = {'levels_auto': layers[-1], 'levels_ink16': leveled, 'curves_ink16': curved}
        print('PASS c5: per-channel Levels and Curves Auto hold K, monochromatic moves the ink, '
              'and the histogram per ink', flush=True)

        # ---- C6: an RGB document's gamut warning against the working CMYK ------------------
        rgb = call('open_document', path=str(rgb_path))['document']['id']
        warned = call('render_proof', document_id=rgb, profile='working_cmyk', gamut_warning=True)
        flagged = warned.get('out_of_gamut', 0)
        assert W * H // 2 <= flagged < W * H, warned
        receipts['c6'] = {'out_of_gamut': flagged}
        print(f'PASS c6: the gamut warning flags the blue half ({flagged} px)', flush=True)

        # ---- C7: separate_color -------------------------------------------------------------
        blue = call('separate_color', document_id=rgb, rgb=[0, 0, 255])
        assert blue['out_of_gamut'] is True and len(blue['in_gamut']) == 3, blue
        grey = call('separate_color', document_id=rgb, rgb=[128, 128, 128])
        assert grey['out_of_gamut'] is False, grey
        on_cmyk = call('separate_color', document_id=doc, color='#808080')
        assert len(on_cmyk['ink_percent']) == 4, on_cmyk
        receipts['c7'] = {'blue': blue, 'grey': grey, 'cmyk_document': on_cmyk}
        call('close_document', document_id=rgb, discard_changes=True)
        print('PASS c7: separate_color reads ink, the "!" verdict and the in-gamut colour', flush=True)

        # ---- C8: exports ---------------------------------------------------------------------
        srgb_png = folder / 'export.png'
        report = call('export_image', document_id=doc, path=str(srgb_png),
                      settings={'format': 'png', 'profile': 'srgb'})
        assert report.get('bit_depth') == 8 and report.get('profile') == 'srgb', report
        assert srgb_png.read_bytes()[:8] == b'\x89PNG\r\n\x1a\n'
        kept = folder / 'export.tif'
        call('export_image', document_id=doc, path=str(kept),
             settings={'format': 'tiff', 'bit_depth': 16, 'profile': 'document'})
        reopened = call('open_document', path=str(kept))['document']['id']
        for p in PROBES:
            assert ink(reopened, *p) == ink(doc, *p), p
        call('close_document', document_id=reopened, discard_changes=True)
        jpeg = folder / 'export.jpg'
        call('export_image', document_id=doc, path=str(jpeg),
             settings={'format': 'jpeg', 'profile': 'document'})
        reopened = call('open_document', path=str(jpeg))['document']['id']
        assert call('get_color_profile', document_id=reopened).get('color_mode') == 'cmyk'
        call('close_document', document_id=reopened, discard_changes=True)
        eight = folder / 'eight.tif'
        report8 = call('export_image', document_id=doc, path=str(eight),
                       settings={'format': 'tiff', 'bit_depth': 8, 'profile': 'document'})
        assert report8.get('bit_depth') == 8, report8
        reopened = call('open_document', path=str(eight))['document']['id']
        assert call('get_precision', document_id=reopened).get('bits_per_channel') == 8
        for p in PROBES:
            assert ink(reopened, *p) == [round(v / 257) * 257 for v in ink(doc, *p)], p
        call('close_document', document_id=reopened, discard_changes=True)
        receipts['c8'] = {'png': report}
        call('close_document', document_id=doc, discard_changes=True)
        print('PASS c8: exports convert to sRGB or keep CMYK bit-exact', flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True, default=str) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
