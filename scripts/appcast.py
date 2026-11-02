#!/usr/bin/env python3
"""The one-item Sparkle appcast the publish workflow uploads beside `Doctored.dmg`.

designs/sparkle-updates.md: every published release carries `appcast.xml` on its
GitHub Release, and the feed every installed copy reads is the LATEST release's
(`releases/latest/download/appcast.xml`), so the file holds one item, this
release. The workflow's "Sign the update and write the appcast" step runs this
after `stapler staple`, so the bytes signed are the bytes served:

    python3 scripts/appcast.py write --app APP --dmg DMG \\
        --sign-update PATH --key-file PATH --url URL --notes-url URL --out PATH
    SPARKLE_ED_PRIVATE_KEY=… python3 scripts/appcast.py check-key [--project PATH]
    python3 scripts/appcast.py --selftest

`write` reads `CFBundleShortVersionString`, `CFBundleVersion`,
`LSMinimumSystemVersion` and `SUPublicEDKey` from `APP/Contents/Info.plist` and
refuses, before anything is signed, a `CFBundleVersion` that is not a positive
integer (it is what installed copies compare), a `--url` that is not `https://`
or lacks `/v<short>/`, and a key file whose Ed25519 public half is not the app's
`SUPublicEDKey`: a secret and a committed public key from different keys would
make every installed copy reject every update, and only a manual download would
recover. The key file's content has ASCII whitespace stripped and must then be
what `generate_keys -x` exports: the base64 of the 32-byte seed, or for a key
made in Sparkle's older format the base64 of 96 bytes (orlp ed25519's 64-byte
expanded private key, then the public key), accepted only when its clamped
scalar makes its own public half. The public half is derived here in pure Python
(RFC 8032 section 5.1.5, the section 6 reference code). The key is read into
memory only, never printed and never passed in argv; a refusal names only the
two public keys.

`check-key` (publish.yml, straight after checkout) makes the same derivation from
the SPARKLE_ED_PRIVATE_KEY environment variable and compares it with
Distribution's DR_SPARKLE_PUBLIC_KEY in project.yml (read with
`updater_check.settings_entries`), so a wrong secret fails before the archive and
notarization rather than after; it prints only public keys.

It then runs Sparkle's `sign_update --ed-key-file KEY DMG`, whose one line is
`sparkle:edSignature="…" length="N"`, refuses any other output and any N that is
not the DMG's size, and writes RSS 2.0 with the `sparkle` namespace: one
`<item>` with a title "Doctored <short>", an RFC 2822 `pubDate`,
`sparkle:version`, `sparkle:shortVersionString`, `sparkle:minimumSystemVersion`,
`sparkle:fullReleaseNotesLink` (the release page; no inline notes) and the
signed `<enclosure>`. `sign_update` without `--ed-key-file` would read the
Keychain, so `write` always passes it and the selftest never runs the real tool.
The output is parsed back, written to `OUT.partial` and moved into place; any
error exits 1 with no file.

`--selftest` (`make release-check`, and the gate's `release` step): standard
library only, a stand-in `sign_update`, no network and no key.

* S1 — `write` with the stand-in printing a canned line, a scratch DMG of that
  line's length and a scratch Info.plist (0.4.3 / 3 / 15.0, `SUPublicEDKey`
  RFC 8032 test 1's public key, the key file test 1's seed): every field reads
  back through `xml.etree`, the stand-in ran as `--ed-key-file KEY DMG`, and
  neither SPARKLE_ED_PRIVATE_KEY nor ANTHROPIC_API_KEY, both set for `write`,
  reached it;
* S2 — eight malformed `sign_update` outputs are refused and the well-formed
  line is accepted (`parse_signature`);
* S3 — a length one over the DMG's size is refused by the parser, and one under
  makes `write` exit 1 with no file;
* S4 — build "0", an `http://` URL, a URL for another tag and an empty
  `SUPublicEDKey` are each refused, with no file;
* S5 — test 1's seed derives test 1's public key, also with a trailing "\\n" and
  "\\r\\n", and so does test 1 in the older 96-byte format; an older-format key
  whose halves disagree, 96 arbitrary bytes, an empty or whitespace-only file,
  non-base64 text and 31 or 33 bytes are refused, none printing the content;
* S6 — a key whose public half is not `SUPublicEDKey` is refused, with no file;
* S7 — `check-key` against a synthetic project.yml passes test 1's seed and
  refuses another key (naming only the two public keys), an unset and an empty
  secret.

The last line is `appcast selftest: <passed>/7`; the exit is 0 only at 7/7.
"""

import argparse
import base64
import binascii
import email.utils
import hashlib
import os
import plistlib
import re
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ElementTree
from xml.sax.saxutils import escape, quoteattr

SPARKLE_NS = "http://www.andymatuschak.org/xml-namespaces/sparkle"
SIGNATURE = re.compile(r'sparkle:edSignature="([A-Za-z0-9+/=]+)"\s+length="([0-9]+)"')
SIGNATURE_LOOSE = re.compile(r"sparkle:edSignature=")
BUNDLE_KEYS = ("CFBundleShortVersionString", "CFBundleVersion", "LSMinimumSystemVersion")
POSITIVE_INTEGER = re.compile(r"[1-9][0-9]*")
# What the workflow writes the secret as may end in a newline; nothing else is stripped.
ASCII_WHITESPACE = b" \t\n\r\x0b\x0c"
SEED_BYTES = 32
# A key generated before Sparkle 2's seed format is stored, and exported by `generate_keys -x`,
# as 96 bytes: orlp ed25519's 64-byte private key (SHA-512 of the seed, its low half already
# clamped into the scalar) followed by the 32-byte public key. sign_update reads it as it is
# (common_cli/Secret.swift), and the seed cannot be recovered, so it is accepted when its
# scalar makes its own public half; nobody has checked which format the reused key is stored in.
OLDER_FORMAT_BYTES = 96
OLDER_FORMAT_MISMATCH = (
    "the key file holds 96 bytes, Sparkle's older private-key format (a 64-byte expanded "
    "private key followed by its public key), and its private half does not make its public half")
PROJECT_YML = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                           "project.yml")
TIMEOUT = 300
SELFTEST_TOTAL = 7


# MARK: - Ed25519 public key (RFC 8032 section 5.1.5, after the section 6 reference code)

# The field prime 2^255 - 19, the curve constant d = -121665/121666 and sqrt(-1) mod p.
FIELD = 2 ** 255 - 19
CURVE_D = -121665 * pow(121666, FIELD - 2, FIELD) % FIELD
SQRT_MINUS_ONE = pow(2, (FIELD - 1) // 4, FIELD)


def _recover_x(y: int, sign: int) -> int:
    """The x coordinate of the curve point with this y and x's low bit `sign`."""
    x2 = (y * y - 1) * pow(CURVE_D * y * y + 1, FIELD - 2, FIELD)
    x = pow(x2, (FIELD + 3) // 8, FIELD)
    if (x * x - x2) % FIELD != 0:
        x = x * SQRT_MINUS_ONE % FIELD
    if (x * x - x2) % FIELD != 0:
        raise ValueError("no square root")
    if (x & 1) != sign:
        x = FIELD - x
    return x


# The base point B: y = 4/5, x positive (even), in extended coordinates (X, Y, Z, T), T = XY/Z.
_BASE_Y = 4 * pow(5, FIELD - 2, FIELD) % FIELD
_BASE_X = _recover_x(_BASE_Y, 0)
BASE = (_BASE_X, _BASE_Y, 1, _BASE_X * _BASE_Y % FIELD)


def _point_add(p, q):
    """Twisted Edwards addition in extended coordinates (RFC 8032 section 5.1.4)."""
    a = (p[1] - p[0]) * (q[1] - q[0]) % FIELD
    b = (p[1] + p[0]) * (q[1] + q[0]) % FIELD
    c = 2 * p[3] * q[3] * CURVE_D % FIELD
    d = 2 * p[2] * q[2] % FIELD
    e, f, g, h = b - a, d - c, d + c, b + a
    return (e * f % FIELD, g * h % FIELD, f * g % FIELD, e * h % FIELD)


def _point_multiply(scalar: int, point):
    """`scalar` times `point`, by double-and-add. Not constant-time: it runs once per publish,
    in the workflow's own job, and nothing outside that job can time it."""
    result = (0, 1, 1, 0)
    while scalar > 0:
        if scalar & 1:
            result = _point_add(result, point)
        point = _point_add(point, point)
        scalar >>= 1
    return result


def _point_compress(point) -> bytes:
    """The 32-byte encoding: y little-endian with x's low bit in the top bit."""
    z_inverse = pow(point[2], FIELD - 2, FIELD)
    x = point[0] * z_inverse % FIELD
    y = point[1] * z_inverse % FIELD
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def expanded_private_key(seed: bytes) -> bytes:
    """orlp ed25519's 64-byte private key of a 32-byte seed: SHA-512, the low half clamped."""
    if len(seed) != SEED_BYTES:
        raise ValueError(f"an Ed25519 seed is {SEED_BYTES} bytes, not {len(seed)}")
    digest = bytearray(hashlib.sha512(seed).digest())
    digest[0] &= 248
    digest[31] &= 63
    digest[31] |= 64
    return bytes(digest)


def public_key_from_scalar(scalar_bytes: bytes) -> bytes:
    """The public key of a 32-byte little-endian (clamped) scalar: the scalar times B."""
    return _point_compress(_point_multiply(int.from_bytes(scalar_bytes, "little"), BASE))


def public_key_from_seed(seed: bytes) -> bytes:
    """The Ed25519 public key of a 32-byte seed: SHA-512, clamp the low half, times B."""
    return public_key_from_scalar(expanded_private_key(seed)[:32])


def key_file_public_key(content: bytes) -> str:
    """The base64 public half of a `generate_keys -x` key file's content: the base64 of a
    32-byte seed, or of the 96-byte older format whose private half must make its public half.
    ValueError naming only lengths and formats, never the key, otherwise."""
    text = content.strip(ASCII_WHITESPACE)
    if not text:
        raise ValueError("the key file is empty")
    try:
        raw = base64.b64decode(text, validate=True)
    except (binascii.Error, ValueError):
        raise ValueError("the key file is not base64 (ASCII whitespace stripped)") from None
    if len(raw) == SEED_BYTES:
        public = public_key_from_seed(raw)
    elif len(raw) == OLDER_FORMAT_BYTES:
        public = raw[64:]
        if public_key_from_scalar(raw[:32]) != public:
            raise ValueError(OLDER_FORMAT_MISMATCH)
    else:
        raise ValueError(f"the key file decodes to {len(raw)} bytes, not the {SEED_BYTES}-byte "
                         f"seed or the {OLDER_FORMAT_BYTES}-byte older format generate_keys -x "
                         "exports")
    return base64.b64encode(public).decode("ascii")


# MARK: - write

def parse_signature(output: str, size: int) -> tuple[str, int]:
    """The EdDSA signature and length in `sign_update`'s output; ValueError unless the
    output holds exactly one `sparkle:edSignature="…" length="N"` and N is `size`."""
    matches = SIGNATURE.findall(output)
    if len(matches) != 1 or len(SIGNATURE_LOOSE.findall(output)) != 1:
        raise ValueError(f"sign_update's output holds {len(matches)} well-formed "
                         f"`sparkle:edSignature=\"…\" length=\"N\"` pairs, not one: "
                         f"{output.strip()[:300]!r}")
    signature, length = matches[0][0], int(matches[0][1])
    if length != size:
        raise ValueError(f"sign_update's length is {length}, the DMG is {size} bytes")
    return signature, length


def appcast_xml(short: str, build: str, minimum: str, url: str, notes_url: str,
                signature: str, length: int, published: str) -> str:
    """The one-item RSS 2.0 appcast."""
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        f'<rss version="2.0" xmlns:sparkle={quoteattr(SPARKLE_NS)}>\n'
        "  <channel>\n"
        "    <title>Doctored</title>\n"
        "    <item>\n"
        f"      <title>{escape('Doctored ' + short)}</title>\n"
        f"      <pubDate>{escape(published)}</pubDate>\n"
        f"      <sparkle:version>{escape(build)}</sparkle:version>\n"
        f"      <sparkle:shortVersionString>{escape(short)}</sparkle:shortVersionString>\n"
        f"      <sparkle:minimumSystemVersion>{escape(minimum)}</sparkle:minimumSystemVersion>\n"
        f"      <sparkle:fullReleaseNotesLink>{escape(notes_url)}</sparkle:fullReleaseNotesLink>\n"
        f"      <enclosure url={quoteattr(url)} length={quoteattr(str(length))} "
        f'type="application/octet-stream" sparkle:edSignature={quoteattr(signature)}/>\n'
        "    </item>\n"
        "  </channel>\n"
        "</rss>\n")


def bundle_values(app):
    """(short, build, minimum, public key) from APP/Contents/Info.plist; ValueError when one
    is not a non-empty string or the build is not a positive integer."""
    path = os.path.join(app, "Contents", "Info.plist")
    try:
        with open(path, "rb") as handle:
            info = plistlib.load(handle)
    except Exception as error:  # OSError or a plist that does not parse
        raise ValueError(f"{path}: {type(error).__name__}: {error}")
    if not isinstance(info, dict):
        raise ValueError(f"{path} is not a dictionary")
    values = []
    for key in (*BUNDLE_KEYS, "SUPublicEDKey"):
        value = info.get(key)
        if not isinstance(value, str) or not value:
            raise ValueError(f"{key} in {path} is {value!r}, not a non-empty string")
        values.append(value)
    if not POSITIVE_INTEGER.fullmatch(values[1]):
        raise ValueError(f"CFBundleVersion in {path} is {values[1]!r}, not a positive integer: "
                         "installed copies compare it, and a local build's \"0\" never ships")
    return tuple(values)


def check_url(url: str, short: str) -> None:
    """ValueError unless the DMG's URL is https:// and names this release's tag."""
    if not url.startswith("https://"):
        raise ValueError(f"--url {url!r} is not https://")
    if f"/v{short}/" not in url:
        raise ValueError(f"--url {url!r} does not name the tag v{short} (`/v{short}/`)")


def child_env():
    """This process's environment without the assistant's key or the update key's secret."""
    env = dict(os.environ)
    env.pop("ANTHROPIC_API_KEY", None)
    env.pop("SPARKLE_ED_PRIVATE_KEY", None)
    return env


def write(arguments):
    """The `write` command: 0, or 1 with the reason on stderr and no file written."""
    try:
        short, build, minimum, app_key = bundle_values(arguments.app)
        check_url(arguments.url, short)
        size = os.path.getsize(arguments.dmg)
        if not os.path.isfile(arguments.key_file):
            raise ValueError(f"the key file {arguments.key_file} does not exist")
        with open(arguments.key_file, "rb") as handle:
            derived = key_file_public_key(handle.read())
        if derived != app_key:
            raise ValueError(f"the key file's public key is {derived}, the app's SUPublicEDKey is "
                             f"{app_key}: an update signed with it would be refused by every "
                             "installed copy")
        try:
            done = subprocess.run([arguments.sign_update, "--ed-key-file", arguments.key_file,
                                   arguments.dmg], capture_output=True, text=True,
                                  timeout=TIMEOUT, env=child_env())
        except (OSError, subprocess.TimeoutExpired) as error:
            raise ValueError(f"sign_update did not run: {error}")
        if done.returncode != 0:
            raise ValueError(f"sign_update exit {done.returncode}: {done.stderr.strip()[:300]}")
        signature, length = parse_signature(done.stdout, size)
        text = appcast_xml(short, build, minimum, arguments.url, arguments.notes_url,
                           signature, length, email.utils.formatdate(usegmt=True))
        ElementTree.fromstring(text.encode("utf-8"))
        partial = arguments.out + ".partial"
        with open(partial, "w", encoding="utf-8") as handle:
            handle.write(text)
        os.replace(partial, arguments.out)
    except (OSError, ValueError, ElementTree.ParseError) as error:
        print(f"appcast: {error}", file=sys.stderr)
        return 1
    print(f"appcast: {arguments.out} — Doctored {short} ({build}), {length} bytes")
    return 0


def distribution_public_key(project_path):
    """Distribution's one DR_SPARKLE_PUBLIC_KEY in project.yml, read with updater_check's
    settings parser (T1 holds it to updater_check.PUBLIC_KEY); ValueError otherwise."""
    import updater_check  # scripts/, beside this file; loaded only by check-key

    with open(project_path, encoding="utf-8") as handle:
        text = handle.read()
    keys = [value for name, settings in updater_check.settings_entries(text)
            if name == "configs:Distribution"
            for key, value in settings if key == "DR_SPARKLE_PUBLIC_KEY"]
    if len(keys) != 1 or not keys[0]:
        raise ValueError(f"{project_path}: Distribution sets DR_SPARKLE_PUBLIC_KEY to {keys}, "
                         "not one key")
    return keys[0]


def check_key(arguments):
    """The `check-key` command, publish.yml's step after checkout: 0 when the
    SPARKLE_ED_PRIVATE_KEY environment variable's public half is Distribution's
    DR_SPARKLE_PUBLIC_KEY, else 1 naming only public keys. `write` checks again against the
    exported app; this catches a wrong secret before the archive and notarization."""
    try:
        secret = os.environ.get("SPARKLE_ED_PRIVATE_KEY", "")
        if not secret:
            raise ValueError("SPARKLE_ED_PRIVATE_KEY is not set")
        derived = key_file_public_key(secret.encode("utf-8"))
        committed = distribution_public_key(arguments.project)
        if derived != committed:
            raise ValueError(f"SPARKLE_ED_PRIVATE_KEY's public key is {derived}, Distribution's "
                             f"DR_SPARKLE_PUBLIC_KEY is {committed}: an update signed with it "
                             "would be refused by every installed copy")
    except (OSError, ValueError) as error:
        print(f"appcast: {error}", file=sys.stderr)
        return 1
    print(f"appcast: SPARKLE_ED_PRIVATE_KEY's public key is Distribution's, {committed}")
    return 0


def check_key_parser():
    parser = argparse.ArgumentParser(
        prog="appcast.py check-key",
        description="the SPARKLE_ED_PRIVATE_KEY environment variable's public half is "
                    "Distribution's DR_SPARKLE_PUBLIC_KEY")
    parser.add_argument("--project", default=PROJECT_YML, help="project.yml (default: the repo's)")
    return parser


def write_parser():
    parser = argparse.ArgumentParser(prog="appcast.py write",
                                     description="write the one-item appcast")
    parser.add_argument("--app", required=True, help="the exported Doctored.app")
    parser.add_argument("--dmg", required=True, help="the stapled Doctored.dmg")
    parser.add_argument("--sign-update", required=True, help="Sparkle's sign_update")
    parser.add_argument("--key-file", required=True,
                        help="the EdDSA private key's file (the base64 of its 32-byte seed)")
    parser.add_argument("--url", required=True,
                        help="the DMG's https:// download URL, naming the tag v<short>")
    parser.add_argument("--notes-url", required=True, help="the release page")
    parser.add_argument("--out", required=True, help="where appcast.xml goes")
    return parser


# MARK: - selftest

CANNED_SIGNATURE = "c2VsZnRlc3Qtc2lnbmF0dXJlLW5vdC1hLXJlYWwtb25lLWp1c3QtYnl0ZXMtZm9yLXM="
CANNED_LENGTH = 4096
# RFC 8032 section 7.1, TEST 1: the secret key (seed) and its public key.
TEST1_SEED = bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
TEST1_PUBLIC = bytes.fromhex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
SELFTEST_SHORT = "0.4.3"
SELFTEST_URL = f"https://example.invalid/releases/download/v{SELFTEST_SHORT}/Doctored.dmg"
SELFTEST_NOTES = f"https://example.invalid/releases/tag/v{SELFTEST_SHORT}"


def stand_in(folder, line):
    """A stand-in sign_update that prints `line` and nothing else. It records its arguments,
    one per line, in `argv` beside it, and in `env` which of SPARKLE_ED_PRIVATE_KEY and
    ANTHROPIC_API_KEY reached it (an empty line when neither did)."""
    path = os.path.join(folder, "sign_update")
    argv_file = shlex.quote(os.path.join(folder, "argv"))
    env_file = shlex.quote(os.path.join(folder, "env"))
    with open(path, "w", encoding="utf-8") as handle:
        handle.write("#!/bin/sh\n"
                     f"printf '%s\\n' \"$@\" > {argv_file}\n"
                     "printf '%s%s\\n' \"${SPARKLE_ED_PRIVATE_KEY+SPARKLE_ED_PRIVATE_KEY}\" "
                     f"\"${{ANTHROPIC_API_KEY+ANTHROPIC_API_KEY}}\" > {env_file}\n"
                     f"printf '%s\\n' {shlex.quote(line)}\n")
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)
    return path


def scratch(folder, build="3", key=TEST1_SEED, key_suffix=b"",
            public_key=base64.b64encode(TEST1_PUBLIC).decode("ascii")):
    """A scratch app (Info.plist only), DMG and key file (the base64 of `key`)."""
    app = os.path.join(folder, "Doctored.app")
    os.makedirs(os.path.join(app, "Contents"))
    with open(os.path.join(app, "Contents", "Info.plist"), "wb") as handle:
        plistlib.dump({"CFBundleShortVersionString": SELFTEST_SHORT, "CFBundleVersion": build,
                       "LSMinimumSystemVersion": "15.0", "SUPublicEDKey": public_key}, handle)
    dmg = os.path.join(folder, "Doctored.dmg")
    with open(dmg, "wb") as handle:
        handle.write(b"\0" * CANNED_LENGTH)
    key_file = os.path.join(folder, "key")
    with open(key_file, "wb") as handle:
        handle.write(base64.b64encode(key) + key_suffix)
    return app, dmg, key_file


def run_write(folder, line, build="3", url=SELFTEST_URL, key=TEST1_SEED, extra_env=None,
              **info):
    """Run `write` as a child with a stand-in sign_update: (exit code, out path, stderr).
    `extra_env` is added to the child's environment; `info` passes `public_key` to scratch."""
    app, dmg, key_file = scratch(folder, build=build, key=key, **info)
    out = os.path.join(folder, "appcast.xml")
    done = subprocess.run(
        [sys.executable, os.path.abspath(__file__), "write", "--app", app, "--dmg", dmg,
         "--sign-update", stand_in(folder, line), "--key-file", key_file,
         "--url", url, "--notes-url", SELFTEST_NOTES, "--out", out],
        capture_output=True, text=True, timeout=60, env=dict(child_env(), **(extra_env or {})))
    return done.returncode, out, done.stderr.strip()


def good_line(length=CANNED_LENGTH):
    return f'sparkle:edSignature="{CANNED_SIGNATURE}" length="{length}"'


def no_file(out):
    return not os.path.exists(out) and not os.path.exists(out + ".partial")


def read_lines(path):
    with open(path, encoding="utf-8") as handle:
        return handle.read().splitlines()


def s1(folder):
    rc, out, stderr = run_write(folder, good_line(), extra_env={
        "SPARKLE_ED_PRIVATE_KEY": "selftest-not-a-key", "ANTHROPIC_API_KEY": "dummy"})
    if rc != 0:
        return False, f"write exit {rc}: {stderr}"
    # sign_update without --ed-key-file would read the Keychain; neither secret may reach it.
    want_argv = ["--ed-key-file", os.path.join(folder, "key"), os.path.join(folder, "Doctored.dmg")]
    if read_lines(os.path.join(folder, "argv")) != want_argv:
        return False, f"sign_update ran with {read_lines(os.path.join(folder, 'argv'))}, want {want_argv}"
    if read_lines(os.path.join(folder, "env")) != [""]:
        return False, f"sign_update's environment held {read_lines(os.path.join(folder, 'env'))}"
    root = ElementTree.parse(out).getroot()
    ns = {"sparkle": SPARKLE_NS}
    items = root.findall("./channel/item")
    if root.tag != "rss" or root.get("version") != "2.0" or len(items) != 1:
        return False, f"root {root.tag} {root.get('version')}, {len(items)} items"
    item = items[0]
    enclosure = item.find("enclosure")
    published = item.findtext("pubDate") or ""
    want = {
        "channel title": ("Doctored", root.findtext("./channel/title")),
        "title": (f"Doctored {SELFTEST_SHORT}", item.findtext("title")),
        "sparkle:version": ("3", item.findtext("sparkle:version", namespaces=ns)),
        "sparkle:shortVersionString":
            (SELFTEST_SHORT, item.findtext("sparkle:shortVersionString", namespaces=ns)),
        "sparkle:minimumSystemVersion":
            ("15.0", item.findtext("sparkle:minimumSystemVersion", namespaces=ns)),
        "sparkle:fullReleaseNotesLink":
            (SELFTEST_NOTES, item.findtext("sparkle:fullReleaseNotesLink", namespaces=ns)),
        "enclosure url": (SELFTEST_URL, enclosure.get("url") if enclosure is not None else None),
        "enclosure length": (str(CANNED_LENGTH),
                             enclosure.get("length") if enclosure is not None else None),
        "enclosure type": ("application/octet-stream",
                           enclosure.get("type") if enclosure is not None else None),
        "enclosure sparkle:edSignature": (
            CANNED_SIGNATURE,
            enclosure.get(f"{{{SPARKLE_NS}}}edSignature") if enclosure is not None else None),
    }
    wrong = [f"{key}: {got!r}, want {expected!r}" for key, (expected, got) in want.items()
             if got != expected]
    try:
        if email.utils.parsedate_to_datetime(published) is None:
            wrong.append(f"pubDate {published!r} is not RFC 2822")
    except (TypeError, ValueError):
        wrong.append(f"pubDate {published!r} is not RFC 2822")
    if wrong:
        return False, "; ".join(wrong)
    return True, (f"{len(want) + 1} fields read back; sign_update ran as --ed-key-file KEY DMG "
                  "with neither secret in its environment")


def s2(_folder):
    good = good_line()
    bad = [
        "",
        "error: no key",
        f'sparkle:edSignature="{CANNED_SIGNATURE}"',
        f'length="{CANNED_LENGTH}"',
        f'sparkle:edSignature="not base64!" length="{CANNED_LENGTH}"',
        f'sparkle:edSignature="{CANNED_SIGNATURE}" length="-1"',
        good + "\n" + good,
        f'sparkle:edSignature="{CANNED_SIGNATURE}" sparkle:edSignature="x" '
        f'length="{CANNED_LENGTH}"',
    ]
    accepted = []
    for output in bad:
        try:
            parse_signature(output, CANNED_LENGTH)
            accepted.append(output)
        except ValueError:
            pass
    if accepted:
        return False, f"accepted {accepted!r}"
    try:
        parse_signature(good, CANNED_LENGTH)
    except ValueError as error:
        return False, f"the well-formed line was refused: {error}"
    return True, f"{len(bad)} malformed outputs refused, the well-formed one accepted"


def s3(folder):
    try:
        parse_signature(good_line(CANNED_LENGTH + 1), CANNED_LENGTH)
        return False, "parse_signature accepted a length one over the DMG's size"
    except ValueError:
        pass
    rc, out, stderr = run_write(folder, good_line(CANNED_LENGTH - 1))
    if rc == 0:
        return False, "write exited 0 with a length one under the DMG's size"
    if not no_file(out):
        return False, f"write left {out} behind"
    return True, f"refused ({stderr[:120]})"


def s4(folder):
    cases = (("build \"0\"", {"build": "0"}, "CFBundleVersion"),
             ("an http:// URL", {"url": SELFTEST_URL.replace("https://", "http://")}, "https://"),
             ("a URL for another tag", {"url": SELFTEST_URL.replace(f"/v{SELFTEST_SHORT}/",
                                                                    "/v0.4.2/")}, "tag"),
             ("an empty SUPublicEDKey", {"public_key": ""}, "SUPublicEDKey"))
    details = []
    for index, (name, overrides, needle) in enumerate(cases):
        case_folder = os.path.join(folder, str(index))
        os.makedirs(case_folder)
        rc, out, stderr = run_write(case_folder, good_line(), **overrides)
        if rc == 0:
            return False, f"write exited 0 with {name}"
        if not no_file(out):
            return False, f"write left {out} behind with {name}"
        if needle not in stderr:
            return False, f"{name} was refused for another reason: {stderr[:160]}"
        details.append(name)
    return True, "refused, with no file: " + ", ".join(details)


def s5(_folder):
    want = base64.b64encode(TEST1_PUBLIC).decode("ascii")
    if public_key_from_seed(TEST1_SEED) != TEST1_PUBLIC:
        return False, "test 1's seed does not derive test 1's public key"
    encoded = base64.b64encode(TEST1_SEED)
    for suffix in (b"", b"\n", b"\r\n"):
        got = key_file_public_key(encoded + suffix)
        if got != want:
            return False, f"the seed with {suffix!r} derives {got}, want {want}"
    # The older format of test 1: its expanded private key, then its public key.
    older = base64.b64encode(expanded_private_key(TEST1_SEED) + TEST1_PUBLIC)
    got = key_file_public_key(older + b"\n")
    if got != want:
        return False, f"test 1 in the older format derives {got}, want {want}"
    other_public = public_key_from_seed(bytes(range(1, SEED_BYTES + 1)))
    refused = {
        "an older-format key whose halves disagree":
            (base64.b64encode(expanded_private_key(TEST1_SEED) + other_public), OLDER_FORMAT_MISMATCH),
        "96 arbitrary bytes": (base64.b64encode(bytes(range(OLDER_FORMAT_BYTES))), OLDER_FORMAT_MISMATCH),
        "an empty file": (b"", "empty"),
        "whitespace only": (b" \n", "empty"),
        "not base64": (b"selftest!not*base64", "not base64"),
        "31 bytes": (base64.b64encode(TEST1_SEED[:31]), "31 bytes"),
        "33 bytes": (base64.b64encode(TEST1_SEED + b"\1"), "33 bytes"),
    }
    for name, (content, needle) in refused.items():
        try:
            key_file_public_key(content)
            return False, f"{name} was accepted"
        except ValueError as error:
            message = str(error)
        if needle not in message:
            return False, f"{name} was refused for another reason: {message}"
        printed = content.strip(ASCII_WHITESPACE).decode("ascii")
        if printed and printed in message:
            return False, f"the refusal of {name} printed the key file's content"
    return True, (f"test 1 derives with no suffix, \"\\n\" and \"\\r\\n\" and in the older format; "
                  f"{len(refused)} key files refused without printing their content")


def s6(folder):
    other_seed = bytes(range(1, SEED_BYTES + 1))
    rc, out, stderr = run_write(folder, good_line(), key=other_seed)
    if rc == 0:
        return False, "write exited 0 with a key whose public half is not SUPublicEDKey"
    if not no_file(out):
        return False, f"write left {out} behind"
    expected = base64.b64encode(public_key_from_seed(other_seed)).decode("ascii")
    if expected not in stderr or "SUPublicEDKey" not in stderr:
        return False, f"refused for another reason: {stderr[:200]}"
    if base64.b64encode(other_seed).decode("ascii") in stderr:
        return False, "the refusal printed the key itself"
    return True, "refused with the two public keys only, no file"


def run_check_key(folder, secret):
    """Run `check-key` as a child against a synthetic project.yml whose Distribution key is test
    1's public key, with SPARKLE_ED_PRIVATE_KEY `secret` (None: unset): (exit, stdout, stderr)."""
    project = os.path.join(folder, "project.yml")
    with open(project, "w", encoding="utf-8") as handle:
        handle.write("settings:\n  configs:\n    Release:\n      DR_SPARKLE_PUBLIC_KEY: \"\"\n"
                     "    Distribution:\n      DR_SPARKLE_PUBLIC_KEY: "
                     f"{base64.b64encode(TEST1_PUBLIC).decode('ascii')}\n")
    env = child_env()
    if secret is not None:
        env["SPARKLE_ED_PRIVATE_KEY"] = secret
    done = subprocess.run([sys.executable, os.path.abspath(__file__), "check-key",
                           "--project", project],
                          capture_output=True, text=True, timeout=60, env=env)
    return done.returncode, done.stdout.strip(), done.stderr.strip()


def s7(folder):
    want = base64.b64encode(TEST1_PUBLIC).decode("ascii")
    seed = base64.b64encode(TEST1_SEED).decode("ascii")
    rc, stdout, stderr = run_check_key(folder, seed + "\n")
    if rc != 0 or want not in stdout:
        return False, f"test 1's seed: exit {rc}, {stdout or stderr}"
    other_seed = base64.b64encode(bytes(range(1, SEED_BYTES + 1))).decode("ascii")
    other_public = base64.b64encode(public_key_from_seed(bytes(range(1, SEED_BYTES + 1)))).decode("ascii")
    rc, stdout, stderr = run_check_key(folder, other_seed)
    if rc == 0:
        return False, "check-key exited 0 with another key"
    if want not in stderr or other_public not in stderr:
        return False, f"another key was refused for another reason: {stderr[:200]}"
    if other_seed in stderr or other_seed in stdout:
        return False, "the refusal printed the key itself"
    for name, secret in (("an unset secret", None), ("an empty secret", "")):
        rc, stdout, stderr = run_check_key(folder, secret)
        if rc == 0 or "SPARKLE_ED_PRIVATE_KEY is not set" not in stderr:
            return False, f"{name}: exit {rc}, {stderr[:160]}"
    return True, "the matching key passes; another key, an unset and an empty secret are refused"


def selftest():
    print("designs/sparkle-updates.md — appcast.py's selftest: the one-item appcast and its "
          "refusals\n")
    passed = 0
    for name, case in (("S1 the appcast's every field reads back", s1),
                       ("S2 malformed sign_update output is refused", s2),
                       ("S3 a length other than the DMG's size is refused", s3),
                       ("S4 a build, scheme or tag that cannot ship is refused", s4),
                       ("S5 the key file's public half is derived per RFC 8032", s5),
                       ("S6 a key that is not the app's SUPublicEDKey is refused", s6),
                       ("S7 check-key matches the secret against project.yml", s7)):
        folder = tempfile.mkdtemp(prefix="appcast-selftest-")
        try:
            ok, detail = case(folder)
        except Exception as error:  # a broken case is a scored failure, not a crash
            ok, detail = False, f"{type(error).__name__}: {error}"
        finally:
            shutil.rmtree(folder, ignore_errors=True)
        passed += ok
        print(f"  {'ok  ' if ok else 'FAIL'} {name} — {detail}")
    print(f"\nappcast selftest: {passed}/{SELFTEST_TOTAL}")
    return 0 if passed == SELFTEST_TOTAL else 1


def main(argv):
    if argv == ["--selftest"]:
        return selftest()
    if argv[:1] == ["write"]:
        return write(write_parser().parse_args(argv[1:]))
    if argv[:1] == ["check-key"]:
        return check_key(check_key_parser().parse_args(argv[1:]))
    print(__doc__.split("\n\n")[2] if "\n\n" in __doc__ else __doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
