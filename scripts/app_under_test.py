"""The app a drive launches by default, and the refusal to launch a stale one.

Standard library only, and it imports nothing from `scripts/` (the rule
`verification_isolation` follows). `DR_VERIFY_APP` selects another app, a
bundle or an executable; `DR_ALLOW_STALE_APP=1` skips the freshness check.
"""

from __future__ import annotations

import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEBUG_APP = ROOT / "build/Build/Products/Debug/Doctored.app"
RELEASE_APP = ROOT / "build/Build/Products/Release/Doctored.app"
EXECUTABLE_IN_BUNDLE = "Contents/MacOS/Doctored"


def default_app_bundle() -> Path:
    """`DR_VERIFY_APP` as given, else the Debug bundle."""
    value = os.environ.get("DR_VERIFY_APP")
    if value:
        return Path(value).expanduser()
    return DEBUG_APP


def default_app_executable() -> Path:
    """`DR_VERIFY_APP` (a bundle gains its executable), else the Debug executable."""
    value = os.environ.get("DR_VERIFY_APP")
    if value:
        path = Path(value).expanduser()
        return path / EXECUTABLE_IN_BUNDLE if path.suffix == ".app" else path
    return DEBUG_APP / EXECUTABLE_IN_BUNDLE


def _inputs():
    yield from (ROOT / "app/Sources").glob("*.swift")
    yield ROOT / "core/include/doctored_core.h"
    yield ROOT / "core/target/release/libdoctored_core.a"
    # The bundled doctored-mcp helper and the protocol versions it shares with the core.
    yield from (ROOT / "mcp-proxy/src").glob("*.rs")
    yield ROOT / "core/src/mcp_versions.rs"


def require_fresh_app(executable: str | os.PathLike) -> None:
    """Exit unless `executable` is at least as new as everything it is built from."""
    if os.environ.get("DR_ALLOW_STALE_APP") == "1":
        return
    path = Path(executable)
    release = "/Build/Products/Release/" in os.path.abspath(path)
    target = "make app" if release else "make app-debug"

    def refuse(reason):
        raise SystemExit(f"refusing to launch {executable}: {reason}. Rebuild with `{target}`, "
                         "or set DR_ALLOW_STALE_APP=1 to launch it anyway.")

    if not path.is_file():
        refuse("not built")
    built = path.stat().st_mtime_ns
    library = path.parent / "Doctored.debug.dylib"
    if library.is_file():
        built = max(built, library.stat().st_mtime_ns)
    newest, changed = None, 0
    for source in _inputs():
        try:
            modified = source.stat().st_mtime_ns
        except OSError:
            continue
        if modified > changed:
            newest, changed = source, modified
    if newest is not None and built < changed:
        refuse(f"older than {newest.relative_to(ROOT)}")
