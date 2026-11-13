#!/usr/bin/env python3
"""Tests of `scripts/probe_build.py` on a synthetic module in a temporary directory: `helper.swift` is its
app source, compiled as the app part, and `main.swift` its probe.

Run: `python3 -B scripts/test_probe_build.py -v`. Needs the Swift toolchain; it never
touches `build/probe-cache` or the app sources.
"""

import contextlib
import fcntl
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent

# Loaded by path with no `sys.modules` entry, the way the probe runners load it.
_spec = importlib.util.spec_from_file_location("probe_build", HERE / "probe_build.py")
probe_build = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(probe_build)

MAIN = 'print(greeting())\n'
HELPER = 'func greeting() -> String { "value \\(probe_value()) note {note}" }\n'
BRIDGE = '#ifndef PROBE_BRIDGE_H\n#define PROBE_BRIDGE_H\n#include "inner/core.h"\n#endif\n'
CORE_HEADER = 'int probe_value(void);\n'
CORE_SOURCE = 'int probe_value(void) { return 41; }\n'
EXTRA_SOURCE = 'int probe_extra(void) { return 7; }\n'
TIMEOUT_S = 300
# A child process that loads the helper by path, points its caches at the test's folders, builds and
# then prints `after build`. argv: helper, cache, module cache, inputs as JSON, output, log.
BUILD_CHILD = '''
import importlib.util, json, os, sys
from pathlib import Path
spec = importlib.util.spec_from_file_location("probe_build", sys.argv[1])
probe_build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe_build)
probe_build.CACHE, probe_build.MODULE_CACHE = Path(sys.argv[2]), Path(sys.argv[3])
inputs = json.loads(sys.argv[4])
probe_build.build(app_sources=inputs["app_sources"], sources=inputs["sources"],
                  bridging_header=inputs["bridging_header"], library=inputs["library"],
                  info_plist=inputs["info_plist"], objects=inputs["objects"],
                  env=dict(os.environ), output=sys.argv[5], log=sys.argv[6], timeout=300)
print("after build", flush=True)
'''


def write_module(folder: Path) -> dict:
    """The synthetic module under `folder`; returns `build`'s input arguments."""
    (folder / "inner").mkdir(parents=True)
    (folder / "lib").mkdir()
    (folder / "main.swift").write_text(MAIN)
    (folder / "helper.swift").write_text(HELPER.replace("{note}", "one"))
    (folder / "bridge.h").write_text(BRIDGE)
    (folder / "inner/core.h").write_text(CORE_HEADER)
    for name, source in (("core", CORE_SOURCE), ("extra", EXTRA_SOURCE)):
        (folder / f"{name}.c").write_text(source)
        subprocess.run(["xcrun", "clang", "-target", "arm64-apple-macos15.0", "-c", str(folder / f"{name}.c"),
                        "-o", str(folder / f"{name}.o")], check=True)
    # `ar` stamps members with the time, so the archive is made once and copied, never remade.
    subprocess.run(["xcrun", "ar", "rcs", str(folder / "lib/libdoctored_core.a"), str(folder / "core.o")],
                   check=True, stderr=subprocess.DEVNULL)
    with (folder / "Info.plist").open("wb") as stream:
        plistlib.dump({"CFBundleIdentifier": "com.kgalligan.doctored.probe-build-test"}, stream)
    return inputs_of(folder)


def inputs_of(folder: Path) -> dict:
    return dict(app_sources=[folder / "helper.swift"], sources=[folder / "main.swift"],
                bridging_header=folder / "bridge.h", library=folder / "lib/libdoctored_core.a",
                info_plist=folder / "Info.plist", objects=[folder / "extra.o"], env=dict(os.environ))


def jsonable(inputs: dict) -> str:
    return json.dumps({name: (str(value) if isinstance(value, Path) else [str(path) for path in value])
                       for name, value in inputs.items() if name != "env"})


class ProbeBuildTests(unittest.TestCase):
    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(prefix="rz-probe-build-test-")).resolve()
        self.addCleanup(shutil.rmtree, self.scratch, ignore_errors=True)
        for name, folder in (("CACHE", "cache"), ("MODULE_CACHE", "module-cache")):
            patcher = mock.patch.object(probe_build, name, self.scratch / folder)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.module = self.scratch / "module"
        self.inputs = write_module(self.module)
        self.log = self.scratch / "build.log"

    def build(self, output, **changes):
        """`(Built, what it printed)` of one `build` call."""
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed):
            built = probe_build.build(**{**self.inputs, **changes}, output=self.scratch / output, log=self.log,
                                      timeout=TIMEOUT_S)
        return built, printed.getvalue()

    def child(self, output, inputs=None, build_only=False):
        """A `BUILD_CHILD` process building `inputs` (the module's by default) to `output`, started."""
        env = dict(os.environ)
        env.pop(probe_build.BUILD_ONLY_ENV, None)
        if build_only:
            env[probe_build.BUILD_ONLY_ENV] = "1"
        return subprocess.Popen(
            [sys.executable, "-B", "-c", BUILD_CHILD, str(HERE / "probe_build.py"), str(self.scratch / "cache"),
             str(self.scratch / "module-cache"), jsonable(inputs or self.inputs), str(self.scratch / output),
             str(self.scratch / f"{output}.log")],
            env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)

    def run_probe(self, output):
        return subprocess.run([str(self.scratch / output)], stdout=subprocess.PIPE, text=True, check=True).stdout

    def entries(self):
        cache = self.scratch / "cache"
        return sorted(child.name for child in cache.iterdir()) if cache.is_dir() else []

    def probe_entries(self):
        return [name for name in self.entries() if (self.scratch / "cache" / name).is_dir()
                and not name.startswith(("app-", "tmp-"))]

    def app_entries(self):
        return [name for name in self.entries() if (self.scratch / "cache" / name).is_dir()
                and name.startswith("app-")]

    def write_probe(self, text, folder=None):
        ((folder or self.module) / "main.swift").write_text(text)

    def test_first_build_is_stored_and_the_next_ones_reuse_it(self):
        first, printed = self.build("first")
        self.assertFalse(first.reused)
        self.assertRegex(first.key, r"^[0-9a-f]{16}$")
        self.assertRegex(first.app_key, r"^[0-9a-f]{16}$")
        self.assertEqual(printed, f"built app part {first.app_key}\nbuilt {first.key}\n")
        self.assertEqual(first.entry, self.scratch / "cache" / first.key)
        self.assertEqual(sorted(child.name for child in first.entry.iterdir()),
                         ["build.log", "manifest.json", "probe"])
        app_entry = self.scratch / "cache" / f"app-{first.app_key}"
        self.assertLessEqual({"Doctored.swiftmodule", "libDoctoredApp.a", "manifest.json", "build.log"},
                             {child.name for child in app_entry.iterdir()})
        self.assertEqual(self.run_probe("first"), "value 41 note one\n")
        sections = subprocess.run(["xcrun", "otool", "-l", str(self.scratch / "first")], stdout=subprocess.PIPE,
                                  text=True, check=True).stdout
        self.assertIn("__info_plist", sections)
        self.assertTrue(self.log.read_text().startswith("$ xcrun swiftc -Onone -whole-module-optimization "))

        second, printed = self.build("second")
        self.assertTrue(second.reused)
        self.assertEqual(second.key, first.key)
        self.assertEqual(printed, f"reused build {first.key}\n")
        self.assertEqual(self.run_probe("second"), "value 41 note one\n")
        self.assertIn(f"reused build {first.key} ({first.entry})\n", self.log.read_text())

        # The same contents at another path outside the repository: the same key, and a hit.
        elsewhere = self.scratch / "elsewhere"
        shutil.copytree(self.module, elsewhere)
        self.assertEqual(probe_build.cache_key(**inputs_of(elsewhere)), first.key)
        third, _ = self.build("third", **inputs_of(elsewhere))
        self.assertTrue(third.reused)
        self.assertEqual(third.key, first.key)
        self.assertEqual(self.probe_entries(), [first.key])
        self.assertEqual(self.app_entries(), [f"app-{first.app_key}"])

    def test_key_changes_with_every_input(self):
        base = probe_build.cache_key(**self.inputs)
        self.assertEqual(probe_build.cache_key(**self.inputs), base)
        seen = {base}

        def expect_new(label, **changes):
            key = probe_build.cache_key(**{**self.inputs, **changes})
            self.assertNotIn(key, seen, label)
            seen.add(key)

        expect_new("defines", defines=("EXTRA",))
        expect_new("optimization", optimization="-O")
        expect_new("link_library", link_library="file")
        expect_new("no Info.plist", info_plist=None)
        expect_new("no object", objects=[])
        for name in ("helper.swift", "main.swift", "bridge.h", "inner/core.h", "lib/libdoctored_core.a",
                     "Info.plist", "extra.o"):
            with (self.module / name).open("ab") as stream:
                stream.write(b"\n")
            expect_new(name)

    def test_an_include_that_does_not_resolve_is_an_error(self):
        (self.module / "inner/core.h").unlink()
        with self.assertRaises(probe_build.ProbeBuildError):
            probe_build.cache_key(**self.inputs)

    def test_file_inside_the_root_and_its_copy_outside_have_different_keys(self):
        outside = self.scratch / "outside"
        shutil.copytree(self.module, outside)
        with mock.patch.object(probe_build, "ROOT", self.module):
            inside_key = probe_build.cache_key(**self.inputs)
            outside_key = probe_build.cache_key(**inputs_of(outside))
        self.assertNotEqual(inside_key, outside_key)

    def test_same_files_under_two_roots_have_different_keys(self):
        moved = self.scratch / "moved"
        shutil.copytree(self.module, moved)
        with mock.patch.object(probe_build, "ROOT", self.module):
            first_key = probe_build.cache_key(**self.inputs)
        with mock.patch.object(probe_build, "ROOT", moved):
            moved_key = probe_build.cache_key(**inputs_of(moved))
        self.assertNotEqual(first_key, moved_key)

    def test_dylib_or_tbd_beside_the_archive_is_an_error_in_search_mode(self):
        for name in ("libdoctored_core.dylib", "libdoctored_core.tbd"):
            beside = self.module / "lib" / name
            beside.write_bytes(b"")
            with self.assertRaises(probe_build.ProbeBuildError, msg=name):
                probe_build.cache_key(**self.inputs)
            probe_build.cache_key(**{**self.inputs, "link_library": "file"})
            beside.unlink()

    def test_scratch_folder_of_a_dead_build_is_removed(self):
        ended = subprocess.Popen(["/usr/bin/true"])
        ended.wait()
        cache = self.scratch / "cache"
        dead = cache / f"tmp-{ended.pid}-00000000"
        live = cache / f"tmp-{os.getpid()}-00000000"
        for folder in (dead, live):
            folder.mkdir(parents=True)
            (folder / "probe").write_bytes(b"partial")
        probe_build._prune()
        self.assertEqual(self.entries(), [live.name])

    def test_a_probe_only_change_reuses_the_app_part(self):
        first, _ = self.build("first")
        self.write_probe('print(greeting() + " again")\n')
        second, printed = self.build("second")
        self.assertNotEqual(second.key, first.key)
        self.assertEqual(second.app_key, first.app_key)
        self.assertEqual(printed, f"reused app part {first.app_key}\nbuilt {second.key}\n")
        self.assertEqual(self.app_entries(), [f"app-{first.app_key}"])
        self.assertEqual(self.run_probe("second"), "value 41 note one again\n")

    def test_an_app_source_change_rebuilds_the_app_part(self):
        first, _ = self.build("first")
        (self.module / "helper.swift").write_text(HELPER.replace("{note}", "two"))
        second, printed = self.build("second")
        self.assertFalse(second.reused)
        self.assertNotEqual(second.key, first.key)
        self.assertNotEqual(second.app_key, first.app_key)
        self.assertEqual(printed, f"built app part {second.app_key}\nbuilt {second.key}\n")
        self.assertEqual(self.run_probe("first"), "value 41 note one\n")
        self.assertEqual(self.run_probe("second"), "value 41 note two\n")
        self.assertEqual(self.probe_entries(), sorted([first.key, second.key]))
        self.assertEqual(self.app_entries(), sorted([f"app-{first.app_key}", f"app-{second.app_key}"]))

    def test_two_builds_racing_on_one_app_key_compile_it_once(self):
        racer = self.scratch / "racer"
        racer.mkdir()
        self.write_probe('print("racer " + greeting())\n', racer)
        racing = [self.child("one"), self.child("two", {**self.inputs, "sources": [racer / "main.swift"]})]
        outputs = [process.communicate(timeout=TIMEOUT_S)[0] for process in racing]
        for process, output in zip(racing, outputs):
            self.assertEqual(process.returncode, 0, output)
        printed = "".join(outputs)
        self.assertEqual(printed.count("built app part "), 1, printed)
        self.assertEqual(printed.count("reused app part "), 1, printed)
        self.assertEqual(len(self.app_entries()), 1)
        self.assertEqual(self.run_probe("one"), "value 41 note one\n")
        self.assertEqual(self.run_probe("two"), "racer value 41 note one\n")

    def test_a_copy_of_the_app_sources_elsewhere_reuses_the_app_part(self):
        first, _ = self.build("first")
        elsewhere = self.scratch / "elsewhere"
        shutil.copytree(self.module, elsewhere)
        self.write_probe('print("elsewhere " + greeting())\n', elsewhere)
        second, printed = self.build("second", **inputs_of(elsewhere))
        self.assertEqual(printed, f"reused app part {first.app_key}\nbuilt {second.key}\n")
        self.assertEqual(self.run_probe("second"), "elsewhere value 41 note one\n")

    def test_an_app_type_reached_only_by_name_is_linked(self):
        # A source of its own, so its object is an archive member the probe never names by symbol.
        (self.module / "named.swift").write_text('import Foundation\n@objc(ProbeBuildNamed) final class Named: '
                                                 'NSObject {}\n')
        self.write_probe('import Foundation\nprint(NSClassFromString("ProbeBuildNamed") == nil ? "missing" : '
                         '"linked")\n')
        self.build("probe", app_sources=[self.module / "helper.swift", self.module / "named.swift"])
        self.assertEqual(self.run_probe("probe"), "linked\n")

    def test_an_optimized_build_is_one_compile(self):
        built, printed = self.build("probe", optimization="-O")
        self.assertIsNone(built.app_key)
        self.assertEqual(printed, f"built {built.key}\n")
        self.assertEqual(self.app_entries(), [])
        commands = [line for line in self.log.read_text().splitlines() if line.startswith("$ ")]
        self.assertEqual(len(commands), 1, commands)
        self.assertTrue(commands[0].startswith("$ xcrun swiftc -O -whole-module-optimization -module-name Doctored "))
        self.assertIn(f" {self.module / 'helper.swift'} {self.module / 'main.swift'} ", commands[0])
        self.assertEqual(self.run_probe("probe"), "value 41 note one\n")

    def test_a_probe_sharing_a_basename_with_an_app_source_builds_threaded(self):
        (self.module / "probe").mkdir()
        (self.module / "probe/Helper.swift").write_text('func probeNote() -> String { "note" }\n')
        built, printed = self.build("probe", sources=[self.module / "main.swift", self.module / "probe/Helper.swift"])
        self.assertNotIn("no -num-threads", printed)
        command = (built.entry / "build.log").read_text().splitlines()[0]
        self.assertIn(" -num-threads ", command)
        self.assertIn(" -module-name DoctoredProbe ", command)
        self.assertEqual(self.run_probe("probe"), "value 41 note one\n")

    def test_an_app_part_in_use_is_not_pruned_and_older_ones_are(self):
        cache = self.scratch / "cache"
        with mock.patch.object(probe_build, "KEEP_APP_ENTRIES", 1):
            first, _ = self.build("first")
            with open(cache / f"app-{first.app_key}.lock", "a") as held:
                fcntl.flock(held, fcntl.LOCK_SH)     # a probe of another process compiling or linking against it
                (self.module / "helper.swift").write_text(HELPER.replace("{note}", "two"))
                second, _ = self.build("second")
                self.assertEqual(self.app_entries(), sorted([f"app-{first.app_key}", f"app-{second.app_key}"]))
            (self.module / "helper.swift").write_text(HELPER.replace("{note}", "three"))
            third, _ = self.build("third")
        self.assertEqual(self.app_entries(), [f"app-{third.app_key}"])
        self.assertEqual(sorted(path.name for path in cache.glob("app-*.lock")), [f"app-{third.app_key}.lock"])
        self.assertEqual(self.run_probe("third"), "value 41 note three\n")

    def test_an_app_part_found_complete_is_not_pruned_before_its_probe_links(self):
        first, _ = self.build("first")
        self.write_probe('print(greeting() + " again")\n')
        is_app_entry = probe_build._is_app_entry
        pruned = []

        def found_then_pruned(entry):
            # Another process's prune, with nothing to keep, right after the build first finds the part.
            found = is_app_entry(entry)
            if not pruned:
                pruned.append(entry)
                with mock.patch.object(probe_build, "KEEP_APP_ENTRIES", 0):
                    probe_build._prune()
            return found

        with mock.patch.object(probe_build, "_is_app_entry", found_then_pruned):
            second, printed = self.build("second")
        self.assertEqual(printed, f"reused app part {first.app_key}\nbuilt {second.key}\n")
        self.assertEqual(self.run_probe("second"), "value 41 note one again\n")

    def test_the_deployment_target_the_command_names_shares_the_app_part(self):
        environment = dict(os.environ)
        environment.pop("MACOSX_DEPLOYMENT_TARGET", None)
        first, _ = self.build("first", env=environment)
        second, printed = self.build("second", env=dict(environment, MACOSX_DEPLOYMENT_TARGET="15.0"))
        self.assertEqual(printed, f"reused app part {first.app_key}\nbuilt {second.key}\n")
        third, printed = self.build("third", env=dict(environment, MACOSX_DEPLOYMENT_TARGET="14.0"))
        self.assertNotEqual(third.app_key, first.app_key)
        self.assertEqual(printed, f"built app part {third.app_key}\nbuilt {third.key}\n")

    def test_only_the_newest_entries_are_kept(self):
        with mock.patch.object(probe_build, "KEEP_ENTRIES", 2):
            keys = [self.build(f"probe-{define}", defines=(define,))[0].key for define in ("A", "B", "C")]
        self.assertEqual(len(set(keys)), 3)
        self.assertEqual(self.probe_entries(), sorted(keys[1:]))
        self.assertEqual(len(self.app_entries()), 1)

    def test_source_rewritten_while_the_compiler_runs_stores_nothing(self):
        compile_once = probe_build._compile

        def compile_then_rewrite(*arguments, **keywords):
            compile_once(*arguments, **keywords)
            (self.module / "helper.swift").write_text(HELPER.replace("{note}", "rewritten"))

        with mock.patch.object(probe_build, "_compile", compile_then_rewrite), \
                self.assertRaises(probe_build.ProbeBuildError):
            self.build("probe")
        self.assertEqual(self.probe_entries(), [])
        self.assertEqual(self.app_entries(), [])
        self.assertFalse((self.scratch / "probe").exists())

    def test_build_only_exits_after_delivering_the_probe(self):
        only = self.child("only", build_only=True)
        printed = only.communicate(timeout=TIMEOUT_S)[0]
        self.assertEqual(only.returncode, 0, printed)
        self.assertIn(f"build only: {self.scratch / 'only'}\n", printed)
        self.assertNotIn("after build", printed)
        self.assertTrue((self.scratch / "only").is_file())
        self.assertIn(f"build only: {self.scratch / 'only'}\n", (self.scratch / "only.log").read_text())

        full = self.child("full")
        printed = full.communicate(timeout=TIMEOUT_S)[0]
        self.assertEqual(full.returncode, 0, printed)
        self.assertIn("after build", printed)
        self.assertNotIn("build only:", printed)

    def test_a_module_cache_made_at_another_path_is_replaced(self):
        # Clang records the module cache's absolute path, so a copy at another path (a worktree's `build/`
        # copied from another checkout) fails every compile until it is made again.
        self.build("first")
        original = self.scratch / "module-cache"
        (original / "sentinel").write_text("kept")
        copied = self.scratch / "copied-module-cache"
        shutil.copytree(original, copied)
        (self.module / "helper.swift").write_text(HELPER.replace("{note}", "two"))
        with mock.patch.object(probe_build, "MODULE_CACHE", copied):
            try:
                second, _ = self.build("second")
            except subprocess.CalledProcessError:
                self.fail("the compile with the copied module cache failed:\n" + self.log.read_text()[-1500:])
        self.assertFalse(second.reused)
        self.assertEqual(self.run_probe("second"), "value 41 note two\n")
        self.assertFalse((copied / "sentinel").exists())
        # A cache used at the path it was made at is kept.
        (self.module / "helper.swift").write_text(HELPER.replace("{note}", "three"))
        third, _ = self.build("third")
        self.assertFalse(third.reused)
        self.assertEqual(self.run_probe("third"), "value 41 note three\n")
        self.assertEqual((original / "sentinel").read_text(), "kept")

    def test_compiler_failure_raises_called_process_error_and_keeps_its_output(self):
        (self.module / "helper.swift").write_text("func greeting() -> String { 1 }\n")
        with self.assertRaises(subprocess.CalledProcessError):
            self.build("probe")
        self.assertEqual(self.probe_entries(), [])
        self.assertEqual(self.app_entries(), [])
        self.assertIn("error:", self.log.read_text())


class RemakeIfMovedTests(unittest.TestCase):
    """`remake_if_moved`, the module caches' one rule, on plain folders: no compiler."""

    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(prefix="rz-remake-if-moved-test-")).resolve()
        self.addCleanup(shutil.rmtree, self.scratch, ignore_errors=True)
        # Neither the lock's folder nor the marker's exists yet, as in a checkout with no `build/`.
        self.here = self.scratch / "build/scratch"
        self.marker = self.here / "made-at"
        self.lock = self.scratch / "build/scratch.lock"
        self.caches = [self.here / "triple/debug/ModuleCache", self.here / "triple/release/ModuleCache"]

    def fill(self):
        for cache in self.caches:
            cache.mkdir(parents=True, exist_ok=True)
            (cache / "sentinel").write_text("old")

    def remake(self, caches=None):
        return probe_build.remake_if_moved(self.marker, self.here, self.caches if caches is None else caches,
                                           self.lock)

    def test_caches_made_at_another_path_are_removed_once_then_kept(self):
        self.fill()
        self.marker.write_text("/elsewhere/build/scratch")
        # A generator is read under the lock.
        self.assertEqual(self.remake(self.here.glob("*/*/ModuleCache")), "/elsewhere/build/scratch")
        for cache in self.caches:
            self.assertFalse(cache.exists(), cache)
        self.assertEqual(self.marker.read_text(), str(self.here))
        self.fill()
        self.assertIsNone(self.remake())
        for cache in self.caches:
            self.assertEqual((cache / "sentinel").read_text(), "old")

    def test_an_absent_marker_removes_the_caches_and_makes_the_folders(self):
        self.assertEqual(self.remake(), "")
        self.assertTrue(self.lock.is_file())
        self.assertEqual(self.marker.read_text(), str(self.here))
        self.fill()
        self.marker.unlink()
        self.assertEqual(self.remake(), "")
        for cache in self.caches:
            self.assertFalse(cache.exists(), cache)

    def test_the_agent_check_removes_each_configuration_s_module_cache_and_nothing_else(self):
        spec = importlib.util.spec_from_file_location("agent_module_cache", HERE / "agent_module_cache.py")
        agent = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(agent)
        scratch = self.scratch / "build/agent-swift"
        caches = [scratch / f"arm64-apple-macosx/{name}/ModuleCache" for name in ("debug", "release")]
        for cache in caches:
            cache.mkdir(parents=True)
            # SwiftPM's `debug` and `release` links at the root.
            (scratch / cache.parent.name).symlink_to(f"arm64-apple-macosx/{cache.parent.name}")
        kept = scratch / "checkouts/package/ModuleCache"
        kept.mkdir(parents=True)
        (scratch / "build.db").write_text("kept")
        marker = scratch / "module-cache-made-at"
        marker.write_text("/elsewhere/build/agent-swift")
        printed = io.StringIO()
        with mock.patch.multiple(agent, SCRATCH=scratch, MARKER=marker,
                                 LOCK=self.scratch / "build/agent-swift.module-cache.lock"):
            with contextlib.redirect_stdout(printed):
                self.assertEqual(agent.main(), 0)
                self.assertEqual(agent.main(), 0)
        self.assertEqual(printed.getvalue(), "agent_module_cache: build/agent-swift was made at "
                         "/elsewhere/build/agent-swift; its module caches are made again\n")
        for cache in caches:
            self.assertFalse(cache.exists(), cache)
            self.assertTrue(cache.parent.is_dir(), cache.parent)
        self.assertTrue(kept.is_dir())
        self.assertEqual((scratch / "build.db").read_text(), "kept")
        self.assertEqual(marker.read_text(), str(scratch))


class TypeCheckTests(unittest.TestCase):
    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(prefix="rz-probe-check-test-")).resolve()
        self.addCleanup(shutil.rmtree, self.scratch, ignore_errors=True)
        for name, folder in (("CACHE", "cache"), ("MODULE_CACHE", "module-cache")):
            patcher = mock.patch.object(probe_build, name, self.scratch / folder)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.module = self.scratch / "module"
        inputs = write_module(self.module)
        self.inputs = dict(app_sources=inputs["app_sources"], bridging_header=inputs["bridging_header"],
                           env=inputs["env"])

    def check(self, text, **changes):
        """The type check's status for a probe of `text`."""
        probe = self.scratch / "main.swift"
        probe.write_text(text)
        with contextlib.redirect_stdout(io.StringIO()):
            return probe_build.typecheck_sources(sources=[probe], **{**self.inputs, **changes})

    def test_the_type_check_fails_on_a_probe_error_and_passes_a_clean_probe(self):
        self.assertNotEqual(self.check('let value: Int = greeting()\n'), 0)
        self.assertEqual(self.check('print(greeting())\n'), 0)

    def test_a_one_module_type_check_refuses_a_redeclared_app_name(self):
        # The probe's own declaration shadows the app part's, but in one module it is a redeclaration.
        redeclared = 'func greeting() -> String { "probe" }\nprint(greeting())\n'
        self.assertNotEqual(self.check(redeclared, optimization="-O"), 0)
        self.assertEqual(self.check(redeclared), 0)


class TakeBuildOnlyTests(unittest.TestCase):
    def test_it_takes_the_variable_from_both_environments(self):
        with mock.patch.dict(os.environ, {probe_build.BUILD_ONLY_ENV: "1"}):
            child = {probe_build.BUILD_ONLY_ENV: "1", "OTHER": "kept"}
            self.assertTrue(probe_build.take_build_only(child))
            self.assertNotIn(probe_build.BUILD_ONLY_ENV, os.environ)
            self.assertEqual(child, {"OTHER": "kept"})
            # Taken once: the build that follows runs to its end in this process.
            self.assertFalse(probe_build.take_build_only())

    def test_any_other_value_is_not_build_only(self):
        for value in ("0", "", "yes"):
            with mock.patch.dict(os.environ, {probe_build.BUILD_ONLY_ENV: value}):
                self.assertFalse(probe_build.take_build_only({}))
                self.assertNotIn(probe_build.BUILD_ONLY_ENV, os.environ)


class CaseClashTests(unittest.TestCase):
    def test_sources_sharing_a_case_folded_basename_clash(self):
        clashing = [Path("app/Sources/Selection.swift"), Path("app/Sources/Workspace.swift"),
                    Path("scripts/probe/selection.swift"), Path("scripts/probe/main.swift")]
        self.assertEqual(probe_build.case_clashes(clashing), ["selection.swift"])

    def test_distinct_basenames_do_not_clash(self):
        distinct = [Path("app/Sources/Selection.swift"), Path("scripts/probe/selections.swift"),
                    Path("scripts/probe/main.swift")]
        self.assertEqual(probe_build.case_clashes(distinct), [])


if __name__ == "__main__":
    unittest.main()
