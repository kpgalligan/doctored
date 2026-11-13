#!/usr/bin/env python3
"""Build an app-hosted probe once per source state, and type-check a probe directory.

`build` compiles the app sources with a probe's sources into
`build/probe-cache/<key>/probe` and copies that file to the caller's path. The key
covers every input, so a second call with the same inputs copies and compiles
nothing. The Clang module cache is `build/probe-module-cache`, shared by every
build and type check, and made again when it was made at another path (a
worktree's `build/` copied from another checkout; `_module_cache`).

`python3 scripts/probe_build.py --typecheck scripts/<probe dir>` type-checks the
app sources with that directory's probe set in batch mode. It writes no artifact
and stops before SILGen, like `make typecheck`.

With `DR_PROBE_BUILD_ONLY=1` in the environment, `build` ends the process with
status 0 right after it delivers the probe and prints `build only: <output>`, so
a runner's build is checked without launching anything it would run next. A
runner that removes its scratch folder takes the variable first
(`take_build_only`) and stops after the build on its own.

Standard library only; imports nothing from `scripts/`.
"""

from __future__ import annotations

import argparse
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
from pathlib import Path
from typing import Mapping, NamedTuple, Sequence

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / "build/probe-cache"
MODULE_CACHE = ROOT / "build/probe-module-cache"
# The file in MODULE_CACHE that names the path the cache was made at (`_module_cache`).
MODULE_CACHE_PATH_FILE = "made-at"
# `make gate` builds each of its probe runs (`PROBE_RUNS` in `scripts/gate.py`) and later reuses that build, so the
# cache must hold all of them beside a few hand runs; with fewer entries than gate probes, each build prunes an
# earlier one and every reuse becomes another whole-app compile. About 75 MB an entry.
KEEP_ENTRIES = 16
TARGET = "arm64-apple-macos15.0"
BRIDGING_HEADER = "app/Bridging/Doctored-Bridging-Header.h"
KEY_ENVIRONMENT = ("MACOSX_DEPLOYMENT_TARGET", "SDKROOT", "TOOLCHAINS", "DEVELOPER_DIR")
QUOTED_INCLUDE = re.compile(rb'^[ \t]*#[ \t]*(?:include|import)[ \t]+"([^"\n]+)"', re.MULTILINE)
QUERY_TIMEOUT_S = 30
BUILD_ONLY_ENV = "DR_PROBE_BUILD_ONLY"


class ProbeBuildError(RuntimeError):
    """A probe build that failed for a reason other than the compiler's exit status or its timeout."""


class ProbeSet(NamedTuple):
    """The Swift files one runner compiles with the app sources."""
    files: tuple[str, ...] | None       # relative to scripts/, in command-line order; None = the directory's *.swift
    exclude: tuple[str, ...] = ()       # file names left out when files is None
    defines: tuple[str, ...] = ()


class Built(NamedTuple):
    key: str        # 16 hex characters
    reused: bool
    entry: Path     # CACHE / key


def _listed(directory: str, *names: str) -> tuple[str, ...]:
    return tuple(f"{directory}/{name}" for name in names)


# Key = directory name under scripts/. Each set is what its runner compiles with the app sources.
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
    "raw-decode-probe": ProbeSet(_listed("raw-decode-probe", "main.swift")),
    "raw-local-probe": ProbeSet(None, exclude=("reference.swift", "oracle.swift", "geometry-reference.swift")),
    "raw-probe": ProbeSet(_listed("raw-probe", "main.swift")),
    "raw-workflow-probe": ProbeSet(_listed("raw-workflow-probe", "main.swift", "develop_ui.swift")),
    "reach-probe": ProbeSet(_listed("reach-probe", "main.swift")),
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


def _module_cache() -> Path:
    """`MODULE_CACHE`, made again first when it was made at another path.

    Clang's module cache records its own absolute path, so a copy of it elsewhere (a worktree's
    `build/` copied from another checkout) fails every compile with "PCH was compiled with module
    cache path …". The cache's `MODULE_CACHE_PATH_FILE` names the path it was made at; when that
    file is missing or names another path, the folder is removed and made again. The check holds
    a lock beside the folder, so a build that starts meanwhile finds the new file and keeps the cache.
    """
    MODULE_CACHE.parent.mkdir(parents=True, exist_ok=True)
    here = str(MODULE_CACHE.resolve())
    with open(MODULE_CACHE.with_name(MODULE_CACHE.name + ".lock"), "a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            made_at = (MODULE_CACHE / MODULE_CACHE_PATH_FILE).read_text()
        except OSError:
            made_at = None
        if made_at != here:
            shutil.rmtree(MODULE_CACHE, ignore_errors=True)
            MODULE_CACHE.mkdir(exist_ok=True)
            (MODULE_CACHE / MODULE_CACHE_PATH_FILE).write_text(here)
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


def _arguments(*, optimization: str, defines: Sequence[str], sdk: str, module_cache: str, bridging_header: str,
               sources: Sequence[str], library: Sequence[str], objects: Sequence[str], info_plist: str | None,
               output: str, threaded: bool) -> list[str]:
    argv = ["xcrun", "swiftc", optimization, "-whole-module-optimization"]
    if optimization == "-Onone" and threaded:
        argv += ["-num-threads", str(os.cpu_count() or 1)]
    argv += ["-module-name", "Doctored"]
    for define in defines:
        argv += ["-D", define]
    argv += ["-sdk", sdk, "-target", TARGET, "-module-cache-path", module_cache,
             "-import-objc-header", bridging_header, *sources, *library, *objects]
    if info_plist is not None:
        argv += ["-Xlinker", "-sectcreate", "-Xlinker", "__TEXT", "-Xlinker", "__info_plist", "-Xlinker", info_plist]
    return [*argv, "-o", output]


def _describe(*, sources: Sequence[Path], bridging_header: Path, library: Path, env: Mapping[str, str],
              info_plist: Path | None, optimization: str, defines: Sequence[str], link_library: str,
              objects: Sequence[Path]) -> tuple[str, dict, str]:
    """The key, the manifest that explains it, and the SDK path."""
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
    included = _included(Path(bridging_header))
    inputs = [*sources, bridging_header, *included, library, *([] if info_plist is None else [info_plist]), *objects]
    arguments = _arguments(
        optimization=optimization, defines=defines, sdk=sdk, module_cache="module-cache",
        bridging_header=_label(bridging_header), sources=[_label(path) for path in sources],
        library=_library_arguments(_label(library), "directory-of:" + _label(library), link_library),
        objects=[_label(path) for path in objects],
        info_plist=None if info_plist is None else _label(info_plist), output="output",
        threaded=not case_clashes(sources))
    manifest = {
        # The `root:` labels are relative, and a probe can carry the repo's path (`#filePath`).
        "root": str(ROOT.resolve()),
        "helper_sha256": _digest(Path(__file__)),
        "toolchain": _query(["xcrun", "swiftc", "--version"], env),
        "sdk": sdk,
        "sdk_settings_sha256": _digest(Path(sdk) / "SDKSettings.json"),
        "arguments": arguments,
        "inputs": [[_label(path), _digest(path)] for path in inputs],
        "environment": {name: env.get(name) for name in KEY_ENVIRONMENT},
    }
    key = hashlib.sha256(json.dumps(manifest, sort_keys=True).encode()).hexdigest()[:16]
    return key, manifest, sdk


def cache_key(*, sources: Sequence[Path], bridging_header: Path, library: Path, env: Mapping[str, str],
              info_plist: Path | None = None, optimization: str = "-Onone", defines: Sequence[str] = (),
              link_library: str = "search", objects: Sequence[Path] = ()) -> str:
    """The cache key of a build with these inputs: 16 hex characters."""
    try:
        return _describe(sources=sources, bridging_header=bridging_header, library=library, env=env,
                         info_plist=info_plist, optimization=optimization, defines=defines,
                         link_library=link_library, objects=objects)[0]
    except OSError as error:
        raise ProbeBuildError(f"could not read a build input: {error}") from error


def _is_entry(entry: Path) -> bool:
    return (entry / "probe").is_file() and (entry / "manifest.json").is_file()


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


def _prune() -> None:
    """Keep the `KEEP_ENTRIES` entries with the newest `manifest.json`; remove the scratch folders of dead builds."""
    entries = []
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
        entries.append((stamp, child.name, child))
    for _, _, child in sorted(entries, reverse=True)[KEEP_ENTRIES:]:
        shutil.rmtree(child, ignore_errors=True)


def _compile(argv: list[str], build_log: Path, log: Path, env: Mapping[str, str], timeout: float) -> None:
    """Run the compiler in its own session; its process group dies with a timeout or an interrupt."""
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


def take_build_only(environment: dict[str, str] | None = None) -> bool:
    """Whether `os.environ` held `DR_PROBE_BUILD_ONLY=1`, once the variable is removed from it and from
    `environment` (the runner's child environment).

    `build` ends the process under that variable, so a runner that removes its scratch folder afterwards calls
    this first, builds, and returns after the build itself when it is true, launching nothing."""
    build_only = os.environ.pop(BUILD_ONLY_ENV, None) == "1"
    if environment is not None:
        environment.pop(BUILD_ONLY_ENV, None)
    return build_only


def build(*, sources: Sequence[Path], bridging_header: Path, library: Path, output: Path, log: Path,
          env: Mapping[str, str], timeout: float, info_plist: Path | None = None, optimization: str = "-Onone",
          defines: Sequence[str] = (), link_library: str = "search", objects: Sequence[Path] = ()) -> Built:
    """Deliver the probe built from these inputs to `output`, compiling only when no cached build has their key.

    Prints `reused build <key>` or `built <key>`. A compiler failure raises
    `subprocess.CalledProcessError`, a timeout `subprocess.TimeoutExpired`, anything else `ProbeBuildError`.
    With `DR_PROBE_BUILD_ONLY=1` in `os.environ` it then ends the process with status 0; no caller code runs.
    """
    inputs = dict(sources=[Path(path) for path in sources], bridging_header=Path(bridging_header),
                  library=Path(library), env=env, info_plist=None if info_plist is None else Path(info_plist),
                  optimization=optimization, defines=tuple(defines), link_library=link_library,
                  objects=[Path(path) for path in objects])
    try:
        built = _build(inputs, Path(output), Path(log), timeout)
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


def _build(inputs: dict, output: Path, log: Path, timeout: float) -> Built:
    key, manifest, sdk = _describe(**inputs)
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
            return Built(key, True, entry)

    CACHE.mkdir(parents=True, exist_ok=True)
    module_cache = _module_cache()
    scratch = CACHE / f"tmp-{os.getpid()}-{secrets.token_hex(4)}"
    scratch.mkdir()
    # Threaded WMO writes one object per source, named after its basename; on a case-insensitive
    # disk two sources whose basenames differ only by case overwrite each other's object and the
    # link fails. Such a build runs on one thread.
    clashes = case_clashes(inputs["sources"])
    if clashes and inputs["optimization"] == "-Onone":
        print("no -num-threads: sources share the case-folded basename(s) " + ", ".join(clashes), flush=True)
    try:
        library = inputs["library"]
        argv = _arguments(
            optimization=inputs["optimization"], defines=inputs["defines"], sdk=sdk,
            module_cache=str(module_cache), bridging_header=str(inputs["bridging_header"]),
            sources=[str(path) for path in inputs["sources"]],
            library=_library_arguments(str(library), str(library.parent), inputs["link_library"]),
            objects=[str(path) for path in inputs["objects"]],
            info_plist=None if inputs["info_plist"] is None else str(inputs["info_plist"]),
            output=str(scratch / "probe"), threaded=not clashes)
        _compile(argv, scratch / "build.log", log, inputs["env"], timeout)
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
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    _prune()
    print(f"built {key}", flush=True)
    return Built(key, False, entry)


def typecheck(name: str) -> int:
    """Type-check the app sources with probe set `name` in batch mode; returns swiftc's exit status."""
    module_cache = _module_cache()
    argv = ["xcrun", "swiftc", "-typecheck", "-enable-batch-mode", "-j", str(os.cpu_count() or 1),
            "-module-name", "Doctored"]
    for define in PROBE_SETS[name].defines:
        argv += ["-D", define]
    argv += ["-sdk", _sdk(os.environ), "-target", TARGET, "-module-cache-path", str(module_cache),
             "-import-objc-header", BRIDGING_HEADER,
             *(str(path.relative_to(ROOT)) for path in (*app_sources(), *probe_sources(name)))]
    return subprocess.run(argv, cwd=ROOT, stdin=subprocess.DEVNULL).returncode


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Type-check the app sources with one probe directory's Swift "
                                                 "files: batch mode, no artifact, stops before SILGen.")
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
