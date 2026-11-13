#!/usr/bin/env python3
"""Build an app-hosted probe once per source state, and type-check a probe directory.

`build` makes a probe in two compiles. The first is the app part: the app sources compiled once at
`-Onone` as the testable Swift module `Doctored` (`-enable-testing`) with a static archive of its
objects, kept in `build/probe-cache/app-<key>/` and shared by every probe built from the same app
sources. The second compiles the probe's own files as the module `DoctoredProbe`, which imports the
app part with internal access (`-testable-import-module`, so no probe file carries an `import` line)
and links its archive whole (`-force_load`: a plain archive link drops every app object the probe
never names by symbol, such as a class found only at run time). The linked probe is
`build/probe-cache/<key>/probe`, copied to the caller's path. Each key covers every input of its
compile, so a second call with the same inputs compiles nothing and a probe edit recompiles only the
probe. A build at any other optimization (the `-O` runners, whose measurements cross-module
inlining would change) stays one compile of the app sources with the probe's as the module
`Doctored`, and makes no app part.

The app part is compiled once per key. A build that does not find it takes the key's lock file
(`app-<key>.lock`) exclusively to compile it, then holds the lock shared while its probe compiles
and links; another build of the same key waits for the shared lock and reuses the part. Its key
names each input by basename and contents, not by path, so a runner's snapshot copy of the sources
or a `build/` copied from another checkout reuses it, and a path the part embeds (`fatalError`'s
`#file`) names wherever it was first compiled. `MACOSX_DEPLOYMENT_TARGET` counts only when it
differs from `TARGET`'s version, which the command passes explicitly. A probe set's `defines` reach
only the probe's compile, and the core library, `Info.plist` and `objects` only its link. The
`KEEP_APP_ENTRIES` newest app parts are kept; `_prune` removes an older one only while no build holds
its lock. The Clang module cache is `build/probe-module-cache`, shared by every
build and type check, and made again when it was made at another path (a worktree's `build/` copied
from another checkout; `remake_if_moved`, which `scripts/agent_module_cache.py` also uses).

`python3 scripts/probe_build.py --typecheck scripts/<probe dir>` type-checks that directory's probe
set in the shape its runner builds: against the app part (compiled first when no build has it, and
then reused by the runner's build), or, for a set whose runner builds one module
(`ProbeSet.optimization`), with the app sources in batch mode. It links no probe and stops before
SILGen.

With `DR_PROBE_BUILD_ONLY=1` in the environment, `build` ends the process with
status 0 right after it delivers the probe and prints `build only: <output>`, so
a runner's build is checked without launching anything it would run next. A
runner that removes its scratch folder takes the variable first
(`take_build_only`) and stops after the build on its own.

Standard library only; imports nothing from `scripts/`.
"""

from __future__ import annotations

import argparse
import contextlib
import fcntl
import hashlib
import json
import os
import re
import secrets
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Callable, Iterable, Iterator, Mapping, NamedTuple, Sequence, TextIO

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / "build/probe-cache"
MODULE_CACHE = ROOT / "build/probe-module-cache"
# The file in MODULE_CACHE that names the path the cache was made at (`_module_cache`).
MODULE_CACHE_PATH_FILE = "made-at"
# `make gate` builds each of its probe runs (`PROBE_RUNS` in `scripts/gate.py`) and later reuses that build, so the
# cache must hold all of them beside a few hand runs; with fewer entries than gate probes, each build prunes an
# earlier one and every reuse becomes another compile. About 80 MB a linked probe; app parts are counted apart.
KEEP_ENTRIES = 16
# App parts (`app-<key>`, about 105 MB each): the current app sources' and two earlier states', so going back to a
# recent state of the sources reuses its compile.
KEEP_APP_ENTRIES = 3
TARGET = "arm64-apple-macos15.0"
BRIDGING_HEADER = "app/Bridging/Doctored-Bridging-Header.h"
KEY_ENVIRONMENT = ("MACOSX_DEPLOYMENT_TARGET", "SDKROOT", "TOOLCHAINS", "DEVELOPER_DIR")
QUOTED_INCLUDE = re.compile(rb'^[ \t]*#[ \t]*(?:include|import)[ \t]+"([^"\n]+)"', re.MULTILINE)
QUERY_TIMEOUT_S = 30
BUILD_ONLY_ENV = "DR_PROBE_BUILD_ONLY"
APP_MODULE = "Doctored"
PROBE_MODULE = "DoctoredProbe"
APP_ARCHIVE = "libDoctoredApp.a"
APP_PREFIX = "app-"
# How often a build that waits for an app part's lock tries it again.
LOCK_POLL_S = 0.2
# The type check's bound on waiting for and compiling the app part; the check that follows has none.
TYPECHECK_APP_PART_TIMEOUT_S = 1800


class ProbeBuildError(RuntimeError):
    """A probe build that failed for a reason other than the compiler's exit status or its timeout."""


class ProbeSet(NamedTuple):
    """The Swift files one runner compiles against the app sources."""
    files: tuple[str, ...] | None       # relative to scripts/, in command-line order; None = the directory's *.swift
    exclude: tuple[str, ...] = ()       # file names left out when files is None
    defines: tuple[str, ...] = ()
    # The optimization the runner builds with; any but -Onone is one module with the app sources (`build`), and
    # `--typecheck` checks that shape.
    optimization: str = "-Onone"


class Built(NamedTuple):
    key: str                # 16 hex characters
    reused: bool
    entry: Path             # CACHE / key
    app_key: str | None     # the app part's key; None for a one-module build


def _listed(directory: str, *names: str) -> tuple[str, ...]:
    return tuple(f"{directory}/{name}" for name in names)


# Key = directory name under scripts/. Each set is what its runner compiles against the app sources.
PROBE_SETS: dict[str, ProbeSet] = {
    "artwork-text-probe": ProbeSet(_listed("artwork-text-probe", "main.swift", "host.swift", "comparison.swift")),
    "artwork-workflow-probe": ProbeSet(
        (*_listed("artwork-text-probe", "main.swift", "host.swift", "comparison.swift"),
         "artwork-workflow-probe/ui.swift"),
        defines=("ARTWORK_WORKFLOW_UI",)),
    "color-metadata-probe": ProbeSet(None),
    "content-aware-probe": ProbeSet(_listed("content-aware-probe", "main.swift", "patch.swift")),
    "data-migration-probe": ProbeSet(_listed("data-migration-probe", "main.swift")),
    "history-probe": ProbeSet(None),
    "jobs-probe": ProbeSet(None),
    "model-fixtures": ProbeSet(None),
    "native-paint-probe": ProbeSet(None),
    "native-selection-probe": ProbeSet(
        _listed("native-selection-probe", "main.swift", "auxiliary.swift", "selection.swift", "statistics.swift")),
    "path-guard-probe": ProbeSet(None),
    "precision-probe": ProbeSet(None),
    # Its runner (`measure.py`) measures memory, so it builds -O as one module.
    "raw-decode-probe": ProbeSet(_listed("raw-decode-probe", "main.swift"), optimization="-O"),
    "raw-local-probe": ProbeSet(None, exclude=("reference.swift", "oracle.swift", "geometry-reference.swift")),
    "raw-probe": ProbeSet(_listed("raw-probe", "main.swift")),
    "raw-workflow-probe": ProbeSet(_listed("raw-workflow-probe", "main.swift", "develop_ui.swift")),
    # Its runner (`run.py`) builds -O as one module.
    "reach-probe": ProbeSet(_listed("reach-probe", "main.swift"), optimization="-O"),
    "retouch-probe": ProbeSet(_listed("retouch-probe", "main.swift")),
    "save-preservation-probe": ProbeSet(_listed("save-preservation-probe", "main.swift")),
    "screens-probe": ProbeSet(None),
    "selected-tone-probe": ProbeSet(
        _listed("selected-tone-probe", "main.swift", "info.swift", "adjustment_dialogs.swift")),
    "selection-history-probe": ProbeSet(None),
    "selection-probe": ProbeSet(None),
    "updater-probe": ProbeSet(_listed("updater-probe", "main.swift")),
}

# Directories under scripts/ whose Swift is never compiled with the app.
STANDALONE_DIRS: tuple[str, ...] = ("auxiliary-matte-probe", "hdr-photos", "host-still-admission",
                                    "host-stills-opaque", "people-instances-oracle")


def app_sources(root: Path = ROOT) -> list[Path]:
    """Every app source but `main.swift`, sorted."""
    return sorted(path for path in (Path(root) / "app/Sources").glob("*.swift") if path.name != "main.swift")


def probe_sources(name: str, root: Path = ROOT) -> list[Path]:
    """The files of probe set `name` under `root/scripts/`, in command-line order."""
    probe_set = PROBE_SETS[name]
    scripts = Path(root) / "scripts"
    if probe_set.files is None:
        return sorted(path for path in (scripts / name).glob("*.swift") if path.name not in probe_set.exclude)
    return [scripts / relative for relative in probe_set.files]


def _label(path: Path) -> str:
    resolved = Path(path).resolve()
    root = ROOT.resolve()
    if resolved.is_relative_to(root):
        return "root:" + resolved.relative_to(root).as_posix()
    return "ext:" + resolved.name


def _shown(path: Path) -> str:
    """`path` relative to ROOT when it is inside it, so a type check's diagnostics stay short."""
    resolved = Path(path).resolve()
    return str(resolved.relative_to(ROOT.resolve())) if resolved.is_relative_to(ROOT.resolve()) else str(path)


def _digest(path: Path) -> str:
    with open(path, "rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def _included(header: Path) -> list[Path]:
    """Every file `header` reaches through quoted `#include` / `#import` lines, each once."""
    found: list[Path] = []
    seen = {Path(header).resolve()}
    queue = [Path(header)]
    while queue:
        current = queue.pop(0)
        for match in QUOTED_INCLUDE.finditer(current.read_bytes()):
            name = os.fsdecode(match.group(1))
            target = current.parent / name
            if not target.is_file():
                raise ProbeBuildError(f'{current}: "{name}" does not resolve relative to the including file')
            if target.resolve() not in seen:
                seen.add(target.resolve())
                found.append(target)
                queue.append(target)
    return found


def _query(argv: list[str], env: Mapping[str, str]) -> str:
    done = subprocess.run(argv, cwd=ROOT, env=dict(env), stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, text=True, errors="replace", timeout=QUERY_TIMEOUT_S)
    if done.returncode != 0:
        raise ProbeBuildError(f"{shlex.join(argv)} exited with status {done.returncode}: {done.stdout.strip()}")
    return done.stdout


def _sdk(env: Mapping[str, str]) -> str:
    return _query(["xcrun", "--show-sdk-path", "--sdk", "macosx"], env).strip()


def remake_if_moved(marker: Path, here: Path, caches: Iterable[Path], lock: Path) -> str | None:
    """Removes `caches` when `marker` does not name `here`, then writes `here`'s path to it.

    Clang's module cache records its own absolute path, so a copy of it elsewhere (a worktree's
    `build/` copied from another checkout) fails every compile with "PCH was compiled with module
    cache path …". `marker` names the path its folder was made at; when it is missing or names
    another path, each of `caches` is removed, for its compiler to make again, and `marker` is
    written. The check holds `lock` exclusively, so a build that starts meanwhile finds the new
    marker and keeps the caches; `caches` is read under it, so a generator sees the folder as it is
    then. The one rule for every module cache the tools keep: `_module_cache` (the probe builds')
    and `scripts/agent_module_cache.py` (the assistant helper's SwiftPM scratch path).

    Returns None when the caches were kept, else what `marker` held before ("" when it was absent).
    """
    lock.parent.mkdir(parents=True, exist_ok=True)
    expected = str(here.resolve())
    with open(lock, "a") as held:
        fcntl.flock(held, fcntl.LOCK_EX)
        try:
            made_at = marker.read_text()
        except OSError:
            made_at = ""
        if made_at == expected:
            return None
        for cache in list(caches):
            shutil.rmtree(cache, ignore_errors=True)
        marker.parent.mkdir(parents=True, exist_ok=True)
        marker.write_text(expected)
    return made_at


def _module_cache() -> Path:
    """`MODULE_CACHE`, made again first when it was made at another path (`remake_if_moved`).

    The cache's `MODULE_CACHE_PATH_FILE` names the path it was made at, and its lock lies beside it.
    """
    remake_if_moved(MODULE_CACHE / MODULE_CACHE_PATH_FILE, MODULE_CACHE, [MODULE_CACHE],
                    MODULE_CACHE.with_name(MODULE_CACHE.name + ".lock"))
    return MODULE_CACHE


def _library_arguments(library: str, directory: str, link_library: str) -> list[str]:
    return ["-L", directory, "-ldoctored_core"] if link_library == "search" else [library]


def case_clashes(sources: Sequence[Path]) -> list[str]:
    """The basenames, case-folded, that more than one of `sources` shares, sorted."""
    counts: dict[str, int] = {}
    for path in sources:
        name = Path(path).name.casefold()
        counts[name] = counts.get(name, 0) + 1
    return sorted(name for name, count in counts.items() if count > 1)


def _threads(threaded: bool) -> list[str]:
    return ["-num-threads", str(os.cpu_count() or 1)] if threaded else []


def _info_plist_arguments(info_plist: str | None) -> list[str]:
    if info_plist is None:
        return []
    return ["-Xlinker", "-sectcreate", "-Xlinker", "__TEXT", "-Xlinker", "__info_plist", "-Xlinker", info_plist]


def _import_arguments(app_entry: str) -> list[str]:
    """What gives a compile the app part in `app_entry` with internal access, as if each of its files began
    `@testable import Doctored`."""
    return ["-I", app_entry, "-Xfrontend", "-testable-import-module", "-Xfrontend", APP_MODULE]


def _arguments(*, optimization: str, defines: Sequence[str], sdk: str, module_cache: str, bridging_header: str,
               sources: Sequence[str], library: Sequence[str], objects: Sequence[str], info_plist: str | None,
               output: str, threaded: bool) -> list[str]:
    """A one-module build: the app sources with the probe's, as the module `Doctored`."""
    argv = ["xcrun", "swiftc", optimization, "-whole-module-optimization"]
    if optimization == "-Onone":
        argv += _threads(threaded)
    argv += ["-module-name", APP_MODULE]
    for define in defines:
        argv += ["-D", define]
    argv += ["-sdk", sdk, "-target", TARGET, "-module-cache-path", module_cache,
             "-import-objc-header", bridging_header, *sources, *library, *objects]
    return [*argv, *_info_plist_arguments(info_plist), "-o", output]


def _app_arguments(*, sdk: str, module_cache: str, bridging_header: str, sources: Sequence[str], output: str,
                   threaded: bool) -> list[str]:
    """The app part's compile: `output`/Doctored.swiftmodule and `output`/APP_ARCHIVE."""
    return ["xcrun", "swiftc", "-Onone", "-whole-module-optimization", *_threads(threaded),
            "-module-name", APP_MODULE, "-enable-testing", "-parse-as-library",
            "-emit-module", "-emit-module-path", f"{output}/{APP_MODULE}.swiftmodule", "-emit-library", "-static",
            "-sdk", sdk, "-target", TARGET, "-module-cache-path", module_cache,
            "-import-objc-header", bridging_header, *sources, "-o", f"{output}/{APP_ARCHIVE}"]


def _probe_arguments(*, defines: Sequence[str], sdk: str, module_cache: str, bridging_header: str, app_entry: str,
                     sources: Sequence[str], library: Sequence[str], objects: Sequence[str], info_plist: str | None,
                     output: str, threaded: bool) -> list[str]:
    """The probe's compile against the app part in `app_entry`, linked with its whole archive."""
    argv = ["xcrun", "swiftc", "-Onone", "-whole-module-optimization", *_threads(threaded),
            "-module-name", PROBE_MODULE]
    for define in defines:
        argv += ["-D", define]
    argv += [*_import_arguments(app_entry), "-sdk", sdk, "-target", TARGET, "-module-cache-path", module_cache,
             "-import-objc-header", bridging_header, *sources,
             "-Xlinker", "-force_load", "-Xlinker", f"{app_entry}/{APP_ARCHIVE}", *library, *objects]
    return [*argv, *_info_plist_arguments(info_plist), "-o", output]


def _environment(env: Mapping[str, str], *, app_part: bool) -> dict[str, str | None]:
    recorded = {name: env.get(name) for name in KEY_ENVIRONMENT}
    if app_part and recorded["MACOSX_DEPLOYMENT_TARGET"] == TARGET.rpartition("macos")[2]:
        # The command passes -target, so this value changes nothing; seven runners set it and the rest do not.
        recorded["MACOSX_DEPLOYMENT_TARGET"] = None
    return recorded


def _tools(env: Mapping[str, str], sdk: str) -> dict:
    return {"helper_sha256": _digest(Path(__file__)),
            "toolchain": _query(["xcrun", "swiftc", "--version"], env),
            "sdk": sdk,
            "sdk_settings_sha256": _digest(Path(sdk) / "SDKSettings.json")}


def _hashed(manifest: dict) -> str:
    return hashlib.sha256(json.dumps(manifest, sort_keys=True).encode()).hexdigest()[:16]


class _AppInputs(NamedTuple):
    """What the app part is compiled from, with its key and the manifest that explains it."""
    key: str
    manifest: dict
    sources: list[Path]
    bridging_header: Path
    env: Mapping[str, str]
    sdk: str


def _describe_app(*, app_sources: Sequence[Path], bridging_header: Path, env: Mapping[str, str],
                  sdk: str) -> _AppInputs:
    sources = [Path(path) for path in app_sources]
    header = Path(bridging_header)
    arguments = _app_arguments(sdk=sdk, module_cache="module-cache", bridging_header=header.name,
                               sources=[path.name for path in sources], output="entry",
                               threaded=not case_clashes(sources))
    manifest = {
        **_tools(env, sdk),
        "arguments": arguments,
        # Basenames and contents, never paths: the same sources anywhere are the same app part.
        "inputs": [[path.name, _digest(path)] for path in (*sources, header, *_included(header))],
        "environment": _environment(env, app_part=True),
    }
    return _AppInputs(_hashed(manifest), manifest, sources, header, env, sdk)


def _describe(*, app_sources: Sequence[Path], sources: Sequence[Path], bridging_header: Path, library: Path,
              env: Mapping[str, str], info_plist: Path | None, optimization: str, defines: Sequence[str],
              link_library: str, objects: Sequence[Path]) -> tuple[str, dict, str, _AppInputs | None]:
    """The key, the manifest that explains it, the SDK path, and the app part (None for a one-module build)."""
    if link_library not in ("search", "file"):
        raise ProbeBuildError(f'link_library is "{link_library}"; it is "search" or "file"')
    library = Path(library)
    if link_library == "search" and library.name != "libdoctored_core.a":
        raise ProbeBuildError(f'{library}: link_library="search" needs a file named libdoctored_core.a')
    if link_library == "search":
        # The linker takes either of these from the -L directory ahead of the archive, and neither is keyed.
        for name in ("libdoctored_core.dylib", "libdoctored_core.tbd"):
            if (library.parent / name).exists():
                raise ProbeBuildError(f'{library.parent / name}: link_library="search" would link this file, '
                                      "not the archive beside it")
    sdk = _sdk(env)
    app = None
    compiled = [*app_sources, *sources]
    if optimization == "-Onone":
        app = _describe_app(app_sources=app_sources, bridging_header=bridging_header, env=env, sdk=sdk)
        compiled = list(sources)
    inputs = [*compiled, bridging_header, *_included(Path(bridging_header)), library,
              *([] if info_plist is None else [info_plist]), *objects]
    shape = dict(defines=defines, sdk=sdk, module_cache="module-cache", bridging_header=_label(bridging_header),
                 sources=[_label(path) for path in compiled],
                 library=_library_arguments(_label(library), "directory-of:" + _label(library), link_library),
                 objects=[_label(path) for path in objects],
                 info_plist=None if info_plist is None else _label(info_plist), output="output",
                 threaded=not case_clashes(compiled))
    if app is None:
        arguments = _arguments(optimization=optimization, **shape)
    else:
        arguments = _probe_arguments(app_entry="app-entry", **shape)
    manifest = {
        # The `root:` labels are relative, and a probe can carry the repo's path (`#filePath`).
        "root": str(ROOT.resolve()),
        **_tools(env, sdk),
        "arguments": arguments,
        "inputs": [[_label(path), _digest(path)] for path in inputs],
        "environment": _environment(env, app_part=False),
    }
    if app is not None:
        manifest["app_key"] = app.key
    return _hashed(manifest), manifest, sdk, app


def cache_key(*, app_sources: Sequence[Path], sources: Sequence[Path], bridging_header: Path, library: Path,
              env: Mapping[str, str], info_plist: Path | None = None, optimization: str = "-Onone",
              defines: Sequence[str] = (), link_library: str = "search", objects: Sequence[Path] = ()) -> str:
    """The cache key of a build with these inputs: 16 hex characters."""
    try:
        return _describe(app_sources=app_sources, sources=sources, bridging_header=bridging_header, library=library,
                         env=env, info_plist=info_plist, optimization=optimization, defines=defines,
                         link_library=link_library, objects=objects)[0]
    except OSError as error:
        raise ProbeBuildError(f"could not read a build input: {error}") from error


def _is_entry(entry: Path) -> bool:
    return (entry / "probe").is_file() and (entry / "manifest.json").is_file()


def _is_app_entry(entry: Path) -> bool:
    return all((entry / name).is_file() for name in (f"{APP_MODULE}.swiftmodule", APP_ARCHIVE, "manifest.json"))


def _append(log: Path, text: str) -> None:
    with open(log, "a") as stream:
        stream.write(text)


def _is_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except OSError:
        return True     # it exists and belongs to another user
    return True


def _is_lock_file(handle: TextIO, path: Path) -> bool:
    """Whether `handle` is still the file at `path`: `_prune` unlinks the lock file of an app part it removes,
    and a lock on the unlinked file guards nothing."""
    try:
        return os.path.samestat(os.fstat(handle.fileno()), os.stat(path))
    except FileNotFoundError:
        return False


def _try_lock(handle: TextIO, operation: int) -> bool:
    """Whether `operation` (`fcntl.LOCK_SH` or `LOCK_EX`) was taken on `handle` without waiting."""
    try:
        fcntl.flock(handle, operation | fcntl.LOCK_NB)
    except BlockingIOError:
        return False
    return True


def _remove_app_part(name: str) -> None:
    """Remove the app part `name` and its lock file, unless a build holds that lock."""
    lock = CACHE / f"{name}.lock"
    try:
        handle = open(lock, "a")
    except OSError:
        return
    with handle:
        if _try_lock(handle, fcntl.LOCK_EX) and _is_lock_file(handle, lock):
            shutil.rmtree(CACHE / name, ignore_errors=True)
            lock.unlink(missing_ok=True)


def _prune() -> None:
    """Keep the `KEEP_ENTRIES` linked probes and the `KEEP_APP_ENTRIES` app parts with the newest
    `manifest.json`; remove the scratch folders of dead builds.

    An older app part, or a lock file with no part, goes only while no build holds its lock: a builder holds
    it exclusively while it compiles the part, and every build holds it shared from finding the part complete
    until its probe has linked against it."""
    probes, apps = [], []
    for child in CACHE.iterdir():
        if not child.is_dir():
            continue
        if child.name.startswith("tmp-"):
            pid = child.name.split("-")[1]
            if pid.isdigit() and int(pid) > 0 and not _is_alive(int(pid)):
                shutil.rmtree(child, ignore_errors=True)
            continue
        try:
            stamp = (child / "manifest.json").stat().st_mtime_ns
        except OSError:
            stamp = 0
        (apps if child.name.startswith(APP_PREFIX) else probes).append((stamp, child.name, child))
    for _, _, child in sorted(probes, reverse=True)[KEEP_ENTRIES:]:
        shutil.rmtree(child, ignore_errors=True)
    newest = sorted(apps, reverse=True)
    kept = {name for _, name, _ in newest[:KEEP_APP_ENTRIES]}
    names = {name for _, name, _ in newest[KEEP_APP_ENTRIES:]}
    names |= {lock.name[:-len(".lock")] for lock in CACHE.glob(f"{APP_PREFIX}*.lock")}
    for name in sorted(names - kept):
        _remove_app_part(name)


class _Deadline(NamedTuple):
    """`build`'s one bound on waiting for the app part's lock and on both compiles."""
    end: float
    seconds: float

    def left(self, what: str) -> float:
        """The seconds left; `subprocess.TimeoutExpired` naming `what` when none are."""
        left = self.end - time.monotonic()
        if left <= 0:
            raise subprocess.TimeoutExpired(what, self.seconds)
        return left


def _compile(argv: list[str], build_log: Path, log: Path, env: Mapping[str, str], deadline: _Deadline) -> None:
    """Run the compiler in its own session; its process group dies with a timeout or an interrupt."""
    timeout = deadline.left(shlex.join(argv))
    try:
        with build_log.open("w") as stream:
            print("$ " + shlex.join(argv), file=stream, flush=True)
            child = subprocess.Popen(argv, cwd=ROOT, env=dict(env, CLANG_MODULE_CACHE_PATH=str(MODULE_CACHE)),
                                     stdin=subprocess.DEVNULL, stdout=stream, stderr=subprocess.STDOUT,
                                     start_new_session=True)
            try:
                status = child.wait(timeout=timeout)
            except BaseException:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait()
                raise
    finally:
        if build_log.is_file():
            _append(log, build_log.read_text(errors="replace"))
    if status:
        raise subprocess.CalledProcessError(status, argv)


def _scratch() -> Path:
    scratch = CACHE / f"tmp-{os.getpid()}-{secrets.token_hex(4)}"
    scratch.mkdir()
    return scratch


def _store_app_part(app: _AppInputs, entry: Path, log: Path, deadline: _Deadline) -> None:
    """Compile the app part in a scratch folder and rename it to `entry`; the caller holds the exclusive lock."""
    scratch = _scratch()
    try:
        argv = _app_arguments(sdk=app.sdk, module_cache=str(_module_cache()), bridging_header=str(app.bridging_header),
                              sources=[str(path) for path in app.sources], output=str(scratch),
                              threaded=not case_clashes(app.sources))
        _compile(argv, scratch / "build.log", log, app.env, deadline)
        if _describe_app(app_sources=app.sources, bridging_header=app.bridging_header, env=app.env,
                         sdk=app.sdk).key != app.key:
            raise ProbeBuildError("a build input changed while the compiler ran; nothing was stored")
        (scratch / "manifest.json").write_text(json.dumps(dict(app.manifest, key=app.key), indent=2) + "\n")
        if entry.exists():
            shutil.rmtree(entry)        # incomplete, or no build would have taken the exclusive lock to compile it
        os.rename(scratch, entry)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def _claim(handle: TextIO, path: Path, entry: Path, deadline: _Deadline, what: str,
           make: Callable[[], None]) -> bool | None:
    """One try of `_app_part` on the lock file `path`, open as `handle`: True when the part was found complete,
    False when `make` stored it (the shared lock held either way), None to start again with the file reopened."""
    while not _try_lock(handle, fcntl.LOCK_SH):     # it waits only while a builder compiles
        deadline.left(what)
        time.sleep(LOCK_POLL_S)
    if not _is_lock_file(handle, path):
        return None
    if _is_app_entry(entry):
        return True
    fcntl.flock(handle, fcntl.LOCK_UN)
    while not _try_lock(handle, fcntl.LOCK_EX):
        if _is_app_entry(entry):
            return None         # another build stored it: take the shared lock again
        deadline.left(what)
        time.sleep(LOCK_POLL_S)
    if not _is_lock_file(handle, path) or _is_app_entry(entry):
        return None
    make()
    # The downgrade drops the exclusive lock before it takes the shared one; the part just stored is the
    # newest, so no `_prune` in that gap removes it.
    fcntl.flock(handle, fcntl.LOCK_SH)
    return False


@contextlib.contextmanager
def _app_part(app: _AppInputs, log: Path, deadline: _Deadline) -> Iterator[Path]:
    """The complete app part's entry, compiled first when no build has stored it, its lock held shared until the
    block ends, so `_prune` cannot remove it from under a compile or link. Prints `built app part <key>` or
    `reused app part <key>` and appends the same to `log`.

    The shared lock is taken before the entry is checked, and waits only while a builder holds the lock
    exclusively to compile. A build that does not find the entry polls for the exclusive lock, going back to
    the shared one as soon as the entry appears, so it never waits behind another probe's link."""
    entry = CACHE / f"{APP_PREFIX}{app.key}"
    path = CACHE / f"{APP_PREFIX}{app.key}.lock"
    what = f"waiting for the app part {app.key} ({path})"
    CACHE.mkdir(parents=True, exist_ok=True)
    while True:
        handle = open(path, "a")
        try:
            found = _claim(handle, path, entry, deadline, what,
                           lambda: _store_app_part(app, entry, log, deadline))
        except BaseException:
            handle.close()
            raise
        if found is not None:
            break
        handle.close()
    with handle:
        if found:
            os.utime(entry / "manifest.json")
        line = f"{'reused' if found else 'built'} app part {app.key}"
        _append(log, f"{line} ({entry})\n")
        print(line, flush=True)
        yield entry


def take_build_only(environment: dict[str, str] | None = None) -> bool:
    """Whether `os.environ` held `DR_PROBE_BUILD_ONLY=1`, once the variable is removed from it and from
    `environment` (the runner's child environment).

    `build` ends the process under that variable, so a runner that removes its scratch folder afterwards calls
    this first, builds, and returns after the build itself when it is true, launching nothing."""
    build_only = os.environ.pop(BUILD_ONLY_ENV, None) == "1"
    if environment is not None:
        environment.pop(BUILD_ONLY_ENV, None)
    return build_only


def build(*, app_sources: Sequence[Path], sources: Sequence[Path], bridging_header: Path, library: Path,
          output: Path, log: Path, env: Mapping[str, str], timeout: float, info_plist: Path | None = None,
          optimization: str = "-Onone", defines: Sequence[str] = (), link_library: str = "search",
          objects: Sequence[Path] = ()) -> Built:
    """Deliver the probe built from these inputs to `output`, compiling only what no cached build has.

    `app_sources` are the app's files and `sources` the probe's own. Prints `reused build <key>` or
    `built <key>`, the latter after `reused app part <key>` or `built app part <key>`. `timeout` bounds the
    wait for another build's app part and both compiles together. A compiler failure raises
    `subprocess.CalledProcessError`, a timeout `subprocess.TimeoutExpired`, anything else `ProbeBuildError`.
    With `DR_PROBE_BUILD_ONLY=1` in `os.environ` it then ends the process with status 0; no caller code runs.
    """
    inputs = dict(app_sources=[Path(path) for path in app_sources], sources=[Path(path) for path in sources],
                  bridging_header=Path(bridging_header), library=Path(library), env=env,
                  info_plist=None if info_plist is None else Path(info_plist), optimization=optimization,
                  defines=tuple(defines), link_library=link_library, objects=[Path(path) for path in objects])
    try:
        built = _build(inputs, Path(output), Path(log), _Deadline(time.monotonic() + timeout, timeout))
    except OSError as error:
        raise ProbeBuildError(f"the probe build failed outside the compiler: {error}") from error
    if os.environ.get(BUILD_ONLY_ENV) == "1":
        line = f"build only: {output}"
        _append(Path(log), line + "\n")
        print(line, flush=True)
        sys.stdout.flush()
        sys.stderr.flush()
        os._exit(0)
    return built


def _build(inputs: dict, output: Path, log: Path, deadline: _Deadline) -> Built:
    key, manifest, sdk, app = _describe(**inputs)
    app_key = None if app is None else app.key
    entry = CACHE / key
    replace = False
    if _is_entry(entry):
        try:
            os.utime(entry / "manifest.json")
            shutil.copy2(entry / "probe", output)
        except OSError:
            replace = True      # a hit that cannot be delivered is a miss; the new build replaces it
        else:
            _append(log, f"reused build {key} ({entry})\n")
            print(f"reused build {key}", flush=True)
            return Built(key, True, entry, app_key)

    CACHE.mkdir(parents=True, exist_ok=True)
    module_cache = _module_cache()
    compiled = inputs["sources"] if app is not None else [*inputs["app_sources"], *inputs["sources"]]
    # Threaded WMO writes one object per source, named after its basename; on a case-insensitive
    # disk two sources whose basenames differ only by case overwrite each other's object and the
    # link fails. Such a build runs on one thread. The app part is a compile of its own.
    clashes = case_clashes(compiled)
    if clashes and inputs["optimization"] == "-Onone":
        print("no -num-threads: sources share the case-folded basename(s) " + ", ".join(clashes), flush=True)
    library = inputs["library"]
    shape = dict(defines=inputs["defines"], sdk=sdk, module_cache=str(module_cache),
                 bridging_header=str(inputs["bridging_header"]), sources=[str(path) for path in compiled],
                 library=_library_arguments(str(library), str(library.parent), inputs["link_library"]),
                 objects=[str(path) for path in inputs["objects"]],
                 info_plist=None if inputs["info_plist"] is None else str(inputs["info_plist"]),
                 threaded=not clashes)
    with contextlib.ExitStack() as held:
        # The app part's lock stays held shared until the probe has linked against it.
        app_entry = None if app is None else held.enter_context(_app_part(app, log, deadline))
        scratch = _scratch()
        held.callback(shutil.rmtree, scratch, ignore_errors=True)
        if app_entry is None:
            argv = _arguments(optimization=inputs["optimization"], output=str(scratch / "probe"), **shape)
        else:
            argv = _probe_arguments(app_entry=str(app_entry), output=str(scratch / "probe"), **shape)
        _compile(argv, scratch / "build.log", log, inputs["env"], deadline)
        if _describe(**inputs)[0] != key:
            raise ProbeBuildError("a build input changed while the compiler ran; nothing was stored")
        (scratch / "manifest.json").write_text(json.dumps(dict(manifest, key=key), indent=2) + "\n")
        if entry.exists() and (replace or not _is_entry(entry)):
            shutil.rmtree(entry)
        try:
            os.rename(scratch, entry)
        except OSError:
            if not _is_entry(entry):
                raise
            # Another build stored this key while ours ran; that entry wins.
        shutil.copy2(entry / "probe", output)
    _prune()
    print(f"built {key}", flush=True)
    return Built(key, False, entry, app_key)


def typecheck_sources(*, app_sources: Sequence[Path], sources: Sequence[Path], bridging_header: Path,
                      env: Mapping[str, str], defines: Sequence[str] = (), optimization: str = "-Onone") -> int:
    """Type-check `sources` in the shape `build` compiles them at `optimization`: against the app part made from
    `app_sources` (compiled first when no build has stored it), or, at any optimization but -Onone, with
    `app_sources` in one module. Batch mode, no artifact of the probe's. Returns swiftc's exit status, or the app
    part's compile status with its output printed when that compile fails."""
    module_cache = _module_cache()
    sdk = _sdk(env)
    argv = ["xcrun", "swiftc", "-typecheck", "-enable-batch-mode", "-j", str(os.cpu_count() or 1)]
    for define in defines:
        argv += ["-D", define]
    common = ["-sdk", sdk, "-target", TARGET, "-module-cache-path", str(module_cache),
              "-import-objc-header", _shown(bridging_header)]
    if optimization != "-Onone":
        argv += ["-module-name", APP_MODULE, *common, *(_shown(path) for path in (*app_sources, *sources))]
        return subprocess.run(argv, cwd=ROOT, env=dict(env), stdin=subprocess.DEVNULL).returncode
    app = _describe_app(app_sources=app_sources, bridging_header=bridging_header, env=env, sdk=sdk)
    with tempfile.TemporaryDirectory(prefix="probe-typecheck-") as folder:
        log = Path(folder) / "app-part.log"
        deadline = _Deadline(time.monotonic() + TYPECHECK_APP_PART_TIMEOUT_S, TYPECHECK_APP_PART_TIMEOUT_S)
        try:
            with _app_part(app, log, deadline) as entry:
                argv += ["-module-name", PROBE_MODULE, *_import_arguments(str(entry)), *common,
                         *(_shown(path) for path in sources)]
                status = subprocess.run(argv, cwd=ROOT, env=dict(env), stdin=subprocess.DEVNULL).returncode
        except subprocess.CalledProcessError as error:
            sys.stdout.write(log.read_text(errors="replace"))
            print(f"error: the app part did not compile (status {error.returncode}), so the probe was not checked",
                  flush=True)
            return error.returncode
        except (subprocess.TimeoutExpired, ProbeBuildError) as error:
            print(f"error: no app part, so the probe was not checked: {error}", flush=True)
            return 1
    _prune()
    return status


def typecheck(name: str) -> int:
    """Type-check probe set `name` in the shape its runner builds it (`typecheck_sources`)."""
    probe_set = PROBE_SETS[name]
    return typecheck_sources(app_sources=app_sources(), sources=probe_sources(name),
                             bridging_header=ROOT / BRIDGING_HEADER, env=os.environ, defines=probe_set.defines,
                             optimization=probe_set.optimization)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Type-check one probe directory's Swift files in the shape its "
                                                 "runner builds them: against the cached app part (compiled first "
                                                 "when the app sources changed), or with the app sources for a "
                                                 "set built -O; batch mode, no probe linked, stops before SILGen.")
    parser.add_argument("--typecheck", required=True, metavar="PROBE_DIR",
                        help="scripts/<name>, <name> or an absolute path of a probe directory in PROBE_SETS")
    arguments = parser.parse_args(argv)
    name = Path(arguments.typecheck).name
    if name not in PROBE_SETS:
        print(f"error: {arguments.typecheck} is not a probe set; the names are: " + ", ".join(sorted(PROBE_SETS)),
              file=sys.stderr)
        return 2
    return typecheck(name)


if __name__ == "__main__":
    sys.exit(main())
