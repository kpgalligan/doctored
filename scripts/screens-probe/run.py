#!/usr/bin/env python3
"""Capture the documentation screens: build the screens probe once and run it in each look.

usage: python3 scripts/screens-probe/run.py --out DIR [--fixtures DIR] [--only ID,...]
                                             [--screens DIR] [--fallback-cache] [--keep]

Run `make rust project` first. The screens are `public-docs/screens/*.md` (INDEX.md is the
catalog), or the screen files of --screens (a trial folder outside the repository); their pictures
are the fixtures of `public-docs/screens/fixtures.json`, each checked by sha256 before use. A
fixture whose root is "fixtures" is found under --fixtures (the fixtures store, e.g.
~/devel/doctored-fixtures); a screen whose fixture is absent is skipped, not failed. Each look's
DR_MODELS_DIR is the default models folder under its private home, so Settings ▸ Models names it
`~/Library/Application Support/Doctored/Models` and never a scratch or store path: a link to the
store's read-only `models/` folder when a screen `needs` "models" (such a screen is skipped without
it), else absent.

This script checks a screen file's shape only. Every name in it (window kinds, cards, panels,
sheets, views, UI actions and their arguments) is the probe's: the probe parses the whole plan
against its tables (windows.swift, editor_tables.swift and actions.swift beside this script)
before it builds any window, so a misspelt name fails the light run at once.

Each look's probe writes `DIR/<look>/manifest.json` and `DIR/<look>/<id>.<capture>.<look>.png`;
DIR must lie outside the repository. Captures are `screencapture -l` of the probe's own windows,
which needs the Screen Recording grant; --fallback-cache uses `cacheDisplay` instead (no title
bar or window shape). `public-docs/scripts/import-screens.mjs DIR` turns the output into the
site's assets.

The run is isolated: private preset directories, Actions and recovery folders and home for each
look (so both looks start from the same state), a private defaults domain, a scratch agent socket
path that is never bound, background mode, recorded alerts and a private pasteboard. Scroll bars
are pinned to the classic style whatever the system setting, so both looks lay out alike. The
scratch folder is kept on failure, or with --keep.
"""

import sys

sys.dont_write_bytecode = True

import argparse  # noqa: E402
import hashlib  # noqa: E402
import importlib.util  # noqa: E402
import json  # noqa: E402
import os  # noqa: E402
from pathlib import Path  # noqa: E402
import plistlib  # noqa: E402
import shutil  # noqa: E402
import subprocess  # noqa: E402
import tempfile  # noqa: E402

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PROBE = "screens-probe"
SCREENS = ROOT / "public-docs/screens"
FIXTURES = SCREENS / "fixtures.json"
LOOKS = ("light", "dark")
# A look's time limit grows with its plan: a capture takes about 5 s and most screens have one or
# two, so 30 s a screen leaves a wide margin for the slow ones (a model, the 24 MP RAW) while a
# hung probe still ends. A short --only run gets the floor.
RUN_TIMEOUT_FLOOR_SECONDS = 600
RUN_TIMEOUT_SECONDS_PER_SCREEN = 30
BUNDLE_ID = "com.kgalligan.doctored.screens-probe"

SCREEN_KEYS = {"id", "title", "fixture", "window", "steps", "captures"}
OPTIONAL_SCREEN_KEYS = {"needs"}
# What a screen may need besides its fixture: "models" is the store's model folder.
NEEDS = {"models"}
# The app's default models folder under a home (`ModelStore`'s default directory).
MODELS_UNDER_HOME = Path("Library/Application Support/Doctored/Models")
FIXTURE_KEYS = {"id", "root", "path", "kind", "licence", "author", "source", "sha256"}
FENCE_OPEN = "```json screen"
FENCE_CLOSE = "```"


def load(name, path):
    """A module of scripts/ loaded by path, so scripts/ never goes on sys.path."""
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


isolation = load("verification_isolation", ROOT / "scripts/verification_isolation.py")
probe_build = load("probe_build", ROOT / "scripts/probe_build.py")


class ScreenError(ValueError):
    """A screen file or the fixture table is malformed."""


def _check_step(step, context):
    if not isinstance(step, dict):
        raise ScreenError(f"{context}: a step is an object")
    kinds = [key for key in ("tool", "ui") if key in step]
    if len(kinds) != 1 or not set(step) <= {kinds[0], "args"}:
        raise ScreenError(f"{context}: a step names exactly one of \"tool\" or \"ui\", plus \"args\"")
    if not isinstance(step[kinds[0]], str) or not step[kinds[0]]:
        raise ScreenError(f"{context}: \"{kinds[0]}\" is not a name")
    if not isinstance(step.get("args", {}), dict):
        raise ScreenError(f"{context}: \"args\" is not an object")


def _check_capture(capture, context):
    """A capture's shape; its kind and keys are the probe's to check."""
    if not isinstance(capture, dict):
        raise ScreenError(f"{context}: a capture is an object")
    for key in ("name", "of"):
        if not isinstance(capture.get(key), str) or not capture[key]:
            raise ScreenError(f"{context}: a capture has a \"{key}\"")


def _sections(prose):
    """The `## <name>` sections of `prose`, by name."""
    sections, name, lines = {}, None, []
    for line in prose.splitlines():
        if line.startswith("## "):
            if name is not None:
                sections[name] = "\n".join(lines).strip()
            name, lines = line[3:].strip(), []
        elif name is not None:
            lines.append(line)
    if name is not None:
        sections[name] = "\n".join(lines).strip()
    return sections


def read_screen(path: Path) -> dict:
    """The screen file's front block, plus `alt`: each capture's alt text by capture name ("" when
    the file has none yet). Raises ScreenError on a malformed file."""
    lines = path.read_text().splitlines()
    if not lines or lines[0] != FENCE_OPEN:
        raise ScreenError(f"{path.name}: the first line is not {FENCE_OPEN}")
    try:
        close = lines.index(FENCE_CLOSE, 1)
    except ValueError:
        raise ScreenError(f"{path.name}: the front block has no closing fence") from None
    try:
        screen = json.loads("\n".join(lines[1:close]))
    except json.JSONDecodeError as error:
        raise ScreenError(f"{path.name}: the front block is not JSON: {error}") from None
    if not isinstance(screen, dict) or not SCREEN_KEYS <= set(screen) <= SCREEN_KEYS | OPTIONAL_SCREEN_KEYS:
        keys = sorted(screen) if isinstance(screen, dict) else type(screen).__name__
        raise ScreenError(f"{path.name}: keys are {keys}; expected {sorted(SCREEN_KEYS)}"
                          f" and optionally {sorted(OPTIONAL_SCREEN_KEYS)}")
    if screen["id"] != path.stem:
        raise ScreenError(f"{path.name}: id \"{screen['id']}\" is not the file stem")
    for key in ("title", "window"):
        if not isinstance(screen[key], str) or not screen[key]:
            raise ScreenError(f"{path.name}: \"{key}\" is not a non-empty string")
    if screen["fixture"] is not None and (not isinstance(screen["fixture"], str) or not screen["fixture"]):
        raise ScreenError(f"{path.name}: \"fixture\" is a fixture id, or null for a window that opens none")
    needs = screen.setdefault("needs", [])
    if not isinstance(needs, list) or not set(needs) <= NEEDS:
        raise ScreenError(f"{path.name}: \"needs\" is a list of {sorted(NEEDS)}")
    if not isinstance(screen["steps"], list) or not isinstance(screen["captures"], list):
        raise ScreenError(f"{path.name}: steps and captures are lists")
    for index, step in enumerate(screen["steps"]):
        _check_step(step, f"{path.name} step {index + 1}")
    if not screen["captures"]:
        raise ScreenError(f"{path.name}: no captures")
    for index, capture in enumerate(screen["captures"]):
        _check_capture(capture, f"{path.name} capture {index + 1}")
    names = [capture["name"] for capture in screen["captures"]]
    if len(set(names)) != len(names):
        raise ScreenError(f"{path.name}: two captures share a name")
    prose = "\n".join(lines[close + 1:]).strip()
    if len(names) == 1:
        screen["alt"] = {names[0]: prose}
    else:
        sections = _sections(prose)
        screen["alt"] = {name: sections.get(name, "") for name in names}
    return screen


def read_fixtures(path: Path) -> dict[str, dict]:
    """The fixture table by id. Raises ScreenError on a malformed table."""
    table = json.loads(path.read_text())
    entries = table.get("fixtures") if isinstance(table, dict) else None
    if not isinstance(entries, list):
        raise ScreenError(f"{path.name}: no \"fixtures\" list")
    fixtures = {}
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != FIXTURE_KEYS:
            raise ScreenError(f"{path.name}: a fixture has exactly the keys {sorted(FIXTURE_KEYS)}")
        if entry["root"] not in ("repo", "fixtures") or entry["kind"] not in ("photo", "raw"):
            raise ScreenError(f"{path.name}: {entry['id']}: root is repo|fixtures and kind photo|raw")
        if Path(entry["path"]).is_absolute() or ".." in Path(entry["path"]).parts:
            raise ScreenError(f"{path.name}: {entry['id']}: the path is relative to its root")
        if entry["id"] in fixtures:
            raise ScreenError(f"{path.name}: {entry['id']} appears twice")
        fixtures[entry["id"]] = entry
    return fixtures


def resolve_fixture(entry: dict, fixtures_root: Path | None) -> Path | None:
    """The fixture's file, or None when it is absent (no --fixtures for a store fixture, or no file)."""
    if entry["root"] == "repo":
        path = ROOT / entry["path"]
    elif fixtures_root is None:
        return None
    else:
        path = fixtures_root / entry["path"]
    return path if path.is_file() else None


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def labelled(entry: dict) -> str:
    """The fixture's path under its root's label, never an absolute path."""
    return f"<{entry['root']}>/{entry['path']}"


def models_store(fixtures_root):
    """The fixtures store's read-only model folder (scripts/fill_model_fixtures.py), or None."""
    if fixtures_root is None or not (fixtures_root / "models").is_dir():
        return None
    return fixtures_root / "models"


def screen_locked() -> bool:
    """Whether the console session's screen is locked (the I/O registry's console users).

    Captures still work while it is, but they differ from an unlocked run's where a window's key
    state shows: the More panel's selected field draws grey instead of blue. Unknown is False.
    """
    listing = subprocess.run(["ioreg", "-n", "Root", "-d1", "-a"], capture_output=True, check=False)
    try:
        root = plistlib.loads(listing.stdout)
    except (plistlib.InvalidFileException, ValueError):
        return False
    users = root.get("IOConsoleUsers", []) if isinstance(root, dict) else []
    return any(user.get("kCGSSessionOnConsoleKey") and user.get("CGSSessionScreenIsLocked")
               for user in users if isinstance(user, dict))


def plan(arguments, fixtures_root):
    """The plan's screens and skip records, after reading every screen file and the fixture table."""
    fixtures = read_fixtures(FIXTURES)
    folder = Path(arguments.screens).expanduser().resolve() if arguments.screens else SCREENS
    screens = [read_screen(path) for path in sorted(folder.glob("*.md")) if path.name != "INDEX.md"]
    for screen in screens:
        if screen["fixture"] is not None and screen["fixture"] not in fixtures:
            raise ScreenError(f"{screen['id']}.md: no fixture \"{screen['fixture']}\" in fixtures.json")
    if arguments.only:
        wanted = [name.strip() for name in arguments.only.split(",") if name.strip()]
        unknown = sorted(set(wanted) - {screen["id"] for screen in screens})
        if unknown:
            raise ScreenError(f"--only names no screen file: {', '.join(unknown)}")
        screens = [screen for screen in screens if screen["id"] in wanted]
    planned, skipped, checked = [], [], {}
    for screen in screens:
        if "models" in screen["needs"] and models_store(fixtures_root) is None:
            reason = "it needs the fixtures store's models/ folder (--fixtures)"
            print(f"skip {screen['id']}: {reason}")
            skipped.append({"screen": screen["id"], "reason": reason})
            continue
        if screen["fixture"] is None:
            planned.append({"id": screen["id"], "window": screen["window"], "fixture": None,
                            "needs": screen["needs"], "steps": screen["steps"], "captures": screen["captures"]})
            continue
        entry = fixtures[screen["fixture"]]
        path = resolve_fixture(entry, fixtures_root)
        if path is None:
            why = "no --fixtures given" if entry["root"] == "fixtures" and fixtures_root is None else "no file"
            reason = f"fixture {entry['id']} is absent ({why}): {labelled(entry)}"
            print(f"skip {screen['id']}: {reason}")
            skipped.append({"screen": screen["id"], "reason": reason})
            continue
        if entry["id"] not in checked:
            checked[entry["id"]] = sha256(path)
            if checked[entry["id"]] != entry["sha256"]:
                raise RuntimeError(f"fixture {entry['id']} at {labelled(entry)} has sha256 "
                                   f"{checked[entry['id']]}, not the table's {entry['sha256']}")
        planned.append({"id": screen["id"], "window": screen["window"],
                        "fixture": {"id": entry["id"], "path": str(path.resolve()), "kind": entry["kind"]},
                        "needs": screen["needs"], "steps": screen["steps"], "captures": screen["captures"]})
    return {"screens": planned, "skipped": skipped}


def run(arguments, scratch, out, fixtures_root):
    library = ROOT / "core/target/release/libdoctored_core.a"
    info = ROOT / "app/Info.plist"
    for required in (library, info):
        if not required.is_file():
            raise RuntimeError(f"missing {required.relative_to(ROOT)}; run `make rust project` first")
    plan_path = scratch / "plan.json"
    planned = plan(arguments, fixtures_root)
    plan_path.write_text(json.dumps(planned, indent=2))
    needs_models = any("models" in screen["needs"] for screen in planned["screens"])
    timeout = max(RUN_TIMEOUT_FLOOR_SECONDS,
                  RUN_TIMEOUT_SECONDS_PER_SCREEN * len(planned["screens"]))

    environment = dict(
        os.environ,
        ANTHROPIC_API_KEY="dummy",
        DR_BACKGROUND="1",
        DR_AUTOMATION_ALERTS="record",
        # A capture instance is not a test instance: the 24 MP RAW's preview needs 2.46 GB.
        DR_WORKING_LIMIT_BYTES="8589934592",
        DR_EDR_HEADROOM="1",
        DR_AGENT_SOCKET=str(scratch / "a.sock"),
        DR_PASTEBOARD_NAME=f"{BUNDLE_ID}.{os.getpid()}",
        CLANG_MODULE_CACHE_PATH=str(scratch / "clang-cache"),
        DR_SCREENS_PLAN=str(plan_path),
        DR_SCREENS_MECHANISM="cache" if arguments.fallback_cache else "server",
    )
    environment.pop("DR_SCREENS_EMAIL", None)
    if fixtures_root is not None:
        environment["DR_SCREENS_FIXTURES_ROOT"] = str(fixtures_root)
    # The real document type registrations under a private defaults domain; the copy is
    # embedded in the probe, never written into the app.
    with info.open("rb") as source:
        probe_info = plistlib.load(source)
    probe_info["CFBundleIdentifier"] = BUNDLE_ID
    info_copy = scratch / "Info.plist"
    with info_copy.open("wb") as destination:
        plistlib.dump(probe_info, destination)

    # probe_build.build would end this process under DR_PROBE_BUILD_ONLY=1 and leave the scratch
    # folder behind; the runner handles that mode itself.
    build_only = os.environ.pop(probe_build.BUILD_ONLY_ENV, None) == "1"
    environment.pop(probe_build.BUILD_ONLY_ENV, None)
    binary = scratch / "probe"
    probe_build.build(
        app_sources=probe_build.app_sources(), sources=probe_build.probe_sources(PROBE),
        bridging_header=ROOT / probe_build.BRIDGING_HEADER, library=library, info_plist=info_copy,
        output=binary, log=scratch / "build.log", env=environment, timeout=600)
    if build_only:
        print(f"build only: {binary}")
        return None

    email = subprocess.run(["git", "config", "user.email"], cwd=ROOT, capture_output=True, text=True,
                           check=False).stdout.strip()
    statuses = {}
    for look in LOOKS:
        folder = out / look
        folder.mkdir(parents=True, exist_ok=True)
        log = scratch / f"run-{look}.log"
        print(f"{look}: {len(planned['screens'])} screens, time limit {timeout} s", flush=True)
        # Each look its own libraries and home, so the dark run starts where the light one did
        # (a preset a step saves is not already there).
        private = scratch / look
        (private / "home").mkdir(parents=True)
        child = dict(environment, DR_SCREENS_OUT=str(folder), DR_SCREENS_LOOK=look,
                     DR_ACTIONS_DIR=str(private / "actions"), DR_RECOVERY_DIR=str(private / "recovery"),
                     CFFIXED_USER_HOME=str(private / "home"), **isolation.preset_environment(private))
        # The app's default models folder under the look's home (module doc), which the Models
        # pane's intro writes from `~`. Read-only: no screen downloads a model.
        models = private / "home" / MODELS_UNDER_HOME
        child["DR_MODELS_DIR"] = str(models)
        if needs_models:
            models.parent.mkdir(parents=True)
            models.symlink_to(models_store(fixtures_root), target_is_directory=True)
        if email:
            child["DR_SCREENS_EMAIL"] = email
        locked = screen_locked()
        with log.open("w") as handle:
            try:
                # The scroll bar style is pinned to classic (Always): left to the system's
                # Automatic setting it follows whether a mouse is connected, which can change
                # mid-run, and overlay scrollers reflow scroll areas and the fit zoom, so the two
                # looks of a screen would stop matching. The screens' text assumes classic bars.
                done = subprocess.run([str(binary), "-ApplePersistenceIgnoreState", "YES",
                                       "-AppleShowScrollBars", "Always"], cwd=ROOT,
                                      env=child, stdout=handle, stderr=subprocess.STDOUT,
                                      timeout=timeout, check=False)
                statuses[look] = done.returncode
            except subprocess.TimeoutExpired:
                statuses[look] = f"timeout after {timeout} s"
        locked = screen_locked() or locked
        text = log.read_text(errors="replace")
        print(f"---- {look} (exit {statuses[look]})")
        print(text, end="" if text.endswith("\n") else "\n")
        if locked:
            print(f"NOTE: the screen was locked during the {look} look; a selected field in a panel "
                  "draws grey, not blue, so a capture that shows one differs from an unlocked run's")
        alerts = [line for line in text.splitlines() if "[AutomationAlerts]" in line]
        print(f"[AutomationAlerts] lines in the {look} run: {len(alerts)}")
        for line in alerts:
            print(line)
    return statuses


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", required=True, help="the folder the looks' PNGs and manifests go under")
    parser.add_argument("--fixtures", help="the fixtures store, for fixtures whose root is \"fixtures\"")
    parser.add_argument("--only", help="comma-separated screen ids (default: every screen file)")
    parser.add_argument("--screens", help="a folder of trial screen files outside the repository "
                        "(default: public-docs/screens)")
    parser.add_argument("--fallback-cache", action="store_true",
                        help="capture with cacheDisplay when Screen Recording is not granted")
    parser.add_argument("--keep", action="store_true", help="keep the scratch folder after success too")
    arguments = parser.parse_args()
    out = Path(arguments.out).expanduser().resolve()
    if out == ROOT.resolve() or ROOT.resolve() in out.parents:
        print("FAIL: --out is inside the repository; captures go outside it", file=sys.stderr)
        return 1
    if arguments.screens:
        trial = Path(arguments.screens).expanduser().resolve()
        if not trial.is_dir() or trial == ROOT.resolve() or ROOT.resolve() in trial.parents:
            print("FAIL: --screens is a folder outside the repository", file=sys.stderr)
            return 1
    fixtures_root = Path(arguments.fixtures).expanduser().resolve() if arguments.fixtures else None
    scratch = Path(tempfile.mkdtemp(prefix="dr-screens-"))
    print(f"scratch folder: {scratch}", flush=True)
    try:
        statuses = run(arguments, scratch, out, fixtures_root)
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError, probe_build.ProbeBuildError) as error:
        print(f"FAIL: {error}\nscratch kept at {scratch}", file=sys.stderr)
        build_log = scratch / "build.log"
        if build_log.is_file():
            errors = [line for line in build_log.read_text(errors="replace").splitlines() if "error:" in line]
            print("\n".join(errors[:40]), file=sys.stderr)
        return 1
    if statuses is None:
        shutil.rmtree(scratch, ignore_errors=True)
        return 0
    if not all(status == 0 for status in statuses.values()):
        print(f"FAIL: probe exits {statuses}; scratch kept at {scratch}", file=sys.stderr)
        return 1
    if arguments.keep:
        print(f"scratch kept at {scratch}")
    else:
        shutil.rmtree(scratch, ignore_errors=True)
    print(f"PASS: both looks ran; captures under {arguments.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
