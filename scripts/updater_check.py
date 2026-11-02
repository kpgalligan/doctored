#!/usr/bin/env python3
"""The updater's build check (designs/sparkle-updates.md): Sparkle is loaded by Distribution alone.

Sparkle.framework is embedded in every configuration (xcodegen links a package per
target), but only Distribution compiles code naming it (`#if DR_UPDATER`), and
`-dead_strip_dylibs` drops its load command everywhere else: a Debug or Release
build, ad-hoc signed with the hardened runtime, could not load the ad-hoc
framework ("different Team IDs") and would not launch. These rows hold the
builds to that.

    python3 scripts/updater_check.py --selftest
    python3 scripts/updater_check.py dist --release R --dist D
    python3 scripts/updater_check.py mutants --release R --dist D
    python3 scripts/updater_check.py published --app A --version V --build B

Release rows (`release_failures(bundle)`, which the gate's `launch` step runs over
the Release bundle, and `dist`):

* R1 — `Contents/Frameworks/Sparkle.framework/Versions/B/Sparkle` is a file (R2's
  positive control);
* R2 — `otool -L` lists at least one load command, none under `/Sparkle.framework/`;
* R3 — `SUFeedURL` and `SUPublicEDKey` are present and empty;
* R4 — the executable lacks NEEDLE;
* R5 — `codesign --verify --deep --strict` passes.

Distribution rows (`dist`, over Distribution unless named):

* D1 — the framework binary's TeamIdentifier (`codesign -dv --verbose=4`) is the
  app binary's (`not set` on both locally); a missing framework binary fails;
* D2 — the other Mach-Os under the framework (found by magic, symlinks not
  followed; 4 in 2.10.1) number at least one, and each has `runtime` in its flags,
  the app binary's TeamIdentifier (`not set` on both locally) and passes
  `codesign --verify --strict`;
* D3 — `SUFeedURL` is FEED_URL;  D4 — `SUPublicEDKey` is PUBLIC_KEY, the decided
  key (strict base64 of 32 bytes);
* D5 — the executable contains NEEDLE (R4's positive control);
* D6 — Distribution's load commands are Release's plus exactly one SPARKLE_LOAD;
* D7 — `codesign --verify --deep --strict` passes;
* D8 — both bundles' `Acknowledgements.txt` hold
  `build/SourcePackages/artifacts/sparkle/Sparkle/LICENSE` verbatim, under a
  `Sparkle <SPARKLE_VERSION>` heading;
* D9 — a smoke launch: Distribution `ditto`ed to a scratch folder, its
  `CFBundleVersion` set to "1" (past `AppUpdater.availability`'s build guard) and
  its `CFBundleIdentifier` to SMOKE_BUNDLE_ID (any defaults land in a throwaway
  domain), re-signed `--deep` ad hoc WITHOUT the runtime, launched with
  `verification_isolation.owned_environment`: `tools/list` must answer within
  60 s, the log must hold `[AppUpdater] automation` and must not hold
  `[AppUpdater] started`, and SMOKE_BUNDLE_ID's defaults domain must hold no
  `SU*` key (where Sparkle would write); then the copy is terminated,
  unregistered (`lsregister -u`) and deleted, and the domain is deleted with
  `defaults delete` (also before the launch), then the empty plist cfprefsd
  leaves is unlinked. It is the one local launch of a
  binary with Sparkle loaded, and it never constructs an updater.

The last line is `updater_check dist: <passed>/14`.

`published` (the publish workflow, after the export) runs D1 and D2 with the team
also required not to be `not set` (upstream signs the helpers ad hoc), D3, D4, D5,
D7 and D8 over the one app, and:

* P1 — `CFBundleShortVersionString` is V, `CFBundleVersion` is B and a positive
  integer, `LSMinimumSystemVersion` is "15.0";
* P2 — `otool -L` holds exactly one Sparkle entry, and it is SPARKLE_LOAD.

The last line is `updater_check published: <passed>/9`.

`mutants` makes the rows fail on purpose. Each case `ditto`s one bundle into a
scratch folder (never `build/`; one copy at a time), mutates the copy, runs every
row but D9 with the copy in place of the real bundle, and passes when the failing
rows are exactly its declared set. A plist or resource edit breaks the seal, so
each edit case re-seals with `codesign --force --sign - -o runtime <bundle>`,
without `--deep`. The sets are declared before the first run (MUTANTS); a
mismatch is investigated, never widened. The last line is
`updater_check mutants: <passed>/8`.

`--selftest` (`make release-check`, no build): T1 project.yml's settings lines
(Distribution alone sets the feed, PUBLIC_KEY and `DR_UPDATER`, in either
`DR_UPDATER` or `-DDR_UPDATER` form), T2 the key validator, T3 R3/D3/D4 over synthetic Info.plists, T4 D6/P2 over
canned `otool -L` text. The last line is `updater_check selftest: <passed>/4`.

Standard library only; D9 also imports `mcp_client` and `verification_isolation`.
It never uses `nm`: the archive strips symbols. Every command exits 0 only when
every row or case passes.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import collections
import http.client
import mmap
import os
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROJECT_YML = ROOT / "project.yml"
SPARKLE_LICENSE = ROOT / "build/SourcePackages/artifacts/sparkle/Sparkle/LICENSE"

FEED_URL = "https://github.com/kpgalligan/doctored/releases/latest/download/appcast.xml"
# The decided EdDSA public key, Frame Job's (designs/sparkle-updates.md, "The key"). Pinned here
# as well as in project.yml, so a rotation changes both in one reviewed commit: a key changed
# with its secret alone would pass every other check and strand every installed copy.
PUBLIC_KEY = "v0qZeH0YUq+E1zznO2RjbaKZcIMuKSWM1hfEAuz7otI="
SPARKLE_VERSION = "2.10.1"
# The class the updater's code constructs. 28 bytes: Swift stores a literal of 15 UTF-8 bytes
# or fewer inline in `-O` code, where no byte search finds it.
NEEDLE = b"SPUStandardUpdaterController"
SPARKLE_LOAD = "@rpath/Sparkle.framework/Versions/B/Sparkle"
SPARKLE_IN_PATH = "/Sparkle.framework/"
FRAMEWORK = "Contents/Frameworks/Sparkle.framework"
FRAMEWORK_BINARY = FRAMEWORK + "/Versions/B/Sparkle"
INFO_PLIST = "Contents/Info.plist"
ACKNOWLEDGEMENTS = "Contents/Resources/Acknowledgements.txt"
ACK_HEADING = re.compile(r"^Sparkle " + re.escape(SPARKLE_VERSION) + r"\n-+\n", re.M)
MINIMUM_SYSTEM = "15.0"
UPDATER_KEYS = ("SUFeedURL", "SUPublicEDKey")
POSITIVE_INTEGER = re.compile(r"[1-9][0-9]*")
# The first four bytes of a Mach-O, thin or universal, either byte order.
MACHO_MAGIC = {bytes.fromhex(magic) for magic in
               ("feedface", "cefaedfe", "feedfacf", "cffaedfe", "cafebabe", "bebafeca")}
TEAM = re.compile(r"^TeamIdentifier=(.*)$", re.M)
# The CodeDirectory line's flags; `Executable Segment flags=0x1` has no parentheses.
FLAGS = re.compile(r"flags=0x[0-9a-fA-F]+\(([^)]*)\)")
TIMEOUT = 300
SMOKE_TIMEOUT_S = 60
# After tools/list answers, how long the log may take to show the decision line.
DECISION_WAIT_S = 10
SMOKE_BUNDLE_ID = "com.kgalligan.doctored.updater-check"
LSREGISTER = ("/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/"
              "LaunchServices.framework/Versions/A/Support/lsregister")

Result = tuple[bool, str]


# MARK: - tools

def child_env() -> dict[str, str]:
    """This process's environment without the assistant's key or the update key's secret."""
    env = dict(os.environ)
    env.pop("ANTHROPIC_API_KEY", None)
    env.pop("SPARKLE_ED_PRIVATE_KEY", None)
    return env


def run_tool(argv: list[str]) -> tuple[subprocess.CompletedProcess | None, str]:
    """(the completed process, "") or (None, why it did not run); output as text."""
    try:
        return subprocess.run(argv, capture_output=True, text=True, errors="replace",
                              timeout=TIMEOUT, env=child_env()), ""
    except FileNotFoundError:
        return None, f"{argv[0]} is not on PATH"
    except subprocess.TimeoutExpired:
        return None, f"{' '.join(argv[:2])} timed out after {TIMEOUT} s"
    except OSError as error:
        return None, f"{argv[0]}: {error}"


def tool_failure(done: subprocess.CompletedProcess, argv: list[str]) -> str:
    return f"{' '.join(argv[:-1])} exit {done.returncode}: {done.stderr.strip()[:300]}"


def read_info(bundle: Path) -> tuple[dict | None, str]:
    """The bundle's Info.plist, or None with the reason."""
    path = bundle / INFO_PLIST
    try:
        with path.open("rb") as handle:
            info = plistlib.load(handle)
    except Exception as error:  # OSError or a plist that does not parse
        return None, f"{path}: {type(error).__name__}: {error}"
    if not isinstance(info, dict):
        return None, f"{path} is not a dictionary"
    return info, ""


def executable(bundle: Path) -> Path:
    """The bundle's main executable (`CFBundleExecutable`, else `Doctored`)."""
    info, _ = read_info(bundle)
    name = info.get("CFBundleExecutable") if info else None
    return bundle / "Contents/MacOS" / (name if isinstance(name, str) and name else "Doctored")


def parse_otool(text: str) -> list[str]:
    """The dylib paths in `otool -L` output: its indented lines, each without its
    `(compatibility version …)` tail; the unindented header lines are skipped."""
    return [line.strip().rsplit(" (", 1)[0] for line in text.splitlines()
            if line[:1] in ("\t", " ") and line.strip()]


def load_commands(binary: Path) -> tuple[list[str] | None, str]:
    """The dylib paths `otool -L` lists for the binary (at least one), or None with the reason."""
    argv = ["otool", "-L", str(binary)]
    done, reason = run_tool(argv)
    if done is None:
        return None, reason
    if done.returncode != 0:
        return None, tool_failure(done, argv)
    paths = parse_otool(done.stdout)
    if not paths:
        return None, f"otool -L listed no load command for {binary}"
    return paths, ""


def contains_needle(binary: Path) -> tuple[bool | None, str]:
    """Whether the file holds NEEDLE, or None with the reason."""
    try:
        with binary.open("rb") as handle:
            if os.fstat(handle.fileno()).st_size == 0:
                return False, ""
            with mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as mapped:
                return mapped.find(NEEDLE) >= 0, ""
    except OSError as error:
        return None, f"{binary}: {error}"


def team_identifier(path: Path) -> tuple[str | None, str]:
    """The TeamIdentifier `codesign -dv --verbose=4` shows (`not set` kept), or None with why."""
    argv = ["codesign", "-dv", "--verbose=4", str(path)]
    done, reason = run_tool(argv)
    if done is None:
        return None, reason
    if done.returncode != 0:
        return None, tool_failure(done, argv)
    match = TEAM.search(done.stderr)
    if match is None:
        return None, f"no TeamIdentifier= line in codesign -dv's display of {path}"
    return match.group(1).strip(), ""


def is_macho(path: Path) -> bool:
    try:
        with path.open("rb") as handle:
            return handle.read(4) in MACHO_MAGIC
    except OSError:
        return False


def helper_paths(framework: Path) -> list[Path]:
    """Every Mach-O under the framework but its own binary, symlinks not followed, in a stable
    order (folders sorted, a folder's files before its subfolders)."""
    own = os.path.realpath(framework / "Versions/B/Sparkle")
    helpers = []
    for folder, directories, files in os.walk(framework):
        directories.sort()
        for name in sorted(files):
            path = Path(folder) / name
            if path.is_symlink() or os.path.realpath(path) == own:
                continue
            if is_macho(path):
                helpers.append(path)
    return helpers


def is_public_key(value) -> bool:
    """Strict base64 of exactly 32 bytes (an Ed25519 public key), as `AppUpdater.isPublicKey`."""
    if not isinstance(value, str) or not value:
        return False
    try:
        raw = base64.b64decode(value, validate=True)
    except (binascii.Error, ValueError):
        return False
    return len(raw) == 32 and base64.b64encode(raw).decode("ascii") == value


# MARK: - rows

def r1_framework(bundle: Path) -> Result:
    path = bundle / FRAMEWORK_BINARY
    if not path.is_file():
        return False, f"{path}: no such file"
    return True, "embedded"


def r2_no_sparkle_load(bundle: Path) -> Result:
    paths, reason = load_commands(executable(bundle))
    if paths is None:
        return False, reason
    held = [path for path in paths if SPARKLE_IN_PATH in path]
    if held:
        return False, f"load commands {held}"
    return True, f"{len(paths)} load commands, none Sparkle's"


def r3_keys(info: dict) -> Result:
    """Both updater keys present and empty (Debug and Release read `:default=`)."""
    for key in UPDATER_KEYS:
        if key not in info:
            return False, f"{key} is absent"
        if info[key] != "":
            return False, f"{key} is {info[key]!r}, not empty"
    return True, "SUFeedURL and SUPublicEDKey are empty"


def d3_feed(info: dict) -> Result:
    value = info.get("SUFeedURL")
    if value != FEED_URL:
        return False, f"SUFeedURL is {value!r}, not {FEED_URL}"
    return True, FEED_URL


def d4_key(info: dict) -> Result:
    value = info.get("SUPublicEDKey")
    if not is_public_key(value):
        return False, f"SUPublicEDKey {value!r} is not strict base64 of 32 bytes"
    if value != PUBLIC_KEY:
        return False, f"SUPublicEDKey is {value!r}, not the decided key {PUBLIC_KEY}"
    return True, PUBLIC_KEY


def with_info(bundle: Path, check) -> Result:
    info, reason = read_info(bundle)
    if info is None:
        return False, reason
    return check(info)


def r4_no_needle(bundle: Path) -> Result:
    binary = executable(bundle)
    found, reason = contains_needle(binary)
    if found is None:
        return False, reason
    if found:
        return False, f"{binary} holds {NEEDLE.decode()}"
    return True, f"no {NEEDLE.decode()}"


def d5_needle(bundle: Path) -> Result:
    binary = executable(bundle)
    found, reason = contains_needle(binary)
    if found is None:
        return False, reason
    if not found:
        return False, f"{binary} lacks {NEEDLE.decode()}"
    return True, f"holds {NEEDLE.decode()}"


def sealed(bundle: Path) -> Result:
    argv = ["codesign", "--verify", "--deep", "--strict", str(bundle)]
    done, reason = run_tool(argv)
    if done is None:
        return False, reason
    if done.returncode != 0:
        return False, tool_failure(done, argv)
    return True, "verifies"


def d1_team(bundle: Path, require_team: bool) -> Result:
    framework_binary = bundle / FRAMEWORK_BINARY
    if not framework_binary.is_file():
        return False, f"{framework_binary}: no such file"
    want, reason = team_identifier(Path(os.path.realpath(executable(bundle))))
    if want is None:
        return False, f"the app binary: {reason}"
    held, reason = team_identifier(Path(os.path.realpath(framework_binary)))
    if held is None:
        return False, f"the framework binary: {reason}"
    if held != want:
        return False, f"the framework's TeamIdentifier is {held!r}, the app's {want!r}"
    if require_team and held == "not set":
        return False, "the TeamIdentifier is not set: the published build is Developer ID signed"
    return True, f"TeamIdentifier {held}"


def helper_signature(path: Path, team: str) -> Result:
    """The helper has `runtime` in its flags, the app binary's TeamIdentifier `team`, and
    passes `codesign --verify --strict`."""
    argv = ["codesign", "-dv", "--verbose=4", str(path)]
    done, reason = run_tool(argv)
    if done is None:
        return False, reason
    if done.returncode != 0:
        return False, tool_failure(done, argv)
    match = FLAGS.search(done.stderr)
    if match is None:
        return False, f"no `flags=0x…(…)` in codesign -dv's display of {path}"
    if "runtime" not in [flag.strip() for flag in match.group(1).split(",")]:
        return False, f"{path}'s flags ({match.group(1)}) lack runtime"
    held = TEAM.search(done.stderr)
    if held is None:
        return False, f"no TeamIdentifier= line in codesign -dv's display of {path}"
    if held.group(1).strip() != team:
        return False, f"{path}'s TeamIdentifier is {held.group(1).strip()!r}, the app's {team!r}"
    argv = ["codesign", "--verify", "--strict", str(path)]
    done, reason = run_tool(argv)
    if done is None:
        return False, reason
    if done.returncode != 0:
        return False, tool_failure(done, argv)
    return True, match.group(1)


def d2_helpers(bundle: Path, require_team: bool) -> Result:
    """Every helper with the runtime and the app binary's team (also required to be set when
    `require_team`: upstream signs the helpers ad hoc, so a published build must re-sign them)."""
    framework = bundle / FRAMEWORK
    if not framework.is_dir():
        return False, f"{framework}: no such directory, so no helper"
    helpers = helper_paths(framework)
    if not helpers:
        return False, f"no Mach-O under {framework} but its own binary"
    team, reason = team_identifier(Path(os.path.realpath(executable(bundle))))
    if team is None:
        return False, f"the app binary: {reason}"
    if require_team and team == "not set":
        return False, "the app's TeamIdentifier is not set: the published build is Developer ID signed"
    for path in helpers:
        ok, reason = helper_signature(path, team)
        if not ok:
            return False, reason
    return True, f"{len(helpers)} helpers with the runtime and TeamIdentifier {team}, each verifying"


def compare_loads(release: list[str], dist: list[str]) -> Result:
    """D6: Distribution's load commands are Release's plus exactly one SPARKLE_LOAD."""
    extra = collections.Counter(dist) - collections.Counter(release)
    missing = collections.Counter(release) - collections.Counter(dist)
    if extra != collections.Counter({SPARKLE_LOAD: 1}) or missing:
        return False, (f"Distribution adds {sorted(extra.elements())} and lacks "
                       f"{sorted(missing.elements())}; want only {SPARKLE_LOAD} added")
    return True, f"Release's {len(release)} plus {SPARKLE_LOAD}"


def one_sparkle_load(paths: list[str]) -> Result:
    """P2: exactly one Sparkle load command, and it is SPARKLE_LOAD."""
    held = [path for path in paths if SPARKLE_IN_PATH in path]
    if held != [SPARKLE_LOAD]:
        return False, f"Sparkle load commands {held}, want [{SPARKLE_LOAD!r}]"
    return True, SPARKLE_LOAD


def d6_loads(release: Path, dist: Path) -> Result:
    release_paths, reason = load_commands(executable(release))
    if release_paths is None:
        return False, f"Release: {reason}"
    dist_paths, reason = load_commands(executable(dist))
    if dist_paths is None:
        return False, f"Distribution: {reason}"
    return compare_loads(release_paths, dist_paths)


def p2_load(bundle: Path) -> Result:
    paths, reason = load_commands(executable(bundle))
    if paths is None:
        return False, reason
    return one_sparkle_load(paths)


def acknowledgement_holds(text: str, license_text: str) -> Result:
    """The licence verbatim, after a `Sparkle <SPARKLE_VERSION>` heading and its underline."""
    heading = ACK_HEADING.search(text)
    if heading is None:
        return False, f"no `Sparkle {SPARKLE_VERSION}` heading"
    if license_text not in text[heading.end():]:
        return False, f"Sparkle's LICENSE is not verbatim under the `Sparkle {SPARKLE_VERSION}` heading"
    return True, ""


def d8_acknowledgements(bundles: tuple[Path, ...]) -> Result:
    try:
        license_text = SPARKLE_LICENSE.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        return False, f"{SPARKLE_LICENSE}: {error} (the package is fetched by the first build)"
    if not license_text.strip():
        return False, f"{SPARKLE_LICENSE} is empty"
    for bundle in bundles:
        path = bundle / ACKNOWLEDGEMENTS
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as error:
            return False, f"{path}: {error}"
        ok, reason = acknowledgement_holds(text, license_text)
        if not ok:
            return False, f"{path}: {reason}"
    return True, f"{len(bundles)} bundle(s) hold the {len(license_text)}-character LICENSE"


def p1_versions(info: dict, version: str, build: str) -> Result:
    short = info.get("CFBundleShortVersionString")
    number = info.get("CFBundleVersion")
    minimum = info.get("LSMinimumSystemVersion")
    if short != version:
        return False, f"CFBundleShortVersionString is {short!r}, not {version!r}"
    if number != build:
        return False, f"CFBundleVersion is {number!r}, not {build!r}"
    if not isinstance(number, str) or not POSITIVE_INTEGER.fullmatch(number):
        return False, f"CFBundleVersion {number!r} is not a positive integer"
    if minimum != MINIMUM_SYSTEM:
        return False, f"LSMinimumSystemVersion is {minimum!r}, not {MINIMUM_SYSTEM!r}"
    return True, f"{short} ({number}), macOS {minimum}"


# MARK: - D9, the smoke launch

def copy_bundle(source: Path, target: Path) -> None:
    """`ditto` the bundle; RuntimeError on a failure."""
    argv = ["ditto", str(source), str(target)]
    done, reason = run_tool(argv)
    if done is None:
        raise RuntimeError(reason)
    if done.returncode != 0:
        raise RuntimeError(tool_failure(done, argv))


def sign(path: Path, *options: str) -> None:
    """Re-sign ad hoc with the options; RuntimeError on a failure."""
    argv = ["codesign", "--force", *options, "--sign", "-", str(path)]
    done, reason = run_tool(argv)
    if done is None:
        raise RuntimeError(reason)
    if done.returncode != 0:
        raise RuntimeError(tool_failure(done, argv))


def edit_info(bundle: Path, changes: dict) -> None:
    """Rewrite the bundle's Info.plist with the changes; RuntimeError on a failure."""
    path = bundle / INFO_PLIST
    try:
        with path.open("rb") as handle:
            info = plistlib.load(handle)
        info.update(changes)
        with path.open("wb") as handle:
            plistlib.dump(info, handle)
    except (OSError, ValueError, plistlib.InvalidFileException) as error:
        raise RuntimeError(f"rewrite {path}: {error}")


def smoke_launch(executable_path: Path, folder: Path) -> tuple[str, str]:
    """Launch the executable as an owned instance; (verdict, log text). The verdict is "" when
    tools/list answered, else why not."""
    import mcp_client
    from verification_isolation import owned_environment

    log_path = folder / "app.log"
    agent_socket = mcp_client.scratch_socket("dr-upd-")
    process = None
    verdict = "the app did not start"
    try:
        env = dict(os.environ, **owned_environment(folder / "state", agent_socket))
        with log_path.open("wb") as log:
            process = subprocess.Popen([str(executable_path), "-ApplePersistenceIgnoreState", "YES"],
                                       env=env, stdin=subprocess.DEVNULL, stdout=log,
                                       stderr=subprocess.STDOUT)
        client = mcp_client.Client(agent_socket)
        started = time.monotonic()
        last = "no reply"
        while True:
            status = process.poll()
            if status is not None:
                verdict = f"the app exited with status {status} before answering tools/list"
                break
            try:
                reply = client.post({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}},
                                    timeout=10)
                tools = reply["result"]["tools"]
                if not isinstance(tools, list) or not tools:
                    raise ValueError("tools/list returned no tools")
            except (OSError, ValueError, KeyError, TypeError, RuntimeError,
                    http.client.HTTPException) as error:
                last = f"{type(error).__name__}: {error}"
            else:
                verdict = ""
                break
            if time.monotonic() - started >= SMOKE_TIMEOUT_S:
                verdict = f"no tools/list reply within {SMOKE_TIMEOUT_S} s ({last})"
                break
            time.sleep(0.2)
        if not verdict:
            deadline = time.monotonic() + DECISION_WAIT_S
            while time.monotonic() < deadline and "[AppUpdater] " not in read_log(log_path):
                time.sleep(0.2)
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        mcp_client.remove_scratch(agent_socket)
    return verdict, read_log(log_path)


def read_log(path: Path) -> str:
    try:
        return path.read_text(errors="replace")
    except OSError:
        return ""


def smoke_defaults() -> tuple[dict | None, str]:
    """The scratch copy's defaults domain as `defaults export` reads it through cfprefsd (an
    empty dictionary when it has none), or None with the reason."""
    argv = ["defaults", "export", SMOKE_BUNDLE_ID, "-"]
    done, reason = run_tool(argv)
    if done is None:
        return None, reason
    if done.returncode != 0:
        return None, tool_failure(done, argv)
    try:
        domain = plistlib.loads(done.stdout.encode("utf-8"))
    except Exception as error:  # a plist that does not parse
        return None, f"defaults export {SMOKE_BUNDLE_ID}: {type(error).__name__}: {error}"
    if not isinstance(domain, dict):
        return None, f"defaults export {SMOKE_BUNDLE_ID} is not a dictionary"
    return domain, ""


def delete_smoke_defaults() -> None:
    """Remove the scratch copy's defaults domain through `defaults`, not by unlinking its plist
    first, because cfprefsd caches the domain; a domain that does not exist is not an error.
    cfprefsd then leaves an empty plist behind, which is unlinked once it reads back empty."""
    subprocess.run(["defaults", "delete", SMOKE_BUNDLE_ID], stdin=subprocess.DEVNULL,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
                   env=child_env())
    path = Path.home() / "Library/Preferences" / f"{SMOKE_BUNDLE_ID}.plist"
    try:
        with path.open("rb") as handle:
            empty = plistlib.load(handle) == {}
        if empty:
            path.unlink()
    except Exception:  # absent, unreadable or not a plist: left alone
        pass


def d9_smoke(dist: Path) -> Result:
    scratch = Path(tempfile.mkdtemp(prefix="dr-updater-check-"))
    copy = scratch / "Doctored.app"
    try:
        delete_smoke_defaults()
        try:
            copy_bundle(dist, copy)
            edit_info(copy, {"CFBundleVersion": "1", "CFBundleIdentifier": SMOKE_BUNDLE_ID})
            sign(copy, "--deep")
        except RuntimeError as error:
            return False, f"the scratch copy: {error}"
        verdict, log = smoke_launch(executable(copy), scratch)
        decided = sorted({line.strip() for line in log.splitlines() if line.startswith("[AppUpdater] ")})
        if verdict:
            return False, f"{verdict}; log tail: {log.strip()[-300:]!r}"
        if "[AppUpdater] automation" not in decided:
            return False, f"tools/list answered, but the log holds {decided} and no [AppUpdater] automation"
        if "[AppUpdater] started" in decided:
            return False, "the log holds [AppUpdater] started: an owned launch constructed the updater"
        domain, reason = smoke_defaults()
        if domain is None:
            return False, f"the scratch copy's defaults: {reason}"
        written = sorted(key for key in domain if key.startswith("SU"))
        if written:
            return False, f"the scratch copy's defaults hold {written}: Sparkle wrote its keys"
        return True, ("tools/list answered with Sparkle loaded; [AppUpdater] automation, nothing "
                      "started, no SU* default")
    finally:
        subprocess.run([LSREGISTER, "-u", str(copy)], stdin=subprocess.DEVNULL,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        shutil.rmtree(scratch, ignore_errors=True)
        delete_smoke_defaults()


# MARK: - row tables

Row = tuple[str, str, object]


def release_rows(bundle: Path) -> list[Row]:
    return [
        ("R1", "Release embeds Sparkle.framework", lambda: r1_framework(bundle)),
        ("R2", "Release has no Sparkle load command", lambda: r2_no_sparkle_load(bundle)),
        ("R3", "Release's SUFeedURL and SUPublicEDKey are empty", lambda: with_info(bundle, r3_keys)),
        ("R4", "Release's executable lacks the updater class", lambda: r4_no_needle(bundle)),
        ("R5", "Release passes codesign --verify --deep --strict", lambda: sealed(bundle)),
    ]


def dist_rows(release: Path, dist: Path, smoke: bool) -> list[Row]:
    rows: list[Row] = [
        ("D1", "the framework binary is signed by the app binary's team", lambda: d1_team(dist, False)),
        ("D2", "every Sparkle helper has the runtime and the app's team, and verifies",
         lambda: d2_helpers(dist, False)),
        ("D3", "Distribution's SUFeedURL is the feed", lambda: with_info(dist, d3_feed)),
        ("D4", "Distribution's SUPublicEDKey is the decided key", lambda: with_info(dist, d4_key)),
        ("D5", "Distribution's executable holds the updater class", lambda: d5_needle(dist)),
        ("D6", "Distribution loads Release's libraries plus Sparkle", lambda: d6_loads(release, dist)),
        ("D7", "Distribution passes codesign --verify --deep --strict", lambda: sealed(dist)),
        ("D8", "both acknowledgements carry Sparkle's LICENSE", lambda: d8_acknowledgements((release, dist))),
    ]
    if smoke:
        rows.append(("D9", "a scratch copy launches under automation and starts no updater",
                     lambda: d9_smoke(dist)))
    return rows + release_rows(release)


def published_rows(app: Path, version: str, build: str) -> list[Row]:
    return [
        ("D1", "the framework binary is signed by the app's team, which is set", lambda: d1_team(app, True)),
        ("D2", "every Sparkle helper has the runtime and the app's team, which is set, and verifies",
         lambda: d2_helpers(app, True)),
        ("D3", "SUFeedURL is the feed", lambda: with_info(app, d3_feed)),
        ("D4", "SUPublicEDKey is the decided key", lambda: with_info(app, d4_key)),
        ("D5", "the executable holds the updater class", lambda: d5_needle(app)),
        ("D7", "the app passes codesign --verify --deep --strict", lambda: sealed(app)),
        ("D8", "the acknowledgements carry Sparkle's LICENSE", lambda: d8_acknowledgements((app,))),
        ("P1", "the version, build and minimum system", lambda: with_info(
            app, lambda info: p1_versions(info, version, build))),
        ("P2", "exactly one Sparkle load command, the expected one", lambda: p2_load(app)),
    ]


def evaluate(rows: list[Row]) -> list[tuple[str, str, bool, str]]:
    """Each row's (id, name, ok, detail); a row that raises is a failure, not a crash."""
    results = []
    for row_id, name, check in rows:
        try:
            ok, detail = check()
        except Exception as error:
            ok, detail = False, f"{type(error).__name__}: {error}"
        results.append((row_id, name, ok, detail))
    return results


def report(results: list[tuple[str, str, bool, str]], label: str) -> int:
    for row_id, name, ok, detail in results:
        print(f"  {'ok  ' if ok else 'FAIL'} {row_id} {name} — {detail}")
    passed = sum(ok for _, _, ok, _ in results)
    print(f"\nupdater_check {label}: {passed}/{len(results)}")
    return 0 if passed == len(results) else 1


def release_failures(bundle: str | os.PathLike) -> list[str]:
    """R1–R5 over a Release bundle: one `<id> <name>: <detail>` per failing row (the gate's launch)."""
    return [f"{row_id} {name}: {detail}"
            for row_id, name, ok, detail in evaluate(release_rows(Path(bundle))) if not ok]


# MARK: - mutants

# (case, the side it copies and mutates, the rows that must fail), declared before any run.
MUTANTS = (
    ("release-feed", "release", {"R3"}),
    ("sparkle-load-release", "release", {"R2", "D6"}),
    ("feed-empty-dist", "dist", {"D3"}),
    ("key-short-dist", "dist", {"D4"}),
    ("helper-no-runtime-dist", "dist", {"D2"}),
    ("sparkle-unsigned-dist", "dist", {"D1", "D7"}),
    ("sparkle-missing-dist", "dist", {"D1", "D2", "D7"}),
    ("ack-missing-release", "release", {"D8"}),
)


def reseal(bundle: Path) -> None:
    """Seal an edited copy again as the build seals it: ad hoc, the runtime, no `--deep`."""
    sign(bundle, "-o", "runtime")


def mutate(case: str, copy: Path) -> None:
    """Apply the case's mutation to the copy; RuntimeError when a step fails."""
    if case == "release-feed":
        edit_info(copy, {"SUFeedURL": FEED_URL})
        reseal(copy)
    elif case == "sparkle-load-release":
        binary = executable(copy)
        paths, reason = load_commands(binary)
        if paths is None:
            raise RuntimeError(f"the copy's load commands: {reason}")
        room = [path for path in paths if len(path) >= len(SPARKLE_LOAD) and SPARKLE_IN_PATH not in path]
        if not room:
            raise RuntimeError(f"no load command in {binary} as long as {SPARKLE_LOAD}")
        argv = ["install_name_tool", "-change", room[0], SPARKLE_LOAD, str(binary)]
        done, reason = run_tool(argv)
        if done is None:
            raise RuntimeError(reason)
        if done.returncode != 0:
            raise RuntimeError(tool_failure(done, argv))
        reseal(copy)
    elif case == "feed-empty-dist":
        edit_info(copy, {"SUFeedURL": ""})
        reseal(copy)
    elif case == "key-short-dist":
        edit_info(copy, {"SUPublicEDKey": base64.b64encode(b"\1" * 31).decode("ascii")})
        reseal(copy)
    elif case == "helper-no-runtime-dist":
        framework = copy / FRAMEWORK
        helpers = helper_paths(framework)
        if not helpers:
            raise RuntimeError(f"no helper under {framework}")
        sign(helpers[0])
        sign(framework / "Versions/B", "-o", "runtime")
        reseal(copy)
    elif case == "sparkle-unsigned-dist":
        binary = Path(os.path.realpath(copy / FRAMEWORK_BINARY))
        argv = ["codesign", "--remove-signature", str(binary)]
        done, reason = run_tool(argv)
        if done is None:
            raise RuntimeError(reason)
        if done.returncode != 0:
            raise RuntimeError(tool_failure(done, argv))
    elif case == "sparkle-missing-dist":
        shutil.rmtree(copy / FRAMEWORK)
    elif case == "ack-missing-release":
        path = copy / ACKNOWLEDGEMENTS
        text = path.read_text(encoding="utf-8")
        heading = ACK_HEADING.search(text)
        if heading is None:
            raise RuntimeError(f"no Sparkle {SPARKLE_VERSION} heading in {path}")
        path.write_text(text[:heading.start()].rstrip("\n") + "\n", encoding="utf-8")
        reseal(copy)
    else:
        raise RuntimeError(f"unknown case {case}")


def mutants(release: Path, dist: Path) -> int:
    print("designs/sparkle-updates.md — updater_check mutants: each row made to fail on purpose\n")
    passed = 0
    for case, side, expected in MUTANTS:
        scratch = Path(tempfile.mkdtemp(prefix="dr-updater-mutant-"))
        try:
            copy = scratch / "Doctored.app"
            try:
                copy_bundle(release if side == "release" else dist, copy)
                mutate(case, copy)
            except (RuntimeError, OSError) as error:
                print(f"  FAIL {case} — setup: {error}")
                continue
            rows = dist_rows(copy if side == "release" else release,
                             copy if side == "dist" else dist, smoke=False)
            failed = {row_id for row_id, _, ok, _ in evaluate(rows) if not ok}
            ok = failed == expected
            passed += ok
            print(f"  {'ok  ' if ok else 'FAIL'} {case} — failed {sorted(failed)}, expected {sorted(expected)}")
        finally:
            shutil.rmtree(scratch, ignore_errors=True)
    print(f"\nupdater_check mutants: {passed}/{len(MUTANTS)}")
    return 0 if passed == len(MUTANTS) else 1


# MARK: - selftest

# project.yml: a `NAME: value` line, the key a build-setting name (with any `[cond=…]` suffix).
SETTING_LINE = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*(?:\[[^\]]*\])*):(?:\s+(.*))?$")
REFERENCE = re.compile(r"\$\([^)]*\)|\$\{[^}]*\}")
# DR_UPDATER as its own word, or straight after `-D` (`OTHER_SWIFT_FLAGS: -DDR_UPDATER`).
UPDATER_TOKEN = re.compile(r"(?:(?<![A-Za-z0-9_])|(?<=-D))DR_UPDATER(?![A-Za-z0-9_])")


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _meaningful(lines: list[str]) -> list[str]:
    """The lines that are neither blank nor comments."""
    return [line.rstrip() for line in lines if line.strip() and not line.lstrip().startswith("#")]


def _scalar(value: str) -> str:
    """A YAML scalar's text: quotes removed, or a trailing ` # comment` dropped."""
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        return value[1:-1]
    return re.split(r"\s+#", value, maxsplit=1)[0].strip()


def _children(lines: list[str]) -> list[tuple[str, list[str]]]:
    """The block's top-level lines (at its first line's indent), each with the lines under it."""
    if not lines:
        return []
    level = _indent(lines[0])
    children: list[tuple[str, list[str]]] = []
    for line in lines:
        if _indent(line) <= level and not line.lstrip().startswith("- "):
            children.append((line.strip(), []))
        elif children:
            children[-1][1].append(line)
    return children


def _settings(lines: list[str]) -> list[tuple[str, str]]:
    """The (NAME, value) setting lines of a block; a list value's items joined by spaces."""
    found = []
    for head, under in _children(lines):
        match = SETTING_LINE.match(head)
        if match is None:
            continue
        value = _scalar(match.group(2) or "")
        if not value:
            value = " ".join(_scalar(line.strip()[2:]) for line in under if line.strip().startswith("- "))
        found.append((match.group(1), value))
    return found


def settings_entries(text: str) -> list[tuple[str, list[tuple[str, str]]]]:
    """Every `settings:` block's entries: ("base", settings) and ("configs:<Name>", settings).
    Only these count as setting a build setting; comments and other blocks (`info:`) do not.
    Settings written straight under `settings:` count as `base`."""
    lines = text.splitlines()
    entries = []
    for index, line in enumerate(lines):
        if line.strip() != "settings:" or line.lstrip().startswith("#"):
            continue
        level = _indent(line)
        block = []
        for following in lines[index + 1:]:
            if following.strip() and not following.lstrip().startswith("#") and _indent(following) <= level:
                break
            block.append(following)
        block = _meaningful(block)
        direct = []
        for head, under in _children(block):
            if head == "base:":
                entries.append(("base", _settings(under)))
            elif head == "configs:":
                for name, config in _children(under):
                    entries.append((f"configs:{name.rstrip(':').strip()}", _settings(config)))
            elif head == "groups:":
                continue
            else:
                direct.append(head)
                direct.extend(under)
        if direct:
            entries.append(("base", _settings(direct)))
    return entries


def sparkle_exact_versions(text: str) -> list[str]:
    """The `exactVersion` values under the top-level `packages:` block's `Sparkle:` entry."""
    lines = text.splitlines()
    versions = []
    for index, line in enumerate(lines):
        if line.rstrip() != "packages:":
            continue
        block = []
        for following in lines[index + 1:]:
            if following.strip() and not following.lstrip().startswith("#") and _indent(following) == 0:
                break
            block.append(following)
        for name, under in _children(_meaningful(block)):
            if name.rstrip(":").strip() != "Sparkle":
                continue
            versions.extend(value for key, value in _settings(under) if key == "exactVersion")
    return versions


def project_problems(text: str) -> list[str]:
    """Why project.yml's settings would put the updater anywhere but Distribution (empty: none)."""
    problems = []
    dist = [settings for name, settings in settings_entries(text) if name == "configs:Distribution"]
    others = [(name, settings) for name, settings in settings_entries(text) if name != "configs:Distribution"]
    dist_settings = [pair for settings in dist for pair in settings]
    feeds = [value for key, value in dist_settings if key == "DR_SPARKLE_FEED_URL"]
    keys = [value for key, value in dist_settings if key == "DR_SPARKLE_PUBLIC_KEY"]
    conditions = [REFERENCE.sub(" ", value) for key, value in dist_settings
                  if key == "SWIFT_ACTIVE_COMPILATION_CONDITIONS"]
    if not dist:
        problems.append("no configs: Distribution: entry in a settings: block")
    if feeds != [FEED_URL]:
        problems.append(f"Distribution sets DR_SPARKLE_FEED_URL to {feeds}, want [{FEED_URL!r}]")
    if keys != [PUBLIC_KEY]:
        problems.append(f"Distribution sets DR_SPARKLE_PUBLIC_KEY to {keys}, want [{PUBLIC_KEY!r}]")
    if not any(UPDATER_TOKEN.search(value) for value in conditions):
        problems.append("Distribution's SWIFT_ACTIVE_COMPILATION_CONDITIONS lacks DR_UPDATER")
    for name, settings in others:
        for key, value in settings:
            if key in ("DR_SPARKLE_FEED_URL", "DR_SPARKLE_PUBLIC_KEY"):
                problems.append(f"{name} sets {key}")
            if UPDATER_TOKEN.search(REFERENCE.sub(" ", value)):
                problems.append(f"{name}'s {key} names DR_UPDATER")
    versions = sparkle_exact_versions(text)
    if versions != [SPARKLE_VERSION]:
        problems.append(f"packages: Sparkle: exactVersion is {versions}, want [{SPARKLE_VERSION!r}]")
    return problems


def _insert_after(text: str, pattern: str, line_text: str) -> str:
    """The text with `line_text` (indented two past the matched line) after the one line that
    matches `pattern`; ValueError unless exactly one does."""
    lines = text.splitlines(keepends=True)
    found = [index for index, line in enumerate(lines) if re.fullmatch(pattern, line.rstrip("\n"))]
    if len(found) != 1:
        raise ValueError(f"{len(found)} lines match {pattern!r}")
    index = found[0]
    lines.insert(index + 1, " " * (_indent(lines[index]) + 2) + line_text + "\n")
    return "".join(lines)


def t1(_folder: Path) -> Result:
    text = PROJECT_YML.read_text(encoding="utf-8")
    problems = project_problems(text)
    if problems:
        return False, f"project.yml: {'; '.join(problems)}"
    leaks = {
        "DR_UPDATER in Debug's conditions": _insert_after(
            text, r"\s+Debug:\s*", 'SWIFT_ACTIVE_COMPILATION_CONDITIONS: "$(inherited) DR_UPDATER"'),
        "-DDR_UPDATER in Debug's Swift flags": _insert_after(
            text, r"\s+Debug:\s*", 'OTHER_SWIFT_FLAGS: "$(inherited) -DDR_UPDATER"'),
        "DR_SPARKLE_FEED_URL in base:": _insert_after(
            text, r"\s+base:\s*", f"DR_SPARKLE_FEED_URL: {FEED_URL}"),
        "another key in Distribution": text.replace(PUBLIC_KEY, base64.b64encode(bytes(32)).decode("ascii")),
    }
    passed = [name for name, leaked in leaks.items() if leaked == text or not project_problems(leaked)]
    if passed:
        return False, f"synthetic leaks passed: {passed}"
    return True, "project.yml passes; " + ", ".join(leaks) + " each fail"


def t2(_folder: Path) -> Result:
    good = base64.b64encode(bytes(range(32))).decode("ascii")
    cases = {
        "good": (good, True),
        "empty": ("", False),
        "31 bytes": (base64.b64encode(bytes(31)).decode("ascii"), False),
        "33 bytes": (base64.b64encode(bytes(33)).decode("ascii"), False),
        "not base64": ("not base64 at all, not even close!!!!!!!!!!=", False),
        "unpadded": (good.rstrip("="), False),
    }
    wrong = [name for name, (value, want) in cases.items() if is_public_key(value) != want]
    if wrong:
        return False, f"misjudged {wrong}"
    return True, f"{len(cases)} keys judged"


def _bundle_with_info(folder: Path, name: str, info: dict) -> Path:
    bundle = folder / f"{name}.app"
    (bundle / "Contents").mkdir(parents=True)
    with (bundle / INFO_PLIST).open("wb") as handle:
        plistlib.dump(info, handle)
    return bundle


def t3(folder: Path) -> Result:
    key = PUBLIC_KEY
    other = base64.b64encode(bytes(range(32))).decode("ascii")
    short = base64.b64encode(bytes(31)).decode("ascii")
    # (Info.plist, want R3, want D3, want D4)
    cases = {
        "release": ({"SUFeedURL": "", "SUPublicEDKey": ""}, True, False, False),
        "distribution": ({"SUFeedURL": FEED_URL, "SUPublicEDKey": key}, False, True, True),
        "neither key": ({}, False, False, False),
        "release with a feed": ({"SUFeedURL": FEED_URL, "SUPublicEDKey": ""}, False, True, False),
        "release with a key": ({"SUFeedURL": "", "SUPublicEDKey": key}, False, False, True),
        "an http feed": ({"SUFeedURL": FEED_URL.replace("https://", "http://"), "SUPublicEDKey": key},
                         False, False, True),
        "a short key": ({"SUFeedURL": FEED_URL, "SUPublicEDKey": short}, False, True, False),
        "another 32-byte key": ({"SUFeedURL": FEED_URL, "SUPublicEDKey": other}, False, True, False),
    }
    wrong = []
    for index, (name, (info, want_r3, want_d3, want_d4)) in enumerate(cases.items()):
        bundle = _bundle_with_info(folder, f"case{index}", info)
        got = (with_info(bundle, r3_keys)[0], with_info(bundle, d3_feed)[0], with_info(bundle, d4_key)[0])
        if got != (want_r3, want_d3, want_d4):
            wrong.append(f"{name}: R3/D3/D4 {got}, want {(want_r3, want_d3, want_d4)}")
    if wrong:
        return False, "; ".join(wrong)
    return True, f"{len(cases)} synthetic Info.plists judged"


CANNED_RELEASE = (
    "/x/Doctored.app/Contents/MacOS/Doctored:\n"
    "\t/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation "
    "(compatibility version 300.0.0, current version 4201.0.0)\n"
    "\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1356.0.0)\n"
    "\t/usr/lib/swift/libswiftCoreImage.dylib (compatibility version 1.0.0, current version 2.2.0, weak)\n")
CANNED_SPARKLE = f"\t{SPARKLE_LOAD} (compatibility version 1.6.0, current version 2.10.1)\n"
CANNED_SPARKLE_A = ("\t@rpath/Sparkle.framework/Versions/A/Sparkle "
                    "(compatibility version 1.6.0, current version 2.10.1)\n")


def t4(_folder: Path) -> Result:
    release = parse_otool(CANNED_RELEASE)
    if len(release) != 3 or release[2] != "/usr/lib/swift/libswiftCoreImage.dylib":
        return False, f"parse_otool read {release}"
    dist_text = CANNED_RELEASE + CANNED_SPARKLE
    d6_cases = {
        "Release plus Sparkle": (dist_text, True),
        "no Sparkle": (CANNED_RELEASE, False),
        "Sparkle twice": (dist_text + CANNED_SPARKLE, False),
        "another library added": (dist_text + "\t/usr/lib/libz.1.dylib (compatibility version 1.0.0)\n", False),
        "a Release library dropped": (dist_text.replace(
            "\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1356.0.0)\n", ""), False),
        "another Sparkle path": (CANNED_RELEASE + CANNED_SPARKLE_A, False),
    }
    p2_cases = {
        "one, the expected path": (dist_text, True),
        "none": (CANNED_RELEASE, False),
        "another version folder": (CANNED_RELEASE + CANNED_SPARKLE_A, False),
        "two": (dist_text + CANNED_SPARKLE, False),
    }
    wrong = [f"D6 {name}" for name, (text, want) in d6_cases.items()
             if compare_loads(release, parse_otool(text))[0] != want]
    wrong += [f"P2 {name}" for name, (text, want) in p2_cases.items()
              if one_sparkle_load(parse_otool(text))[0] != want]
    if wrong:
        return False, f"misjudged {wrong}"
    return True, f"{len(d6_cases)} D6 and {len(p2_cases)} P2 listings judged"


def selftest() -> int:
    print("designs/sparkle-updates.md — updater_check's selftest: the rows' own logic, no build\n")
    cases = (("T1 project.yml sets the updater in Distribution alone", t1),
             ("T2 the public-key validator", t2),
             ("T3 R3, D3 and D4 over synthetic Info.plists", t3),
             ("T4 D6 and P2 over canned otool -L text", t4))
    passed = 0
    for name, case in cases:
        folder = Path(tempfile.mkdtemp(prefix="updater-check-selftest-"))
        try:
            ok, detail = case(folder)
        except Exception as error:  # a broken case is a scored failure, not a crash
            ok, detail = False, f"{type(error).__name__}: {error}"
        finally:
            shutil.rmtree(folder, ignore_errors=True)
        passed += ok
        print(f"  {'ok  ' if ok else 'FAIL'} {name} — {detail}")
    print(f"\nupdater_check selftest: {passed}/{len(cases)}")
    return 0 if passed == len(cases) else 1


# MARK: - main

def main(argv: list[str]) -> int:
    if argv == ["--selftest"]:
        return selftest()
    parser = argparse.ArgumentParser(prog="updater_check.py",
                                     description="Sparkle is loaded by Distribution alone "
                                                 "(designs/sparkle-updates.md); --selftest checks "
                                                 "the rows' own logic with no build.")
    commands = parser.add_subparsers(dest="command", required=True)
    for name, about in (("dist", "D1–D9 over Distribution and R1–R5 over Release (make dist)"),
                        ("mutants", "each row made to fail on purpose, on scratch copies (make dist)")):
        command = commands.add_parser(name, help=about)
        command.add_argument("--release", type=Path, required=True, help="the Release Doctored.app")
        command.add_argument("--dist", type=Path, required=True, help="the Distribution Doctored.app")
    command = commands.add_parser("published", help="the exported, Developer ID signed app (publish.yml)")
    command.add_argument("--app", type=Path, required=True, help="the exported Doctored.app")
    command.add_argument("--version", required=True, help="the release's CFBundleShortVersionString")
    command.add_argument("--build", required=True, help="the release's CFBundleVersion, the run number")
    arguments = parser.parse_args(argv)
    if arguments.command == "dist":
        print("designs/sparkle-updates.md — updater_check dist: Distribution loads Sparkle, Release "
              "does not\n")
        return report(evaluate(dist_rows(arguments.release, arguments.dist, smoke=True)), "dist")
    if arguments.command == "mutants":
        return mutants(arguments.release, arguments.dist)
    print("designs/sparkle-updates.md — updater_check published: the exported app's updater\n")
    return report(evaluate(published_rows(arguments.app, arguments.version, arguments.build)), "published")


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
