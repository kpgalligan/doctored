#!/usr/bin/env python3
"""MCP drive for HDR Toning: `hdr_toning` (Image > Adjustments >
HDR Toning..., and Image > Mode > 8/16 Bits/Channel on a 32-bit document) end
to end.

The script owns its app instance (`verify_v1_stack_align.App`): the Debug app
(or `DR_VERIFY_APP`) on its own scratch MCP socket with `DR_BACKGROUND=1`, a
dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s.
It terminates only that process and never launches Photoshop. Every expected
number is computed here.

THE CHART. A 32-bit float TIFF written here, 32 x 2: row 0 neutral
v = 2^(-10 + 16x/31) (16 stops, linear light), row 1 the same times
(1, 0.6, 0.25).

THE ORACLES, fixed before measuring:
- Exposure and Gamma: u = v * 2^e, clamp(sign(u)|u|^(1/g), 0, 1) in f64;
- Highlight Compression: the extended Reinhard curve
  u(1 + u/W^2)/(1 + u) with W the chart's largest RGB sample;
- both meet the document profile's own forward curve through the app's
  identity: a second float chart holding the oracle's linear values, toned
  at Exposure and Gamma (0, 1) (the core test pins that identity to Convert
  Precision bit for bit);
- Equalize Histogram: the table written again here (65536 levels of
  log2(max(v, 2^-20)) over the chart's own log range, every R, G and B sample
  weighted 1, lut = round(65535 (cdf - cdf_min) / (N - cdf_min))), which IS the
  encoded code;
- Local Adaptation on a FLAT chart (one colour c): the log range is one level,
  so span = 1, t = 0 and the guided base's delta d = 0, and the map is
  Y' = 2^(-1/gamma + exposure), c' = c Y'/Y (Y the Rec. 709 luminance), then
  Saturation's pull clamp01(A + (c' - A)(1 + s/100)) about the ROMM Y row
  A = (0.288040, 0.711874, 0.000086) . c', through the same identity.
BOUNDS: 1 code of 65535 per channel at 16 bits; 1 code of 255 at 8 bits (the
narrowing of the 16-bit result, round(v * 255 / 65535)); an 8-bit source at
(0, 1) within 1 code of itself; a 16-bit source within 2 codes of 65535.

CASES
- each method on the float chart against its oracle, at 16 bits;
- float -> 8: the identity at bit_depth 8 against the 16-bit identity narrowed;
- an 8-bit and a 16-bit source keep their depth (and refuse another);
- a three-layer stack flattens to one "Background", `flattened` is true, and
  one `undo` restores the three layers;
- a CMYK document is refused by name;
- argument refusals: exposure with another method, gamma 0, bit_depth 12, an
  unknown method;
- `merge_to_hdr` of a three-frame bracket, then `hdr_toning`;
- Actions: `start_recording` / `stop_recording` record one `hdr_toning` step
  with every value explicit, and `run_action` replays it to the same codes;
- Local Adaptation (measured against Photoshop, it differs): the
  flat chart against the closed form above (1 code of 65535); the defaults on
  the chart answer one layer unlike Exposure and Gamma (0, 1); an identity
  toning_curve equals none; 8- and 16-bit sources keep their depth; refusals
  (radius 0, strength 5, detail 400, a one-point curve, radius with
  exposure_gamma, gamma 2.5 with local_adaptation); Actions record and replay
  with every Local Adaptation value explicit.
- Presets: the built-in Default equals `{method:
  local_adaptation}` on the float chart, and `{preset: Default, detail: 50}`
  equals `{method: local_adaptation, detail: 50}`; an unknown name and
  "Scott5" refuse with the unknown-preset sentence naming Default; a user
  preset written into the instance's library applies by name, records its
  explicit values with no `preset` key, replays the same after its file is
  deleted, and then refuses; recording `{preset: Default}` keeps no `preset`
  key and replays to the same codes; a user preset named "Scott5" applies by
  name; a user file with a stray key inside `hdr_toning` does not apply.

Usage: python3 scripts/verify_v1_hdr_toning.py [--output DIR]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_phase1 import png  # noqa: E402
from verify_v1_stack_align import App, close, documents, rows  # noqa: E402

CHART = (32, 2)
TINT = (1.0, 0.6, 0.25)
LOG_FLOOR = 2.0 ** -20
FLAT = (0.4, 0.2, 0.1)
LUMA = (0.2126, 0.7152, 0.0722)
ROMM_Y = (0.288040, 0.711874, 0.000086)
LOCAL_KEYS = ('radius', 'strength', 'smooth_edges', 'gamma', 'exposure', 'detail', 'shadow',
              'highlight', 'vibrance', 'saturation', 'toning_curve')


# ----------------------------------------------------------------- files --

def tiff(path, size, pixels, bits):
    """A baseline little-endian RGB TIFF, one strip, no ICC: floats at 32 bits
    (SampleFormat 3), codes at 16."""
    width, height = size
    code = {32: '<3f', 16: '<3H'}[bits]
    body = b''.join(struct.pack(code, *p) for p in pixels)
    entries = []

    def short(tag, *values):
        entries.append((tag, 3, len(values), struct.pack(f'<{len(values)}H', *values)))

    def long(tag, *values):
        entries.append((tag, 4, len(values), struct.pack(f'<{len(values)}I', *values)))

    long(256, width)
    long(257, height)
    short(258, bits, bits, bits)
    short(259, 1)
    short(262, 2)
    long(273, 0)
    short(277, 3)
    long(278, height)
    long(279, len(body))
    short(284, 1)
    short(339, *([3, 3, 3] if bits == 32 else [1, 1, 1]))
    entries.sort(key=lambda e: e[0])
    start = 8 + 2 + 12 * len(entries) + 4
    extra = bytearray()
    placed = []
    for tag, kind, count, payload in entries:
        if len(payload) <= 4:
            placed.append((tag, kind, count, payload.ljust(4, b'\0')))
        else:
            placed.append((tag, kind, count, struct.pack('<I', start + len(extra))))
            extra += payload
    pixels_at = start + len(extra)
    pixels_at += pixels_at % 2
    head = bytearray(b'II*\0' + struct.pack('<I', 8) + struct.pack('<H', len(placed)))
    for tag, kind, count, value in placed:
        if tag == 273:
            value = struct.pack('<I', pixels_at)
        head += struct.pack('<HHI', tag, kind, count) + value
    head += struct.pack('<I', 0)
    Path(path).write_bytes((bytes(head) + bytes(extra)).ljust(pixels_at, b'\0') + body)
    return Path(path)


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def chart_pixels():
    out = []
    for y in range(CHART[1]):
        for x in range(CHART[0]):
            v = 2.0 ** (-10 + 16 * x / (CHART[0] - 1))
            tint = (1.0, 1.0, 1.0) if y == 0 else TINT
            out.append(tuple(f32(v * t) for t in tint))
    return out


def png8(path, size, codes):
    png(path, size[0], size[1], bytes(c for p in codes for c in (*p, 255)))
    return Path(path)


# --------------------------------------------------------------- oracles --

def eg(v, exposure, gamma):
    u = v * 2.0 ** exposure
    return min(1.0, max(0.0, math.copysign(abs(u) ** (1 / gamma), u)))


def hc(v, white):
    u = max(v, 0.0)
    return min(1.0, u * (1 + u / (white * white)) / (1 + u))


def eh_codes(pixels):
    def log(v):
        return math.log2(max(v, LOG_FLOOR))
    logs = [log(v) for p in pixels for v in p]
    low, high = min(logs), max(logs)

    def level(v):
        return round(min(1.0, max(0.0, (log(v) - low) / (high - low))) * 65535)
    bins = {}
    for p in pixels:
        for v in p:
            bins[level(v)] = bins.get(level(v), 0) + 1
    total = sum(bins.values())
    floor = bins[min(bins)]
    cdf, running = {}, 0
    for key in sorted(bins):
        running += bins[key]
        cdf[key] = running
    return [tuple(round(65535 * (cdf[level(v)] - floor) / (total - floor)) for v in p)
            for p in pixels]


def la_flat(c, gamma=1.0, exposure=0.0, saturation=20.0):
    """Local Adaptation of a flat colour (span 1, t = 0, d = 0), linear and clamped."""
    target = 2.0 ** (-1.0 / gamma + exposure)
    y = sum(w * v for w, v in zip(LUMA, c))
    scaled = [v * target / y for v in c]
    if saturation == 0:
        return [min(1.0, max(0.0, v)) for v in scaled]
    anchor = sum(w * v for w, v in zip(ROMM_Y, scaled))
    return [min(1.0, max(0.0, anchor + (v - anchor) * (1 + saturation / 100))) for v in scaled]


# -------------------------------------------------------------- plumbing --

def open_document(app, path):
    return app.call('open_document', path=str(path))['document']['id']


def codes16(app, document):
    out = []
    for y in range(CHART[1]):
        for x in range(CHART[0]):
            reply = app.call('sample_color', document_id=document, x=x, y=y)
            out.append(tuple(reply['center_rgba16'][:3]))
    return out


def codes8(app, document, size=CHART):
    out = []
    for y in range(size[1]):
        for x in range(size[0]):
            reply = app.call('sample_color', document_id=document, x=x, y=y)
            out.append((reply['r'], reply['g'], reply['b']))
    return out


def within(got, want, bound, label):
    worst = 0
    for i, (a, b) in enumerate(zip(got, want)):
        for c in range(3):
            worst = max(worst, abs(a[c] - b[c]))
            assert abs(a[c] - b[c]) <= bound, (label, i, c, a, b)
    assert len(got) == len(want), (label, len(got), len(want))
    return worst


def toned(app, path, **arguments):
    """Opens `path`, tones it and answers (reply, document)."""
    document = open_document(app, path)
    reply = app.call('hdr_toning', document_id=document, **arguments)
    assert reply['ok'] is True, reply
    summary = reply['document']
    assert summary['layer_count'] == 1, summary
    assert [row['name'] for row in rows(app, document)] == ['Background']
    return reply, document


class Drive:
    def __init__(self, app, folder):
        self.app = app
        self.folder = folder
        self.pixels = chart_pixels()
        self.chart = tiff(folder / 'chart.tif', CHART, self.pixels, 32)

    def encoded(self, linear, tag):
        """The app's own forward curve of `linear` (the identity on a chart of them)."""
        path = tiff(self.folder / f'encode-{tag}.tif', CHART, [tuple(f32(v) for v in p) for p in linear], 32)
        reply, document = toned(self.app, path, method='exposure_gamma')
        assert reply['bit_depth'] == 16, reply
        try:
            return codes16(self.app, document)
        finally:
            close(self.app, document)

    def exposure_gamma(self):
        detail = {}
        for exposure, gamma in ((0, 1), (1.5, 1.8), (-2, 0.6)):
            reply, document = toned(self.app, self.chart, method='exposure_gamma',
                                    exposure=exposure, gamma=gamma)
            try:
                assert reply['bit_depth'] == 16 and reply['method'] == 'exposure_gamma', reply
                assert reply['document']['precision'] == 'rgba16', reply['document']
                got = codes16(self.app, document)
            finally:
                close(self.app, document)
            want = self.encoded([[eg(v, exposure, gamma) for v in p] for p in self.pixels],
                                f'eg-{exposure}-{gamma}')
            detail[f'{exposure},{gamma}'] = within(got, want, 1, f'EG {exposure} {gamma}')
        return detail

    def highlight_compression(self):
        white = max(1.0, max(v for p in self.pixels for v in p))
        reply, document = toned(self.app, self.chart, method='highlight_compression')
        try:
            got = codes16(self.app, document)
        finally:
            close(self.app, document)
        want = self.encoded([[hc(v, white) for v in p] for p in self.pixels], 'hc')
        assert max(max(p) for p in got) == 65535, 'the brightest sample lands at white'
        return dict(white=white, worst=within(got, want, 1, 'HC'))

    def equalize_histogram(self):
        reply, document = toned(self.app, self.chart, method='equalize_histogram')
        try:
            got = codes16(self.app, document)
        finally:
            close(self.app, document)
        return dict(worst=within(got, eh_codes(self.pixels), 1, 'EH'))

    def float_to_eight(self):
        reply, document = toned(self.app, self.chart, method='exposure_gamma', bit_depth=8)
        try:
            assert reply['bit_depth'] == 8 and reply['document']['precision'] == 'rgba8', reply
            got = codes8(self.app, document)
        finally:
            close(self.app, document)
        sixteen = self.encoded([list(p) for p in self.pixels], 'identity')
        want = [tuple(round(v * 255 / 65535) for v in p) for p in sixteen]
        return dict(worst=within(got, want, 1, 'float -> 8'))

    def sources_keep_depth(self):
        size = (16, 2)
        codes = [(x * 16, 255 - x * 16, (x * 37) % 256) for _ in range(size[1]) for x in range(size[0])]
        eight = png8(self.folder / 'eight.png', size, codes)
        reply, document = toned(self.app, eight, method='exposure_gamma')
        try:
            assert reply['bit_depth'] == 8 and reply['document']['precision'] == 'rgba8', reply
            worst8 = within(codes8(self.app, document, size), codes, 1, '8-bit source')
            self.app.refused('hdr_toning', 'bit_depth must be 8', document_id=document,
                             method='exposure_gamma', bit_depth=16)
        finally:
            close(self.app, document)
        wide = [(x * 4000 + 7, 65535 - x * 4000, (x * 9001) % 65536)
                for _ in range(size[1]) for x in range(size[0])]
        sixteen = tiff(self.folder / 'sixteen.tif', size, wide, 16)
        reply, document = toned(self.app, sixteen, method='exposure_gamma')
        try:
            assert reply['bit_depth'] == 16 and reply['document']['precision'] == 'rgba16', reply
            got = []
            for y in range(size[1]):
                for x in range(size[0]):
                    sample = self.app.call('sample_color', document_id=document, x=x, y=y)
                    got.append(tuple(sample['center_rgba16'][:3]))
            worst16 = within(got, wide, 2, '16-bit source')
            self.app.refused('hdr_toning', 'bit_depth must be 16', document_id=document,
                             method='highlight_compression', bit_depth=8)
        finally:
            close(self.app, document)
        return dict(eight=worst8, sixteen=worst16)

    def flatten_and_undo(self):
        size = (64, 8)
        paths = [png8(self.folder / f'layer{k}.png', size,
                      [((40 * k + x) % 256, 90, 200 - 20 * k)
                       for _ in range(size[1]) for x in range(size[0])])
                 for k in range(3)]
        document = self.app.call('load_files_into_stack',
                                 paths=[str(p) for p in paths])['document']['id']
        try:
            assert len(rows(self.app, document)) == 3
            reply = self.app.call('hdr_toning', document_id=document, method='equalize_histogram')
            assert reply['flattened'] is True and reply['document']['layer_count'] == 1, reply
            assert [row['name'] for row in rows(self.app, document)] == ['Background']
            self.app.call('undo', document_id=document)
            restored = rows(self.app, document)
            assert len(restored) == 3, restored
            return dict(restored=[row['name'] for row in restored])
        finally:
            close(self.app, document)

    def cmyk_refusal(self):
        path = png8(self.folder / 'cmyk-source.png', (8, 8), [(120, 60, 30)] * 64)
        document = open_document(self.app, path)
        try:
            self.app.call('convert_mode', document_id=document, mode='cmyk')
            text = self.app.refused('hdr_toning', 'unavailable in a CMYK document',
                                    document_id=document, method='exposure_gamma')
            return dict(text=text[:200])
        finally:
            close(self.app, document)

    def argument_refusals(self):
        document = open_document(self.app, self.chart)
        try:
            cases = [
                (dict(method='highlight_compression', exposure=1),
                 'exposure applies to methods exposure_gamma and local_adaptation only'),
                (dict(method='exposure_gamma', gamma=0), 'gamma'),
                (dict(method='exposure_gamma', bit_depth=12), 'bit_depth must be 8 or 16'),
                (dict(method='posterize'), 'method must be'),
            ]
            for arguments, needle in cases:
                self.app.refused('hdr_toning', needle, document_id=document, **arguments)
            history = self.app.call('list_history', document_id=document)
            assert len(rows(self.app, document)) == 1 and history['cursor'] == 0, history
            return dict(cases=len(cases))
        finally:
            close(self.app, document)

    def after_merge(self):
        size = (64, 8)
        frames = []
        for k, ev in enumerate((-2, 0, 2)):
            codes = []
            for _ in range(size[1]):
                for x in range(size[0]):
                    linear = min(1.0, 0.002 * 2 ** (x / 6) * 2 ** ev)
                    encoded = 12.92 * linear if linear <= 0.0031308 else 1.055 * linear ** (1 / 2.4) - 0.055
                    value = round(encoded * 255)
                    codes.append((value, value, value))
            frames.append(png8(self.folder / f'bracket{k}.png', size, codes))
        before = documents(self.app)
        merged = self.app.call('merge_to_hdr', paths=[str(p) for p in frames], evs=[-2, 0, 2],
                               align=False)
        document = merged['document']['id']
        try:
            assert merged['document']['precision'] == 'rgba32_float', merged['document']
            reply = self.app.call('hdr_toning', document_id=document, method='highlight_compression')
            assert reply['bit_depth'] == 16 and reply['document']['precision'] == 'rgba16', reply
            assert reply['document']['layer_count'] == 1, reply
            return dict(opened=len(documents(self.app)) - len(before))
        finally:
            close(self.app, document)

    def actions(self):
        name = 'v1 hdr toning'
        document = open_document(self.app, self.chart)
        try:
            self.app.call('start_recording', discard=True)
            self.app.call('hdr_toning', document_id=document, method='exposure_gamma',
                          exposure=1.5, gamma=1.8)
            recorded = self.app.call('stop_recording', name=name)
            steps = recorded['steps']
            assert [step['tool'] for step in steps] == ['hdr_toning'], steps
            arguments = steps[0]['arguments']
            assert arguments.get('method') == 'exposure_gamma', arguments
            assert arguments.get('exposure') == 1.5 and arguments.get('gamma') == 1.8, arguments
            first = codes16(self.app, document)
        finally:
            close(self.app, document)
        again = open_document(self.app, self.chart)
        try:
            self.app.call('run_action', document_id=again, name=name)
            assert len(rows(self.app, again)) == 1
            assert codes16(self.app, again) == first, 'the replay gives the same codes'
            return dict(arguments=arguments)
        finally:
            close(self.app, again)

    # ------------------------------------------------- Local Adaptation (4b) --

    def la_closed_form(self):
        flat = tiff(self.folder / 'flat.tif', CHART, [tuple(f32(v) for v in FLAT)] * (CHART[0] * CHART[1]), 32)
        detail = {}
        for arguments, oracle in (
            (dict(), dict()),
            (dict(exposure=0.5, gamma=1.5, saturation=0, detail=200),
             dict(exposure=0.5, gamma=1.5, saturation=0)),
        ):
            reply, document = toned(self.app, flat, method='local_adaptation', **arguments)
            try:
                assert reply['bit_depth'] == 16 and reply['method'] == 'local_adaptation', reply
                got = codes16(self.app, document)
            finally:
                close(self.app, document)
            linear = la_flat([f32(v) for v in FLAT], **oracle)
            want = self.encoded([linear] * (CHART[0] * CHART[1]), f'la-{len(arguments)}')
            detail[json.dumps(arguments)] = within(got, want, 1, f'LA flat {arguments}')
        return detail

    def la_defaults(self):
        reply, document = toned(self.app, self.chart, method='local_adaptation')
        try:
            assert reply['bit_depth'] == 16 and reply['document']['precision'] == 'rgba16', reply
            got = codes16(self.app, document)
        finally:
            close(self.app, document)
        identity = self.encoded([list(p) for p in self.pixels], 'la-identity')
        moved = sum(1 for a, b in zip(got, identity) for c in range(3) if abs(a[c] - b[c]) > 2)
        assert moved > len(got), ('the defaults differ from Exposure and Gamma (0, 1)', moved)
        return dict(moved_samples=moved)

    def la_identity_curve(self):
        out = []
        for arguments in (dict(), dict(toning_curve=[[0, 0], [255, 255]])):
            reply, document = toned(self.app, self.chart, method='local_adaptation', **arguments)
            try:
                out.append(codes16(self.app, document))
            finally:
                close(self.app, document)
        assert out[0] == out[1], 'an identity toning_curve is no curve'
        return dict(samples=len(out[0]))

    def la_sources_keep_depth(self):
        size = (16, 2)
        codes = [(x * 16, 255 - x * 16, (x * 37) % 256) for _ in range(size[1]) for x in range(size[0])]
        eight = png8(self.folder / 'la-eight.png', size, codes)
        reply, document = toned(self.app, eight, method='local_adaptation')
        try:
            assert reply['bit_depth'] == 8 and reply['document']['precision'] == 'rgba8', reply
        finally:
            close(self.app, document)
        wide = [(x * 4000 + 7, 65535 - x * 4000, (x * 9001) % 65536)
                for _ in range(size[1]) for x in range(size[0])]
        sixteen = tiff(self.folder / 'la-sixteen.tif', size, wide, 16)
        reply, document = toned(self.app, sixteen, method='local_adaptation')
        try:
            assert reply['bit_depth'] == 16 and reply['document']['precision'] == 'rgba16', reply
        finally:
            close(self.app, document)
        return dict(eight='rgba8', sixteen='rgba16')

    def la_refusals(self):
        document = open_document(self.app, self.chart)
        try:
            cases = [
                (dict(method='local_adaptation', radius=0), 'radius'),
                (dict(method='local_adaptation', strength=5), 'strength'),
                (dict(method='local_adaptation', detail=400), 'detail'),
                (dict(method='local_adaptation', toning_curve=[[0, 0]]), 'toning_curve'),
                (dict(method='exposure_gamma', radius=3), 'radius applies to method local_adaptation only'),
                (dict(method='local_adaptation', gamma=2.5), 'gamma'),
            ]
            for arguments, needle in cases:
                self.app.refused('hdr_toning', needle, document_id=document, **arguments)
            history = self.app.call('list_history', document_id=document)
            assert len(rows(self.app, document)) == 1 and history['cursor'] == 0, history
            return dict(cases=len(cases))
        finally:
            close(self.app, document)

    def la_actions(self):
        name = 'v1 hdr toning local adaptation'
        settings = dict(radius=24, strength=0.8, smooth_edges=True, gamma=1.2, exposure=0.5,
                        detail=60, shadow=20, highlight=-10, vibrance=15, saturation=10,
                        toning_curve=[[0, 0], [128, 150], [255, 255]])
        document = open_document(self.app, self.chart)
        try:
            self.app.call('start_recording', discard=True)
            self.app.call('hdr_toning', document_id=document, method='local_adaptation', **settings)
            recorded = self.app.call('stop_recording', name=name)
            steps = recorded['steps']
            assert [step['tool'] for step in steps] == ['hdr_toning'], steps
            arguments = steps[0]['arguments']
            assert arguments.get('method') == 'local_adaptation', arguments
            missing = [key for key in LOCAL_KEYS if key not in arguments]
            assert not missing, ('every Local Adaptation value is explicit', missing, arguments)
            for key, value in settings.items():
                assert arguments[key] == value, (key, arguments[key], value)
            first = codes16(self.app, document)
        finally:
            close(self.app, document)
        again = open_document(self.app, self.chart)
        try:
            self.app.call('run_action', document_id=again, name=name)
            assert len(rows(self.app, again)) == 1
            assert codes16(self.app, again) == first, 'the replay gives the same codes'
            return dict(arguments=arguments)
        finally:
            close(self.app, again)


    # ------------------------------------------------------------ presets (4c) --

    def toned_codes(self, **arguments):
        reply, document = toned(self.app, self.chart, **arguments)
        try:
            return codes16(self.app, document)
        finally:
            close(self.app, document)

    def user_preset(self, name, values, extra=None):
        """Writes a user preset file into the instance's own library; returns its path."""
        folder = self.app.folder / 'hdr-toning-presets'
        folder.mkdir(parents=True, exist_ok=True)
        stored = dict(method='local_adaptation', **values)
        stored.update(extra or {})
        path = folder / (hashlib.sha256(name.lower().encode()).hexdigest() + '.json')
        path.write_text(json.dumps(dict(schema_version=1, name=name, hdr_toning=stored)))
        return path

    def recorded_replay(self, name, arguments, before_replay=None):
        """Records one `hdr_toning` call; answers (step arguments, codes, replayed codes)."""
        document = open_document(self.app, self.chart)
        try:
            self.app.call('start_recording', discard=True)
            self.app.call('hdr_toning', document_id=document, **arguments)
            steps = self.app.call('stop_recording', name=name)['steps']
            assert [step['tool'] for step in steps] == ['hdr_toning'], steps
            first = codes16(self.app, document)
        finally:
            close(self.app, document)
        if before_replay:
            before_replay()
        again = open_document(self.app, self.chart)
        try:
            self.app.call('run_action', document_id=again, name=name)
            return steps[0]['arguments'], first, codes16(self.app, again)
        finally:
            close(self.app, again)

    def unknown_preset(self, *names):
        """Each name refuses with the unknown-preset sentence; answers the last refusal."""
        document = open_document(self.app, self.chart)
        try:
            for name in names:
                text = self.app.refused('hdr_toning', 'No HDR Toning preset named',
                                        'The built-in preset is Default',
                                        document_id=document, preset=name)
            return text
        finally:
            close(self.app, document)

    def presets(self):
        detail = {}
        assert self.toned_codes(preset='Default') == self.toned_codes(method='local_adaptation'), \
            'Default is local_adaptation at its defaults'
        assert self.toned_codes(preset='Default', detail=50) == \
            self.toned_codes(method='local_adaptation', detail=50), 'an explicit key overrides'
        detail['refusal'] = self.unknown_preset('No Such Look', 'Scott5')[:200]
        mine = dict(radius=40, strength=1.2, smooth_edges=False, gamma=0.9, exposure=0.25,
                    detail=80, shadow=-20, highlight=30, vibrance=10, saturation=-15,
                    toning_curve=[[0, 0], [100, 120], [255, 255]])
        path = self.user_preset('Drive Look', mine)
        assert self.toned_codes(preset='Drive Look') == \
            self.toned_codes(method='local_adaptation', **mine), 'a user preset applies by name'
        arguments, first, again = self.recorded_replay(
            'v1 hdr toning user preset', dict(preset='Drive Look'), before_replay=path.unlink)
        assert 'preset' not in arguments and arguments.get('method') == 'local_adaptation', arguments
        assert not [key for key in LOCAL_KEYS if key not in arguments], arguments
        for key, value in mine.items():
            assert arguments[key] == value, (key, arguments[key], value)
        assert again == first, 'a recorded user preset replays after its file is deleted'
        self.unknown_preset('Drive Look')
        arguments, first, again = self.recorded_replay('v1 hdr toning default preset',
                                                       dict(preset='Default'))
        assert 'preset' not in arguments and arguments.get('method') == 'local_adaptation', arguments
        assert again == first, 'the recorded Default replays to the same codes'
        renamed = dict(mine, detail=150, gamma=1.4)
        self.user_preset('Scott5', renamed)
        assert self.toned_codes(preset='Scott5') == \
            self.toned_codes(method='local_adaptation', **renamed), \
            "a user preset may carry a removed built-in's name"
        self.user_preset('Drive Stray', mine, extra=dict(radious=300))
        document = open_document(self.app, self.chart)
        try:
            self.app.refused('hdr_toning', 'No HDR Toning preset named', document_id=document,
                             preset='Drive Stray')
        finally:
            close(self.app, document)
        return detail


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-hdr-toning-'))
    fixtures = output / 'fixtures'
    fixtures.mkdir(parents=True, exist_ok=True)
    results, failed = {}, []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}: {json.dumps(results[name]["detail"], default=str)}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', 2 * 1024 ** 3) as app:
        drive = Drive(app, fixtures)
        run('(a) exposure and gamma', drive.exposure_gamma)
        run('(b) highlight compression', drive.highlight_compression)
        run('(c) equalize histogram', drive.equalize_histogram)
        run('(d) float to 8 bits', drive.float_to_eight)
        run('(e) 8- and 16-bit sources keep their depth', drive.sources_keep_depth)
        run('(f) flatten and one undo', drive.flatten_and_undo)
        run('(g) CMYK refusal', drive.cmyk_refusal)
        run('(h) argument refusals', drive.argument_refusals)
        run('(i) merge to hdr, then hdr toning', drive.after_merge)
        run('(j) actions record and replay', drive.actions)
        run('(k) local adaptation: the flat closed form', drive.la_closed_form)
        run('(l) local adaptation: defaults on the chart', drive.la_defaults)
        run('(m) local adaptation: an identity toning curve', drive.la_identity_curve)
        run('(n) local adaptation: 8- and 16-bit sources keep their depth', drive.la_sources_keep_depth)
        run('(o) local adaptation: refusals', drive.la_refusals)
        run('(p) local adaptation: actions record and replay', drive.la_actions)
        run('(q) presets: Default, a user preset, a removed name', drive.presets)
    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
