#!/usr/bin/env python3
"""The M3b (phases 1 and 2) MCP drive: Edit ▸ Generative Fill's twin `generative_fill`,
Edit ▸ Generative Expand's twin `generative_expand` and the Generative Layer variation tools on the
Debug app against stub OpenAI, Gemini and fal servers
this file owns. No case reaches the network or a real key.

The stub is one `ThreadingHTTPServer` on 127.0.0.1:0 with three routes, recording every request
(path, headers, the multipart fields and files or the JSON body):
- `POST /v1/images/edits` (OpenAI, multipart) answers `n` solid PNGs at the requested size in
  OpenAI's `{"data": [{"b64_json": …}]}` shape, picture i in COLORS[i] (COLORS[0] is STUB), or,
  in case E, HTTP 401 with an OpenAI-shaped error; with `short` 'openai', ONE picture whatever `n`;
- `POST /v1beta/interactions` (Gemini, JSON) answers ONE solid PNG at the 1K size of the asked
  aspect ratio, the i-th request of a case in COLORS[i], alternately as `output_image.data` and as
  a `steps[]` `model_output` image (both reply paths the provider reads); with `short` 'gemini',
  HTTP 429 `{"error": {"message": GEMINI_SHORT}}` from a case's second request on;
- `POST /fal-ai/flux-pro/v1/fill` (fal, JSON) answers `num_images` PNG data URIs in `images[].url`
  at the size of the image it was sent, picture i in COLORS[i]; with `short` 'fal', HTTP 422
  `{"detail": FAL_SHORT}` from a case's second request on.
The instance is owned here (`phase5_support.App`: a scratch MCP socket, a dummy API key, a private
folder for every preference library, DR_BACKGROUND=1) with DR_OPENAI_BASE_URL,
DR_GEMINI_BASE_URL and DR_FAL_BASE_URL pointed at the stub and DR_CLOUD_DIR at a scratch folder
this file seeds per case with `<provider>_api_key` = `dummy-<provider>-key` and `cloud.json`.
Whole pictures are read back through `save_copy` and the independent PNG reader
(`scripts/raw-probe/native.py`) at the document's own depth; the pictures the stub receives are
read by this file's own PNG reader.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the receipt.

- **A** the agent switch off (consent and key present): refused naming Settings ▸ Cloud's
  switch; the stub receives 0 requests.
- **B** no consent (switch on, key present): refused naming the terms; 0 requests.
- **C** no key (switch on, consent): "Add your OpenAI API key in Settings ▸ Cloud."; 0 requests.
- **M8 / M16** `select_rect` SELECTION, `generative_fill` prompt PROMPT on an 8-bit / 16-bit
  gradient document: ok, completed, changed, provider "openai", model MODEL, variations 3,
  selected_variation 0; exactly one POST `/v1/images/edits` with `Authorization: Bearer
  dummy-openai-key` and the fields model MODEL, prompt PROMPT, n "3", size "<w>x<h>" equal to the
  reply's native_size, background "opaque"; the image and mask PNGs at that size, the mask's
  alpha 0 exactly on the cells whose footprint in the reply's crop touches the selection and 255
  elsewhere, the image an RGBA PNG with the same alpha (the hole transparent, so GPT Image
  generates there); variations_requested 3, variations_received 3 and no short_reason; the new layer
  at index 1 (directly above the anchor), named PROMPT, with a mask; every pixel outside the
  selection bit-exact against the picture before; every pixel inside it equal to STUB (exact at
  8 bits; at 16 bits within 1 code of STUB × 257); one undo restores the picture, one layer and
  the history cursor.
- **P** an empty prompt sends the context prompt CONTEXT (one request).
- **E** the stub answers 401: refused with the stub's own message, the layer count and the
  history cursor unchanged.
- **F** a 32-bit document: refused naming 32-bit; 0 requests.
- **G** (Gemini) `generative_fill` provider gemini: ok, changed, provider "gemini", model
  GEMINI_MODEL, variations 3; exactly three POSTs to `/v1beta/interactions`, each with
  `x-goog-api-key: dummy-gemini-key`, model GEMINI_MODEL, an input of one text part (PROMPT, then
  the masking instruction) and two PNG image parts at the native size, `response_format` image /
  image/jpeg with the aspect ratio of the reply's native_size and image_size "1K"; the image
  opaque (alpha 255 on every cell); the mask image opaque, white on the hole cells and black
  elsewhere; variations_requested 3, variations_received 3, no short_reason.
- **L** (fal) `generative_fill` provider fal: ok, changed, provider "fal", variations 3; exactly
  three POSTs to `/fal-ai/flux-pro/v1/fill` (one a variation), each with `Authorization: Key
  dummy-fal-key`, `X-Fal-Store-IO: 0`, an `X-Fal-Object-Lifecycle-Preference` naming 3600
  seconds, prompt PROMPT, num_images 1, output_format "png", sync_mode true and both pictures as
  PNG data URIs at the native size, the image opaque, the mask white on the hole cells and black
  elsewhere; variations_requested 3, variations_received 3, no short_reason.
- **MS / GS / LS** (short sets) an 8-bit `generative_fill` with the stub's `short` set to openai,
  gemini and fal: ok, changed, variations 1, variations_requested 3, variations_received 1; MS
  sends 1 request and its short_reason is OPENAI_SHORT exactly; GS and LS send 2 requests and
  their short_reason contains GEMINI_SHORT / FAL_SHORT (the provider's own sentence).
- **V** (variations, OpenAI, 8 bits): after a fill `generative_variations` reports count 3,
  selected 0; `select_generative_variation` 1, then 2, changes only the covered pixels, each to
  that variation's colour (COLORS[1], COLORS[2]), with everything outside the selection
  bit-exact; one undo returns to variation 1; `save_copy` to `.rz` and `open_document` again:
  count 3 and a select to 0 lands COLORS[0]; `generate_variations` answers count 6, selected 3,
  with one more request.
- **R** missing consent for gemini, then for fal: each refused naming that provider's terms; 0
  requests.
- **C8** an 8-bit CMYK document: `generative_fill` ok and changed with variations 3 (one
  request), then `select_generative_variation` 1 changed.
- **C16** a 16-bit CMYK document: refused naming 16-bit CMYK; 0 requests.
- **X8 / X16** (Generative Expand) the 8-bit / 16-bit gradient, `canvas_size` EXPANDED centred
  (the old picture at OLD_AT), then `generative_expand` prompt PROMPT on layer 0 with no
  selection: ok, completed, changed, variations 3, output_layer 1; exactly one POST
  `/v1/images/edits` with n "3", size equal to the reply's native_size and background "opaque";
  the mask's alpha 0 exactly on the cells whose footprint in the reply's crop leaves the old
  picture's rect and 255 elsewhere, and the image an RGBA PNG with the same alpha (the border
  transparent, not opaque black); variations_requested 3, variations_received 3, no
  short_reason; every pixel of the old rect bit-exact against the picture after `canvas_size`; every
  pixel of the new area STUB (exact at 8 bits, within 1 code of STUB × 257 at 16) and opaque;
  `select_generative_variation` 1 recolours only the new area, to COLORS[1]; undoing the select
  and then the expand (one step each) restores the picture, one layer and the history cursor.
- **XG / XL** (Generative Expand through Gemini / fal) the 8-bit gradient after `canvas_size`,
  `generative_expand` provider gemini / fal: ok, changed, variations 3, exactly 3 requests; the
  first request's image opaque with RGB exactly (0, 0, 0) on every cell whose footprint misses the
  old rect; its mask white exactly on the cells not within the old rect and black elsewhere; every
  later request's image and mask bytes equal the first's.
- **XN** `generative_expand` on a picture with no canvas change: refused naming "no transparent
  area"; 0 requests.
- **XA** the agent switch off: `generative_expand` refused naming Settings ▸ Cloud's switch; 0
  requests.
- **XC8** an 8-bit CMYK document after `canvas_size`: `generative_expand` changed, variations 3,
  one request.
- **XC16** a 16-bit CMYK document after `canvas_size`: refused naming 16-bit CMYK; 0 requests.
- **XF** a 32-bit document after `canvas_size`: refused naming 32-bit; 0 requests.
- **XK8 / XK16** (the Crop tool's Fill) the 8-bit / 16-bit gradient, `crop` x -40,
  y -30, 400 × 300 (EXPANDED, the old picture at OLD_AT) with `fill` "generative_expand", prompt
  PROMPT and delete_cropped true, against a twin document cropped the same way with the default
  fill: ok, completed, changed, `extended` true, `fill` "generative_expand", variations 3,
  output_layer 1; exactly one POST `/v1/images/edits` with n "3" and size equal to the reply's
  native_size; the mask's alpha by `expand_hole`; every pixel of the old rect bit-exact against the
  twin; every pixel of the new area STUB (exact at 8 bits, within 1 code of STUB × 257 at 16) and
  opaque; the history cursor +1; one undo restores 320 × 240, the picture and one layer.
- **XKA** the agent switch off: that `crop` refused naming Settings ▸ Cloud's switch; 0 requests;
  the canvas still 320 × 240.
- **XKF** a 32-bit document: that `crop` refused naming 32-bit; 0 requests; the canvas still
  320 × 240.
- **XKR** `start_recording`, that `crop` (completed), `stop_recording`: no recorded step is a
  `crop`.

Usage: python3 scripts/verify_generative_fill.py [--receipts DIR]. Prints PASS/FAIL per gate and
exits 0 only when every gate passes.

OWNER ONLY, never run by a lane: `--live-key-file <path> [--provider openai|gemini|fal]` copies a
real key for that provider from <path> into the scratch DR_CLOUD_DIR, records consent, turns the
agent switch on, points the instance at the provider's own base URL and runs ONE live fill (case
LIVE, billed to that key; three variations), saving the result as `live.png` and the variations
reply as `live-variations.json` in the receipts folder for a person to look at. With `--expand`
the live case grows the canvas by 80 × 60 centred (`canvas_size`) and runs ONE `generative_expand`
instead of the fill. It prints `LIVE variations: <received> of <requested>` and the reply's
short_reason when one is present.
"""
import argparse
import base64
import http.server
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import sys
import tempfile
import threading
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402
from verify_phase1 import png  # noqa: E402

# ---- the expectations, fixed here BEFORE anything is measured -------------
WIDTH, HEIGHT = 320, 240
SELECTION = (100, 80, 60, 40)
PROMPT = 'a red boat'
MODEL = 'gpt-image-2.5-sunburst'
GEMINI_MODEL = 'gemini-nano-banana-2.1'
FAL_MODEL = 'fal-ai/flux-pro/v1/fill'
KEY = 'dummy-openai-key'
KEYS = {'openai': KEY, 'gemini': 'dummy-gemini-key', 'fal': 'dummy-fal-key'}
STUB = (37, 140, 201)
COLORS = (STUB, (200, 40, 30), (30, 190, 70))
GEMINI_INSTRUCTION = ('The first image is the picture. The second image is a mask of the same '
                      'size: repaint only its white area and keep every other pixel unchanged. '
                      "The first image's pixels under the white area are placeholders (black where "
                      'the canvas was extended), not part of the picture: paint new content there '
                      'that continues it. Answer one image of the whole picture.')
OPENAI_SHORT = 'OpenAI answered 1 of the 3 pictures asked for.'
GEMINI_SHORT = 'Resource has been exhausted (stub quota).'
FAL_SHORT = 'The stub refused this fal request.'
GEMINI_SIZES = {'1:1': (1024, 1024), '2:3': (832, 1248), '3:2': (1248, 832), '3:4': (864, 1184),
                '4:3': (1184, 864), '4:5': (896, 1152), '5:4': (1152, 896), '9:16': (768, 1344),
                '16:9': (1344, 768), '21:9': (1536, 672)}
BASES = {'openai': 'https://api.openai.com/v1',
         'gemini': 'https://generativelanguage.googleapis.com/v1beta', 'fal': 'https://fal.run'}
CONTEXT = ('Fill the masked area so it continues the surrounding image naturally, matching its '
           'content, lighting, perspective and grain. Add no new objects or text.')
REFUSED_401 = 'Incorrect API key provided: dummy-op***-key.'
SWITCH_OFF = 'Allow MCP agents to use cloud tools'
NO_CONSENT = "OpenAI's terms have not been accepted"
NO_CONSENT_GEMINI = "Gemini's terms have not been accepted"
NO_CONSENT_FAL = "fal's terms have not been accepted"
NO_KEY = 'Add your OpenAI API key in Settings ▸ Cloud.'
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
EXPANDED = (WIDTH + 80, HEIGHT + 60)
OLD_AT = (40, 30)
NO_TRANSPARENCY = 'no transparent area'


def _load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


png_samples = _load('generative_fill_png_reader', 'scripts/raw-probe/native.py').png_samples


def read_png8(data):
    """An 8-bit RGB or RGBA PNG (any filter, no interlace) as (width, height, RGBA bytes)."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n', 'not a PNG'
    at, chunks = 8, {}
    while at < len(data):
        size = struct.unpack_from('>I', data, at)[0]
        kind, body = data[at + 4:at + 8], data[at + 8:at + 8 + size]
        chunks.setdefault(kind, []).append(body)
        at += size + 12
    width, height, depth, color, _, _, interlace = struct.unpack('>IIBBBBB', chunks[b'IHDR'][0])
    assert depth == 8 and color in (2, 6) and interlace == 0, (depth, color, interlace)
    unit = 3 if color == 2 else 4
    stride = width * unit
    encoded = zlib.decompress(b''.join(chunks[b'IDAT']))
    previous, out = bytearray(stride), bytearray()
    for y in range(height):
        offset = y * (stride + 1)
        mode = encoded[offset]
        row = bytearray(encoded[offset + 1:offset + stride + 1])
        if mode == 1:
            for x in range(unit, stride):
                row[x] = (row[x] + row[x - unit]) & 255
        elif mode == 2:
            row = bytearray((v + a) & 255 for v, a in zip(row, previous))
        elif mode == 3:
            for x in range(stride):
                left = row[x - unit] if x >= unit else 0
                row[x] = (row[x] + (left + previous[x]) // 2) & 255
        elif mode == 4:
            for x in range(stride):
                left, above = (row[x - unit] if x >= unit else 0), previous[x]
                corner = previous[x - unit] if x >= unit else 0
                p = left + above - corner
                a, b, c = abs(p - left), abs(p - above), abs(p - corner)
                row[x] = (row[x] + (left if a <= b and a <= c else (above if b <= c else corner))) & 255
        if unit == 3:
            out.extend(b for i in range(width) for b in (*row[3 * i:3 * i + 3], 255))
        else:
            out.extend(row)
        previous = row
    return width, height, bytes(out)


def png_color(data):
    """The PNG colour type in IHDR (2 RGB, 6 RGBA), None for anything that is not a PNG."""
    if not isinstance(data, bytes) or data[:8] != b'\x89PNG\r\n\x1a\n' or data[12:16] != b'IHDR':
        return None
    return data[25]


def alpha_cells_ok(picture, native, is_hole):
    """A decoded picture at `native` whose alpha is exactly 0 on the cells `is_hole(i, j)` names
    and 255 on every other."""
    if not isinstance(picture, tuple) or picture[:2] != tuple(native):
        return False
    w, h, rgba = picture
    return all(rgba[(j * w + i) * 4 + 3] == (0 if is_hole(i, j) else 255)
               for j in range(h) for i in range(w))


def counts_ok(reply, requested=3, received=3):
    """The reply's variation counts, with a short_reason exactly when fewer came."""
    return (reply.get('variations_requested') == requested
            and reply.get('variations_received') == received
            and ('short_reason' in reply) == (received < requested))


def solid_png(width, height, rgb):
    folder = Path(tempfile.mkdtemp(prefix='stub-png-'))
    path = folder / 'solid.png'
    png(path, width, height, bytes((*rgb, 255)) * (width * height))
    return path.read_bytes()


def multipart(body, content_type):
    boundary = content_type.split('boundary=', 1)[1].strip().encode()
    fields, files = {}, {}
    for part in body.split(b'--' + boundary):
        if not part.strip() or part.strip() == b'--':
            continue
        head, _, content = part.lstrip(b'\r\n').partition(b'\r\n\r\n')
        content = content[:-2] if content.endswith(b'\r\n') else content
        disposition = next((line for line in head.decode().split('\r\n')
                            if line.lower().startswith('content-disposition')), '')
        name = disposition.split('name="', 1)[1].split('"', 1)[0]
        if 'filename="' in disposition:
            files[name] = content
        else:
            fields[name] = content.decode()
    return fields, files


def data_uri(png_bytes):
    return 'data:image/png;base64,' + base64.b64encode(png_bytes).decode()


def from_data_uri(uri):
    """The PNG bytes of a base64 data URI, None for anything else."""
    if not isinstance(uri, str) or not uri.startswith('data:image/png;base64,'):
        return None
    return base64.b64decode(uri.split(',', 1)[1])


class Stub:
    """The owned three-provider stub: `requests` lists what arrived; `mode` is 'ok' or '401'
    (OpenAI only); `short` is None, 'openai', 'gemini' or 'fal' (that provider answers a short
    set, module doc); `served` counts the pictures answered since the last `reset`."""

    def __init__(self):
        self.requests = []
        self.mode = 'ok'
        self.short = None
        self.served = 0
        stub = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                length = int(self.headers.get('Content-Length') or 0)
                body = self.rfile.read(length)
                record = dict(path=self.path, authorization=self.headers.get('Authorization'),
                              headers={k.lower(): v for k, v in self.headers.items()})
                record['fields'], record['files'], record['json'] = {}, {}, None
                try:
                    if self.path.endswith('/images/edits'):
                        record['fields'], record['files'] = multipart(body, self.headers.get('Content-Type', ''))
                    else:
                        record['json'] = json.loads(body)
                except Exception as error:  # Recorded, never repaired.
                    record['parse_error'] = repr(error)
                stub.requests.append(record)
                status, reply = 200, b'{}'
                if self.path.endswith('/images/edits'):
                    if stub.mode == '401':
                        reply = json.dumps({'error': {'message': REFUSED_401 + ' You can find your API key '
                                                      'at https://platform.openai.com/account/api-keys.',
                                                      'type': 'invalid_request_error'}}).encode()
                        status = 401
                    else:
                        w, h = (int(v) for v in record['fields'].get('size', '1024x1024').split('x'))
                        n = 1 if stub.short == 'openai' else int(record['fields'].get('n', '1'))
                        data = []
                        for _ in range(n):
                            data.append({'b64_json': base64.b64encode(solid_png(w, h, stub.colour())).decode()})
                        reply = json.dumps({'created': 0, 'data': data}).encode()
                elif self.path.endswith('/interactions') and stub.short == 'gemini' and stub.served >= 1:
                    status, reply = 429, json.dumps({'error': {'message': GEMINI_SHORT,
                                                               'status': 'RESOURCE_EXHAUSTED'}}).encode()
                elif self.path.endswith('/interactions'):
                    ratio = ((record['json'] or {}).get('response_format') or {}).get('aspect_ratio', '1:1')
                    w, h = GEMINI_SIZES.get(ratio, (1024, 1024))
                    first = stub.served % 2 == 0
                    picture = base64.b64encode(solid_png(w, h, stub.colour())).decode()
                    if first:
                        reply = json.dumps({'output_image': {'mime_type': 'image/png', 'data': picture}}).encode()
                    else:
                        reply = json.dumps({'steps': [
                            {'type': 'thought', 'content': [{'type': 'text', 'text': 'ok'}]},
                            {'type': 'model_output',
                             'content': [{'type': 'image', 'mime_type': 'image/png', 'data': picture}]}]}).encode()
                elif self.path.endswith('/fal-ai/flux-pro/v1/fill') and stub.short == 'fal' and stub.served >= 1:
                    status, reply = 422, json.dumps({'detail': FAL_SHORT}).encode()
                elif self.path.endswith('/fal-ai/flux-pro/v1/fill'):
                    sent = from_data_uri((record['json'] or {}).get('image_url'))
                    w, h = struct.unpack('>II', sent[16:24]) if sent else (1024, 1024)
                    n = int((record['json'] or {}).get('num_images', 1))
                    images = [{'url': data_uri(solid_png(w, h, stub.colour())), 'width': w, 'height': h,
                               'content_type': 'image/png'} for _ in range(n)]
                    reply = json.dumps({'images': images, 'seed': 1}).encode()
                else:
                    status, reply = 404, b'{"detail": "no such route"}'
                self.send_response(status)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(reply)))
                self.end_headers()
                self.wfile.write(reply)

            def log_message(self, *_):
                pass

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def colour(self):
        rgb = COLORS[self.served % len(COLORS)]
        self.served += 1
        return rgb

    def reset(self):
        self.served = 0

    @property
    def root(self):
        return f'http://127.0.0.1:{self.server.server_address[1]}'

    @property
    def base(self):
        return self.root + '/v1'

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.server.shutdown()
        self.server.server_close()


def seed(cloud, *, switch, consent, key, providers=('openai', 'gemini', 'fal')):
    """DR_CLOUD_DIR's files for one case: consent (when `consent`) and a key (when `key`) for each
    of `providers`; the other providers get neither."""
    cloud.mkdir(parents=True, exist_ok=True)
    consents = {name: {'terms_version': 1, 'accepted_at': '2026-10-15T00:00:00Z'}
                for name in providers} if consent else {}
    (cloud / 'cloud.json').write_text(json.dumps({'consents': consents, 'agent_cloud_tools': switch}))
    for name, value in KEYS.items():
        key_file = cloud / f'{name}_api_key'
        if key and name in providers:
            key_file.write_text(value + '\n')
        elif key_file.exists():
            key_file.unlink()


def gradient(folder, name):
    path = folder / name
    pixels = bytes(v for i in range(WIDTH * HEIGHT)
                   for v in (30 + (i % WIDTH) * 190 // WIDTH, 20 + (i // WIDTH) * 200 // HEIGHT,
                             (i % WIDTH + i // WIDTH) % 256, 255))
    png(path, WIDTH, HEIGHT, pixels)
    return path


def opened(app, folder, depth, tag):
    doc = app.call('open_document', path=str(gradient(folder, f'gradient-{tag}.png')),
                   precision='rgba8')['document']['id']
    if depth != 8:
        app.call('convert_precision', document_id=doc, mode=f'rgba{depth}', structure='preserve')
    return doc


def shot(app, doc, folder, name):
    path = folder / f'{name}.png'
    app.call('save_copy', document_id=doc, path=str(path))
    decoded = png_samples(path.read_bytes())
    return decoded['width'], decoded['samples']


def cursor(app, doc):
    return app.call('list_history', document_id=doc)['cursor']


def layers(app, doc):
    return app.call('get_document', document_id=doc).get('layers') or []


def inside(x, y):
    x0, y0, w, h = SELECTION
    return x0 <= x < x0 + w and y0 <= y < y0 + h


def expected_hole(crop, native):
    """Per native cell, whether its footprint in `crop` touches the selection."""
    cx, cy, cw, ch = crop
    nw, nh = native
    x0, y0, w, h = SELECTION

    def touches(i, extent, n, start, low, size):
        a = start + i * extent // n
        b = start + -(-(i + 1) * extent // n)
        return a < low + size and b > low

    xs = [touches(i, cw, nw, cx, x0, w) for i in range(nw)]
    ys = [touches(j, ch, nh, cy, y0, h) for j in range(nh)]
    return xs, ys


def fill_case(app, folder, depth, stub, receipts, gates):
    tag = f'M{depth}'
    doc = opened(app, folder, depth, tag)
    before = shot(app, doc, folder, f'before-{depth}')
    app.call('select_rect', document_id=doc, x=SELECTION[0], y=SELECTION[1], width=SELECTION[2],
             height=SELECTION[3])
    start = cursor(app, doc)
    count = len(stub.requests)
    stub.reset()
    reply = app.call('generative_fill', document_id=doc, layer=0, prompt=PROMPT)
    filled_cursor = cursor(app, doc)
    sent = stub.requests[count:]
    after = shot(app, doc, folder, f'after-{depth}')
    rows = layers(app, doc)
    native = reply.get('native_size') or [0, 0]
    crop = reply.get('crop') or [0, 0, 0, 0]
    request = sent[0] if len(sent) == 1 else {}
    fields, files = request.get('fields', {}), request.get('files', {})
    pictures = {}
    for name in ('image', 'mask'):
        try:
            pictures[name] = read_png8(files[name])
        except Exception as error:  # Recorded, never repaired.
            pictures[name] = repr(error)
    xs, ys = expected_hole(crop, native)
    mask_ok = alpha_cells_ok(pictures['mask'], native, lambda i, j: xs[i] and ys[j])
    # The image carries the hole as alpha 0 too, RGBA: GPT Image reads transparent image pixels
    # as the area to generate.
    image_ok = (png_color(files.get('image')) == 6
                and alpha_cells_ok(pictures['image'], native, lambda i, j: xs[i] and ys[j]))
    width, samples = after
    _, old = before
    scale = 257 if depth == 16 else 1
    outside_moved = inside_wrong = 0
    for y in range(HEIGHT):
        for x in range(width):
            i = (y * width + x) * 4
            if inside(x, y):
                if any(abs(samples[i + c] - STUB[c] * scale) > (1 if depth == 16 else 0) for c in range(3)):
                    inside_wrong += 1
            elif samples[i:i + 4] != old[i:i + 4]:
                outside_moved += 1
    app.call('undo', document_id=doc)
    undone = shot(app, doc, folder, f'undone-{depth}')
    rows_after_undo = layers(app, doc)
    receipts[tag] = dict(reply=reply, requests=len(sent), path=request.get('path'),
                         authorization_is_bearer_key=request.get('authorization') == f'Bearer {KEY}',
                         fields=fields, files={k: len(v) for k, v in files.items()},
                         picture_sizes={k: (v[:2] if isinstance(v, tuple) else v) for k, v in pictures.items()},
                         image_color=png_color(files.get('image')), image_ok=image_ok,
                         mask_ok=mask_ok, layers=[(r.get('name'), r.get('has_mask')) for r in rows],
                         outside_moved=outside_moved, inside_wrong=inside_wrong,
                         undo_layers=len(rows_after_undo), filled_cursor=filled_cursor,
                         cursor=cursor(app, doc), start=start)
    gates[tag] = (
        reply.get('ok') is True and reply.get('state') == 'completed' and reply.get('changed') is True
        and reply.get('provider') == 'openai' and reply.get('model') == MODEL
        and reply.get('variations') == 3 and reply.get('selected_variation') == 0
        and reply.get('output_layer') == 1 and len(sent) == 1
        and request.get('path') == '/v1/images/edits'
        and request.get('authorization') == f'Bearer {KEY}'
        and fields.get('model') == MODEL and fields.get('prompt') == PROMPT and fields.get('n') == '3'
        and fields.get('size') == f'{native[0]}x{native[1]}' and image_ok and mask_ok
        and fields.get('background') == 'opaque' and counts_ok(reply)
        and len(rows) == 2 and rows[1].get('name') == PROMPT and rows[1].get('has_mask') is True
        and outside_moved == 0 and inside_wrong == 0
        and filled_cursor == start + 1
        and undone[1] == before[1] and len(rows_after_undo) == 1 and cursor(app, doc) == start)
    return doc


def white_on_black_cells(picture, native, is_hole):
    """An opaque mask picture at `native`: white (255, 255, 255) on the cells `is_hole(i, j)`
    names, black elsewhere."""
    if not isinstance(picture, tuple) or picture[:2] != tuple(native):
        return False
    mw, mh, mask = picture
    for j in range(mh):
        for i in range(mw):
            at = (j * mw + i) * 4
            want = 255 if is_hole(i, j) else 0
            if mask[at + 3] != 255 or any(mask[at + c] != want for c in range(3)):
                return False
    return True


def white_on_black_ok(picture, crop, native):
    """An opaque mask picture at `native`: white (255, 255, 255) on the hole cells, black elsewhere."""
    xs, ys = expected_hole(crop, native)
    return white_on_black_cells(picture, native, lambda i, j: xs[i] and ys[j])


def opaque_ok(picture, native):
    """A decoded picture at `native` with alpha 255 on every cell."""
    return alpha_cells_ok(picture, native, lambda i, j: False)


def decoded(data):
    try:
        return read_png8(data)
    except Exception as error:  # Recorded, never repaired.
        return repr(error)


def gemini_case(app, folder, stub, receipts, gates):
    doc = opened(app, folder, 8, 'G')
    app.call('select_rect', document_id=doc, x=SELECTION[0], y=SELECTION[1], width=SELECTION[2],
             height=SELECTION[3])
    count = len(stub.requests)
    stub.reset()
    reply = app.call('generative_fill', document_id=doc, prompt=PROMPT, provider='gemini')
    sent = stub.requests[count:]
    native = reply.get('native_size') or [0, 0]
    crop = reply.get('crop') or [0, 0, 0, 0]
    ratio = next((k for k, v in GEMINI_SIZES.items() if list(v) == list(native)), None)
    checks = []
    for request in sent:
        body = request.get('json') or {}
        parts = body.get('input') or []
        texts = [p for p in parts if p.get('type') == 'text']
        images = [decoded(base64.b64decode(p.get('data', ''))) for p in parts if p.get('type') == 'image']
        form = body.get('response_format') or {}
        checks.append(dict(
            path=request.get('path') == '/v1beta/interactions',
            key=request.get('headers', {}).get('x-goog-api-key') == KEYS['gemini'],
            model=body.get('model') == GEMINI_MODEL,
            parts=len(parts) == 3 and len(texts) == 1 and len(images) == 2,
            text=bool(texts) and texts[0].get('text') == f'{PROMPT} {GEMINI_INSTRUCTION}',
            image=len(images) == 2 and opaque_ok(images[0], native),
            mask=len(images) == 2 and white_on_black_ok(images[1], crop, native),
            format=form.get('type') == 'image' and form.get('mime_type') == 'image/jpeg'
            and form.get('aspect_ratio') == ratio and form.get('image_size') == '1K'))
    receipts['G'] = dict(reply=reply, requests=len(sent), ratio=ratio, checks=checks)
    gates['G'] = (reply.get('ok') is True and reply.get('changed') is True
                  and reply.get('provider') == 'gemini' and reply.get('model') == GEMINI_MODEL
                  and reply.get('variations') == 3 and ratio is not None and len(sent) == 3
                  and counts_ok(reply) and all(all(c.values()) for c in checks))


def fal_case(app, folder, stub, receipts, gates):
    doc = opened(app, folder, 8, 'L')
    app.call('select_rect', document_id=doc, x=SELECTION[0], y=SELECTION[1], width=SELECTION[2],
             height=SELECTION[3])
    count = len(stub.requests)
    stub.reset()
    reply = app.call('generative_fill', document_id=doc, prompt=PROMPT, provider='fal')
    sent = stub.requests[count:]
    native = reply.get('native_size') or [0, 0]
    crop = reply.get('crop') or [0, 0, 0, 0]
    checks = []
    for request in sent:
        body = request.get('json') or {}
        headers = request.get('headers', {})
        image, mask = (from_data_uri(body.get(k)) for k in ('image_url', 'mask_url'))
        lifecycle = headers.get('x-fal-object-lifecycle-preference', '')
        try:
            lifecycle_ok = json.loads(lifecycle).get('expiration_duration_seconds') == 3600
        except Exception:  # Recorded as a failed gate, never repaired.
            lifecycle_ok = False
        checks.append(dict(
            path=request.get('path') == '/fal-ai/flux-pro/v1/fill',
            key=request.get('authorization') == f"Key {KEYS['fal']}",
            store=headers.get('x-fal-store-io') == '0', lifecycle=lifecycle_ok,
            prompt=body.get('prompt') == PROMPT, count=body.get('num_images') == 1,
            format=body.get('output_format') == 'png', sync=body.get('sync_mode') is True,
            image=image is not None and opaque_ok(decoded(image), native),
            mask=mask is not None and white_on_black_ok(decoded(mask), crop, native)))
    receipts['L'] = dict(reply=reply, requests=len(sent), checks=checks)
    gates['L'] = (reply.get('ok') is True and reply.get('changed') is True
                  and reply.get('provider') == 'fal' and reply.get('model') == FAL_MODEL
                  and reply.get('variations') == 3 and len(sent) == 3 and counts_ok(reply)
                  and all(all(c.values()) for c in checks))


def short_case(app, folder, provider, tag, stub, receipts, gates):
    """MS / GS / LS: `provider` answers one picture of the three asked, and the reply says so."""
    doc = opened(app, folder, 8, tag)
    app.call('select_rect', document_id=doc, x=SELECTION[0], y=SELECTION[1], width=SELECTION[2],
             height=SELECTION[3])
    count = len(stub.requests)
    stub.reset()
    stub.short = provider
    try:
        reply = app.call('generative_fill', document_id=doc, layer=0, prompt=PROMPT, provider=provider)
    finally:
        stub.short = None
    sent = stub.requests[count:]
    reason = reply.get('short_reason')
    if provider == 'openai':
        want_requests, reason_ok = 1, reason == OPENAI_SHORT
    else:
        sentence = GEMINI_SHORT if provider == 'gemini' else FAL_SHORT
        want_requests, reason_ok = 2, isinstance(reason, str) and sentence in reason
    receipts[tag] = dict(reply=reply, requests=len(sent))
    gates[tag] = (reply.get('ok') is True and reply.get('changed') is True
                  and reply.get('variations') == 1 and counts_ok(reply, 3, 1)
                  and len(sent) == want_requests and reason_ok)


def covered_colour(after, before, rgb):
    """(wrong inside, moved outside) of an 8-bit shot against `rgb` and the picture before."""
    width, samples = after
    _, old = before
    wrong = moved = 0
    for y in range(HEIGHT):
        for x in range(width):
            i = (y * width + x) * 4
            if inside(x, y):
                if any(samples[i + c] != rgb[c] for c in range(3)):
                    wrong += 1
            elif samples[i:i + 4] != old[i:i + 4]:
                moved += 1
    return wrong, moved


def variations_case(app, folder, stub, receipts, gates):
    doc = opened(app, folder, 8, 'V')
    before = shot(app, doc, folder, 'V-before')
    app.call('select_rect', document_id=doc, x=SELECTION[0], y=SELECTION[1], width=SELECTION[2],
             height=SELECTION[3])
    stub.reset()
    fill = app.call('generative_fill', document_id=doc, layer=0, prompt=PROMPT)
    layer = fill.get('output_layer')
    info = app.call('generative_variations', document_id=doc, layer=layer)
    steps = {}
    for index in (1, 2):
        reply = app.call('select_generative_variation', document_id=doc, layer=layer, index=index)
        steps[index] = dict(reply=reply, check=covered_colour(shot(app, doc, folder, f'V-{index}'),
                                                              before, COLORS[index]))
    app.call('undo', document_id=doc)
    undone = covered_colour(shot(app, doc, folder, 'V-undo'), before, COLORS[1])
    undone_info = app.call('generative_variations', document_id=doc, layer=layer)
    path = folder / 'variations.rz'
    app.call('save_copy', document_id=doc, path=str(path))
    reopened = app.call('open_document', path=str(path))['document']['id']
    reopened_info = app.call('generative_variations', document_id=reopened, layer=layer)
    picked = app.call('select_generative_variation', document_id=reopened, layer=layer, index=0)
    picked_check = covered_colour(shot(app, reopened, folder, 'V-reopened'), before, COLORS[0])
    count = len(stub.requests)
    stub.reset()
    more = app.call('generate_variations', document_id=reopened, layer=layer)
    more_info = app.call('generative_variations', document_id=reopened, layer=layer)
    receipts['V'] = dict(fill=fill, info=info, steps=steps, undone=undone, undone_info=undone_info,
                         reopened_info=reopened_info, picked=picked, picked_check=picked_check,
                         more=more, more_info=more_info, more_requests=len(stub.requests) - count)
    gates['V'] = (fill.get('changed') is True and info.get('count') == 3 and info.get('selected') == 0
                  and all(v['reply'].get('changed') is True and v['reply'].get('selected') == k
                          and v['check'] == (0, 0) for k, v in steps.items())
                  and undone == (0, 0) and undone_info.get('selected') == 1
                  and reopened_info.get('count') == 3 and reopened_info.get('selected') == 1
                  and picked.get('changed') is True and picked_check == (0, 0)
                  and more.get('changed') is True and more_info.get('count') == 6
                  and more_info.get('selected') == 3 and len(stub.requests) - count == 1)


def consent_case(app, folder, cloud, stub, receipts, gates):
    doc = opened(app, folder, 8, 'R')
    app.call('select_rect', document_id=doc, x=10, y=10, width=20, height=20)
    seed(cloud, switch=True, consent=True, key=True, providers=('openai',))
    results = {}
    for provider, expected in (('gemini', NO_CONSENT_GEMINI), ('fal', NO_CONSENT_FAL)):
        count = len(stub.requests)
        message = app.call('generative_fill', fail=True, document_id=doc, prompt=PROMPT, provider=provider)
        results[provider] = dict(message=message, requests=len(stub.requests) - count,
                                 ok=expected in message and len(stub.requests) == count)
    seed(cloud, switch=True, consent=True, key=True)
    receipts['R'] = results
    gates['R'] = all(r['ok'] for r in results.values())


def cmyk_cases(app, folder, stub, receipts, gates):
    cmyk = opened(app, folder, 8, 'cmyk8')
    converted = app.call('convert_mode', document_id=cmyk, mode='cmyk', profile='file', path=str(GENERIC))
    app.call('select_rect', document_id=cmyk, x=SELECTION[0], y=SELECTION[1], width=SELECTION[2],
             height=SELECTION[3])
    count = len(stub.requests)
    stub.reset()
    fill = app.call('generative_fill', document_id=cmyk, prompt=PROMPT)
    picked = app.call('select_generative_variation', document_id=cmyk, layer=fill.get('output_layer'),
                      index=1)
    receipts['C8'] = dict(converted=converted, fill=fill, picked=picked,
                          requests=len(stub.requests) - count)
    gates['C8'] = (converted.get('depth') == 8 and fill.get('changed') is True
                   and fill.get('variations') == 3 and picked.get('changed') is True
                   and picked.get('selected') == 1 and len(stub.requests) - count == 1)
    deep = opened(app, folder, 16, 'cmyk16')
    app.call('convert_mode', document_id=deep, mode='cmyk', profile='file', path=str(GENERIC))
    app.call('select_rect', document_id=deep, x=10, y=10, width=20, height=20)
    count = len(stub.requests)
    message = app.call('generative_fill', fail=True, document_id=deep, prompt=PROMPT)
    receipts['C16'] = dict(message=message, requests=len(stub.requests) - count)
    gates['C16'] = '16-bit CMYK' in message and len(stub.requests) == count


def inside_old(x, y):
    return OLD_AT[0] <= x < OLD_AT[0] + WIDTH and OLD_AT[1] <= y < OLD_AT[1] + HEIGHT


def expand_hole(crop, native):
    """Per native cell, whether its footprint in `crop` leaves the old picture's rect (the same
    integer footprint rule as `expected_hole`)."""
    cx, cy, cw, ch = crop
    nw, nh = native

    def within(i, extent, n, start, low, size):
        a = start + i * extent // n
        b = start + -(-(i + 1) * extent // n)
        return a >= low and b <= low + size

    xs = [within(i, cw, nw, cx, OLD_AT[0], WIDTH) for i in range(nw)]
    ys = [within(j, ch, nh, cy, OLD_AT[1], HEIGHT) for j in range(nh)]
    return xs, ys


def grown(app, doc):
    return app.call('canvas_size', document_id=doc, width=EXPANDED[0], height=EXPANDED[1],
                    anchor='center')


def expanded_check(after, before, rgb, depth):
    """(wrong in the new area, moved in the old rect) of a shot against `rgb` and `before`."""
    width, samples = after
    _, old = before
    scale = 257 if depth == 16 else 1
    slack = 1 if depth == 16 else 0
    wrong = moved = 0
    for y in range(EXPANDED[1]):
        for x in range(width):
            i = (y * width + x) * 4
            if inside_old(x, y):
                if samples[i:i + 4] != old[i:i + 4]:
                    moved += 1
            elif (any(abs(samples[i + c] - rgb[c] * scale) > slack for c in range(3))
                  or samples[i + 3] != 255 * scale):
                wrong += 1
    return wrong, moved


def expand_case(app, folder, depth, stub, receipts, gates):
    tag = f'X{depth}'
    doc = opened(app, folder, depth, tag)
    canvas = grown(app, doc)
    before = shot(app, doc, folder, f'X-before-{depth}')
    start = cursor(app, doc)
    count = len(stub.requests)
    stub.reset()
    reply = app.call('generative_expand', document_id=doc, layer=0, prompt=PROMPT)
    expanded_cursor = cursor(app, doc)
    sent = stub.requests[count:]
    native = reply.get('native_size') or [0, 0]
    crop = reply.get('crop') or [0, 0, 0, 0]
    request = sent[0] if len(sent) == 1 else {}
    fields, files = request.get('fields', {}), request.get('files', {})
    mask = decoded(files['mask']) if 'mask' in files else 'no mask'
    image = decoded(files['image']) if 'image' in files else 'no image'
    xs, ys = expand_hole(crop, native)
    mask_ok = alpha_cells_ok(mask, native, lambda i, j: not (xs[i] and ys[j]))
    # The border reaches GPT Image transparent, not opaque black (the 2026-10-19 live run).
    image_ok = (png_color(files.get('image')) == 6
                and alpha_cells_ok(image, native, lambda i, j: not (xs[i] and ys[j])))
    filled = expanded_check(shot(app, doc, folder, f'X-after-{depth}'), before, STUB, depth)
    layer = reply.get('output_layer')
    picked = app.call('select_generative_variation', document_id=doc, layer=layer, index=1)
    picked_check = expanded_check(shot(app, doc, folder, f'X-picked-{depth}'), before, COLORS[1], depth)
    app.call('undo', document_id=doc)
    app.call('undo', document_id=doc)
    undone = shot(app, doc, folder, f'X-undone-{depth}')
    rows = layers(app, doc)
    receipts[tag] = dict(canvas=canvas, reply=reply, requests=len(sent), path=request.get('path'),
                         fields=fields, mask_size=(mask[:2] if isinstance(mask, tuple) else mask),
                         image_color=png_color(files.get('image')), image_ok=image_ok,
                         mask_ok=mask_ok, filled=filled, picked=picked, picked_check=picked_check,
                         undo_layers=len(rows), expanded_cursor=expanded_cursor,
                         cursor=cursor(app, doc), start=start)
    gates[tag] = (
        reply.get('ok') is True and reply.get('state') == 'completed' and reply.get('changed') is True
        and reply.get('variations') == 3 and reply.get('output_layer') == 1 and len(sent) == 1
        and request.get('path') == '/v1/images/edits' and fields.get('n') == '3'
        and fields.get('size') == f'{native[0]}x{native[1]}' and mask_ok and filled == (0, 0)
        and image_ok and fields.get('background') == 'opaque' and counts_ok(reply)
        and picked.get('changed') is True and picked_check == (0, 0)
        and expanded_cursor == start + 1 and undone[1] == before[1] and len(rows) == 1
        and cursor(app, doc) == start)


def meets_old(crop, native):
    """Per native cell, whether its footprint in `crop` touches the old picture's rect (the same
    integer footprint rule as `expected_hole`)."""
    cx, cy, cw, ch = crop
    nw, nh = native

    def touches(i, extent, n, start, low, size):
        a = start + i * extent // n
        b = start + -(-(i + 1) * extent // n)
        return a < low + size and b > low

    xs = [touches(i, cw, nw, cx, OLD_AT[0], WIDTH) for i in range(nw)]
    ys = [touches(j, ch, nh, cy, OLD_AT[1], HEIGHT) for j in range(nh)]
    return xs, ys


def sent_pictures(provider, request):
    """The (image, mask) PNG bytes a Gemini or fal request carried, None for one missing."""
    body = request.get('json') or {}
    if provider == 'gemini':
        parts = [p for p in body.get('input') or [] if p.get('type') == 'image']
        datas = [base64.b64decode(p.get('data', '')) for p in parts]
        return tuple((datas + [None, None])[:2])
    return tuple(from_data_uri(body.get(k)) for k in ('image_url', 'mask_url'))


def expand_provider_case(app, folder, provider, tag, stub, receipts, gates):
    """XG / XL: Generative Expand through `provider`, the border sent opaque black under a white
    mask, the same pictures on every request."""
    doc = opened(app, folder, 8, tag)
    grown(app, doc)
    count = len(stub.requests)
    stub.reset()
    reply = app.call('generative_expand', document_id=doc, layer=0, prompt=PROMPT, provider=provider)
    sent = stub.requests[count:]
    native = reply.get('native_size') or [0, 0]
    crop = reply.get('crop') or [0, 0, 0, 0]
    pictures = [sent_pictures(provider, r) for r in sent]
    first = pictures[0] if pictures else (None, None)
    image = decoded(first[0]) if first[0] else 'no image'
    mask = decoded(first[1]) if first[1] else 'no mask'
    image_ok = opaque_ok(image, native)
    if image_ok:
        mx, my = meets_old(crop, native)
        w, h, rgba = image
        image_ok = all(rgba[(j * w + i) * 4:(j * w + i) * 4 + 3] == b'\x00\x00\x00'
                       for j in range(h) for i in range(w) if not (mx[i] and my[j]))
    xs, ys = expand_hole(crop, native)
    mask_ok = white_on_black_cells(mask, native, lambda i, j: not (xs[i] and ys[j]))
    same = all(p == first for p in pictures[1:])
    receipts[tag] = dict(reply=reply, requests=len(sent),
                         image_size=(image[:2] if isinstance(image, tuple) else image),
                         image_ok=image_ok, mask_ok=mask_ok, same=same)
    gates[tag] = (reply.get('ok') is True and reply.get('changed') is True
                  and reply.get('variations') == 3 and len(sent) == 3 and image_ok and mask_ok
                  and same and counts_ok(reply))


def expand_refusals(app, folder, cloud, stub, receipts, gates):
    flat = opened(app, folder, 8, 'XN')
    count = len(stub.requests)
    message = app.call('generative_expand', fail=True, document_id=flat, prompt=PROMPT)
    receipts['XN'] = dict(message=message, requests=len(stub.requests) - count)
    gates['XN'] = NO_TRANSPARENCY in message and len(stub.requests) == count

    doc = opened(app, folder, 8, 'XA')
    grown(app, doc)
    seed(cloud, switch=False, consent=True, key=True)
    count = len(stub.requests)
    message = app.call('generative_expand', fail=True, document_id=doc, prompt=PROMPT)
    seed(cloud, switch=True, consent=True, key=True)
    receipts['XA'] = dict(message=message, requests=len(stub.requests) - count)
    gates['XA'] = SWITCH_OFF in message and len(stub.requests) == count

    cmyk = opened(app, folder, 8, 'Xcmyk8')
    converted = app.call('convert_mode', document_id=cmyk, mode='cmyk', profile='file', path=str(GENERIC))
    grown(app, cmyk)
    count = len(stub.requests)
    stub.reset()
    reply = app.call('generative_expand', document_id=cmyk, prompt=PROMPT)
    receipts['XC8'] = dict(converted=converted, reply=reply, requests=len(stub.requests) - count)
    gates['XC8'] = (converted.get('depth') == 8 and reply.get('changed') is True
                    and reply.get('variations') == 3 and len(stub.requests) - count == 1)

    deep = opened(app, folder, 16, 'Xcmyk16')
    app.call('convert_mode', document_id=deep, mode='cmyk', profile='file', path=str(GENERIC))
    grown(app, deep)
    count = len(stub.requests)
    message = app.call('generative_expand', fail=True, document_id=deep, prompt=PROMPT)
    receipts['XC16'] = dict(message=message, requests=len(stub.requests) - count)
    gates['XC16'] = '16-bit CMYK' in message and len(stub.requests) == count

    float_doc = opened(app, folder, 32, 'Xfloat')
    grown(app, float_doc)
    count = len(stub.requests)
    message = app.call('generative_expand', fail=True, document_id=float_doc, prompt=PROMPT)
    receipts['XF'] = dict(message=message, requests=len(stub.requests) - count)
    gates['XF'] = '32-bit' in message and len(stub.requests) == count


def crop_expand(app, doc, fail=False):
    """The XK crop: the old picture at OLD_AT on an EXPANDED canvas, filled by Generative Expand."""
    return app.call('crop', fail=fail, document_id=doc, x=-OLD_AT[0], y=-OLD_AT[1],
                    width=EXPANDED[0], height=EXPANDED[1], delete_cropped=True,
                    fill='generative_expand', prompt=PROMPT)


def size_of(app, doc):
    summary = app.call('get_document', document_id=doc)
    return summary.get('width'), summary.get('height')


def crop_expand_case(app, folder, depth, stub, receipts, gates):
    tag = f'XK{depth}'
    twin = opened(app, folder, depth, f'{tag}-twin')
    app.call('crop', document_id=twin, x=-OLD_AT[0], y=-OLD_AT[1], width=EXPANDED[0],
             height=EXPANDED[1], delete_cropped=True)
    before = shot(app, twin, folder, f'XK-twin-{depth}')
    doc = opened(app, folder, depth, tag)
    original = shot(app, doc, folder, f'XK-original-{depth}')
    start = cursor(app, doc)
    count = len(stub.requests)
    stub.reset()
    reply = crop_expand(app, doc)
    cropped_cursor = cursor(app, doc)
    sent = stub.requests[count:]
    native = reply.get('native_size') or [0, 0]
    crop = reply.get('crop') or [0, 0, 0, 0]
    request = sent[0] if len(sent) == 1 else {}
    fields, files = request.get('fields', {}), request.get('files', {})
    mask = decoded(files['mask']) if 'mask' in files else 'no mask'
    mask_ok = False
    if isinstance(mask, tuple) and mask[:2] == tuple(native):
        mw, mh, alpha = mask
        xs, ys = expand_hole(crop, native)
        mask_ok = all(alpha[(j * mw + i) * 4 + 3] == (255 if xs[i] and ys[j] else 0)
                      for j in range(mh) for i in range(mw))
    filled = expanded_check(shot(app, doc, folder, f'XK-after-{depth}'), before, STUB, depth)
    app.call('undo', document_id=doc)
    undone = shot(app, doc, folder, f'XK-undone-{depth}')
    rows = layers(app, doc)
    size = size_of(app, doc)
    receipts[tag] = dict(reply=reply, requests=len(sent), path=request.get('path'), fields=fields,
                         mask_size=(mask[:2] if isinstance(mask, tuple) else mask), mask_ok=mask_ok,
                         filled=filled, cropped_cursor=cropped_cursor, start=start,
                         counts=[reply.get(k) for k in ('variations_requested', 'variations_received')],
                         undo_size=size, undo_layers=len(rows), cursor=cursor(app, doc))
    gates[tag] = (
        reply.get('ok') is True and reply.get('state') == 'completed' and reply.get('changed') is True
        and reply.get('extended') is True and reply.get('fill') == 'generative_expand'
        and reply.get('variations') == 3 and reply.get('output_layer') == 1 and len(sent) == 1
        and request.get('path') == '/v1/images/edits' and fields.get('n') == '3'
        and fields.get('size') == f'{native[0]}x{native[1]}' and mask_ok and filled == (0, 0)
        and cropped_cursor == start + 1 and size == (WIDTH, HEIGHT) and undone == original
        and len(rows) == 1 and cursor(app, doc) == start)


def crop_expand_refusals(app, folder, cloud, stub, receipts, gates):
    doc = opened(app, folder, 8, 'XKA')
    seed(cloud, switch=False, consent=True, key=True)
    count = len(stub.requests)
    message = crop_expand(app, doc, fail=True)
    seed(cloud, switch=True, consent=True, key=True)
    size = size_of(app, doc)
    receipts['XKA'] = dict(message=message, requests=len(stub.requests) - count, size=size)
    gates['XKA'] = SWITCH_OFF in message and len(stub.requests) == count and size == (WIDTH, HEIGHT)

    float_doc = opened(app, folder, 32, 'XKF')
    count = len(stub.requests)
    message = crop_expand(app, float_doc, fail=True)
    size = size_of(app, float_doc)
    receipts['XKF'] = dict(message=message, requests=len(stub.requests) - count, size=size)
    gates['XKF'] = '32-bit' in message and len(stub.requests) == count and size == (WIDTH, HEIGHT)

    doc = opened(app, folder, 8, 'XKR')
    app.call('start_recording', discard=True)
    stub.reset()
    reply = crop_expand(app, doc)
    stopped = app.call('stop_recording')
    tools = [step.get('tool') for step in stopped.get('steps') or []]
    receipts['XKR'] = dict(reply_state=reply.get('state'), tools=tools)
    gates['XKR'] = reply.get('state') == 'completed' and 'crop' not in tools


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--live-key-file', default=None,
                        help='OWNER ONLY: a real key file for --provider; runs one live, billed fill')
    parser.add_argument('--provider', default='openai', choices=('openai', 'gemini', 'fal'),
                        help='OWNER ONLY: the live run\'s provider (default openai)')
    parser.add_argument('--expand', action='store_true',
                        help='OWNER ONLY: the live run grows the canvas and runs generative_expand')
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='generative-fill-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    cloud = folder / 'cloud'
    receipts = {'expectations': 'fixed in scripts/verify_generative_fill.py before any measurement'}
    gates = {}
    saved = {key: os.environ.get(key) for key in ('DR_CLOUD_DIR', 'DR_OPENAI_BASE_URL',
                                                  'DR_GEMINI_BASE_URL', 'DR_FAL_BASE_URL',
                                                  'DR_BACKGROUND')}
    try:
        os.environ['DR_BACKGROUND'] = '1'
        os.environ['DR_CLOUD_DIR'] = str(cloud)
        if args.live_key_file:
            return live(args, folder, cloud, output)
        with Stub() as stub:
            os.environ['DR_OPENAI_BASE_URL'] = stub.base
            os.environ['DR_GEMINI_BASE_URL'] = stub.root + '/v1beta'
            os.environ['DR_FAL_BASE_URL'] = stub.root
            with App(folder / 'app', folder / 'recovery', interval=600) as app:
                doc = opened(app, folder, 8, 'gate')
                app.call('select_rect', document_id=doc, x=SELECTION[0], y=SELECTION[1],
                         width=SELECTION[2], height=SELECTION[3])
                for tag, state, expected in (
                        ('A', dict(switch=False, consent=True, key=True), SWITCH_OFF),
                        ('B', dict(switch=True, consent=False, key=True), NO_CONSENT),
                        ('C', dict(switch=True, consent=True, key=False), NO_KEY)):
                    seed(cloud, **state)
                    count = len(stub.requests)
                    message = app.call('generative_fill', fail=True, document_id=doc, prompt=PROMPT)
                    receipts[tag] = dict(message=message, requests=len(stub.requests) - count)
                    gates[tag] = expected in message and len(stub.requests) == count
                seed(cloud, switch=True, consent=True, key=True)
                fill_case(app, folder, 8, stub, receipts, gates)
                fill_case(app, folder, 16, stub, receipts, gates)

                count = len(stub.requests)
                reply = app.call('generative_fill', document_id=doc, prompt='')
                sent = stub.requests[count:]
                receipts['P'] = dict(reply=reply, prompts=[r['fields'].get('prompt') for r in sent])
                gates['P'] = (len(sent) == 1 and sent[0]['fields'].get('prompt') == CONTEXT
                              and reply.get('prompt') == CONTEXT and reply.get('changed') is True)
                app.call('undo', document_id=doc)

                stub.mode = '401'
                start, before_layers = cursor(app, doc), len(layers(app, doc))
                count = len(stub.requests)
                message = app.call('generative_fill', fail=True, document_id=doc, prompt=PROMPT)
                receipts['E'] = dict(message=message, requests=len(stub.requests) - count,
                                     layers=len(layers(app, doc)), cursor=cursor(app, doc), start=start)
                gates['E'] = (REFUSED_401 in message and len(stub.requests) == count + 1
                              and len(layers(app, doc)) == before_layers and cursor(app, doc) == start)
                stub.mode = 'ok'

                float_doc = opened(app, folder, 32, 'float')
                app.call('select_rect', document_id=float_doc, x=10, y=10, width=20, height=20)
                count = len(stub.requests)
                message = app.call('generative_fill', fail=True, document_id=float_doc, prompt=PROMPT)
                receipts['F'] = dict(message=message, requests=len(stub.requests) - count)
                gates['F'] = '32-bit' in message and len(stub.requests) == count

                gemini_case(app, folder, stub, receipts, gates)
                fal_case(app, folder, stub, receipts, gates)
                short_case(app, folder, 'openai', 'MS', stub, receipts, gates)
                short_case(app, folder, 'gemini', 'GS', stub, receipts, gates)
                short_case(app, folder, 'fal', 'LS', stub, receipts, gates)
                variations_case(app, folder, stub, receipts, gates)
                consent_case(app, folder, cloud, stub, receipts, gates)
                cmyk_cases(app, folder, stub, receipts, gates)
                expand_case(app, folder, 8, stub, receipts, gates)
                expand_case(app, folder, 16, stub, receipts, gates)
                expand_provider_case(app, folder, 'gemini', 'XG', stub, receipts, gates)
                expand_provider_case(app, folder, 'fal', 'XL', stub, receipts, gates)
                expand_refusals(app, folder, cloud, stub, receipts, gates)
                crop_expand_case(app, folder, 8, stub, receipts, gates)
                crop_expand_case(app, folder, 16, stub, receipts, gates)
                crop_expand_refusals(app, folder, cloud, stub, receipts, gates)
            receipts['stub_requests'] = [
                dict(path=r['path'], fields=r['fields'], files={k: len(v) for k, v in r['files'].items()},
                     json_keys=sorted((r.get('json') or {}).keys()))
                for r in stub.requests]
    finally:
        for key, value in saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    receipts['gates'] = gates
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str))
    for name, ok in gates.items():
        print(f'{"PASS" if ok else "FAIL"} {name}')
    print(f'receipt: {output / "receipt.json"}')
    return 0 if gates and all(gates.values()) else 1


def live(args, folder, cloud, output):
    """OWNER ONLY (module doc): one fill (or, with --expand, one expand) against --provider with
    the owner's own key."""
    provider = args.provider
    source = Path(args.live_key_file).expanduser()
    cloud.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, cloud / f'{provider}_api_key')
    os.chmod(cloud / f'{provider}_api_key', 0o600)
    (cloud / 'cloud.json').write_text(json.dumps({
        'consents': {provider: {'terms_version': 1, 'accepted_at': '2026-10-15T00:00:00Z'}},
        'agent_cloud_tools': True}))
    environment = {'openai': 'DR_OPENAI_BASE_URL', 'gemini': 'DR_GEMINI_BASE_URL', 'fal': 'DR_FAL_BASE_URL'}
    os.environ[environment[provider]] = BASES[provider]
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        doc = opened(app, folder, 8, 'live')
        if args.expand:
            grown(app, doc)
            tool = 'generative_expand'
        else:
            app.call('select_rect', document_id=doc, x=SELECTION[0], y=SELECTION[1],
                     width=SELECTION[2], height=SELECTION[3])
            tool = 'generative_fill'
        result = app.client.rpc('tools/call', {'name': tool,
                                        'arguments': {'document_id': doc, 'prompt': PROMPT,
                                                      'provider': provider}})
        (output / 'live-reply.json').write_text(json.dumps(result, indent=2))
        if not result.get('isError'):
            app.call('save_copy', document_id=doc, path=str(output / 'live.png'))
            variations = app.call('generative_variations', document_id=doc, layer=1)
            (output / 'live-variations.json').write_text(json.dumps(variations, indent=2))
    if result.get('isError'):
        text = ' '.join(part.get('text', '') for part in result.get('content') or [])
        print(f'LIVE FAIL: {text or "no message"} (reply: {output / "live-reply.json"})')
        return 1
    text = next((c.get('text') for c in result.get('content') or [] if c.get('type') == 'text'), '')
    try:
        body = json.loads(text)
    except (TypeError, json.JSONDecodeError):
        body = {}
    counted = f'LIVE variations: {body.get("variations_received")} of {body.get("variations_requested")}'
    if body.get('short_reason'):
        counted += f'; short_reason: {body["short_reason"]}'
    print(counted)
    print(f'LIVE reply: {output / "live-reply.json"}; picture: {output / "live.png"}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
