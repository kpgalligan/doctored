#!/usr/bin/env python3
"""Every scripts provenance manifest hashes every file its script loads.

A verifier's source manifest is the record of which code produced a receipt.
This check walks each registered script's static Python load chain (imports,
`spec_from_file_location`, `run_path` and the repo's own path-taking loader
helpers) plus the `core/src/*.rs` files a chain reads by literal path, then
asks the script for its manifest in a fresh child process and reports every
chain file the manifest does not hash. Run with `python3 -B`.
"""
import ast
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / 'scripts'

_DEPENDENCIES = ['verify_phase8c', 'verify_phase8d', 'verify_phase8e',
                 'verify_phase9d', 'verify_phase9d_native', 'verify_phase9e', 'verify_phase9e_lifecycle',
                 'verify_phase9f', 'verify_phase9g', 'verify_phase9g_auxiliary', 'verify_phase9g_compat',
                 'verify_phase9h', 'verify_phase9h_compat', 'verify_phase9i_typed']
# (script, manifest callable, call args, files the same report records elsewhere).
ENTRIES: list[tuple[str, str, tuple[str, ...], tuple[str, ...]]] = [
    *((f'scripts/{name}.py', 'dependencies', (), ()) for name in _DEPENDENCIES),
    ('scripts/verify_phase8g.py', 'dependencies', ('all',), ()),
    ('scripts/verify_phase8d_color.py', 'provenance', (), ()),
    # The report's `script_sha256` holds the script itself.
    ('scripts/verify_phase11d_formats.py', 'helper_digests', (), ('scripts/verify_phase11d_formats.py',)),
    ('scripts/precision-probe/verify.py', 'measured_sources', (), ()),
    ('scripts/raw-local-probe/mcp.py', 'manifest', ('@empty-dir', '@empty-dir'), ()),
    *((f'scripts/verify_phase{name}.py', 'dependencies', (), ()) for name in
      ['10_photoshop', '10_photoshop_appearance', '10_photoshop_depth16', '10_receipts', '10a_external',
       '10a_groups', '10a_markers', '10a_masks', '10a_pixels', '10b_external',
       '10d_blending', '10d_five_modes', '10d_four_modes', '10d_separable', '11c_softproof16', '12f3_warp',
       '12f5_paint']),
    ('scripts/color-metadata-probe/verify.py', 'dependencies', (), ()),
    *((f'scripts/psd-exchange-probe/run_{name}.py', 'dependencies', (), ()) for name in
      ['author', 'merged_capture', 'reopen']),
    # The report's `script_sha256` holds the script itself.
    *((f'scripts/verify_phase{name}.py', 'dependencies', (), (f'scripts/verify_phase{name}.py',)) for name in
      ['10d_adjustment_blending', '10d_blend_if', '10d_hsl_modes', '10d_threshold_modes', '11a_export',
       '11b_production', '11c_color']),
    ('scripts/verify_phase8g.py', 'dependencies', ('host',), ()),
]
# (script, call args) -> the chain files that mode never loads; `test_unloaded_files_are_mode_guarded`
# proves each is loaded only under an `args.mode in (...)` test that excludes the mode.
UNLOADED: dict[tuple[str, tuple[str, ...]], tuple[str, ...]] = {
    ('scripts/verify_phase8g.py', ('host',)): ('scripts/raw-local-probe/mcp.py',),
}
# (script, function, manifest variable, files the same report records elsewhere). These scripts are
# never imported or run: their manifest is read from the source (`static_manifest_keys`).
STATIC_ENTRIES: list[tuple[str, str, str, tuple[str, ...]]] = [
    (f'scripts/verify_phase9{phase}_ui.py', 'main', 'inputs', ()) for phase in 'defgh']
# Per file, names that evaluate to a repo-relative path: `frozen` is a digest-checked copy of the repo.
CHAIN_ALIASES: dict[str, dict[str, str]] = {'scripts/color-metadata-probe/verify.py': {'frozen': '.'}}
# Globbed under `scripts/`: every match is registered somewhere below.
SCOPE: tuple[str, ...] = ('verify_phase1[0-3]*.py', 'verify_phase*_ui.py', 'color-metadata-probe/verify.py',
                          'psd-exchange-probe/run_*.py')
NO_CODE_MANIFEST: dict[str, str] = {
    **{f'scripts/verify_phase{name}.py': 'records no code provenance' for name in [
        '10_photoshop_channels', '10a_mask_foundation', '11e_linear', '11f_float', '12a_replay', '12b_actions', '12b2_presets',
        '12c_workspace', '12c2_workspace', '12d1_comps', '12d2_views', '12e1_structure', '12e1b_structure',
        '12e1c_guides', '12e2_adjustments', '12e2b_placement', '12e2c_stamp_flatten', '12e3_knockout',
        '12e3b_typed_knockout', '12e3c_psd', '12f1_geometry', '12f1b_geometry', '12f1b3_rotation',
        '12f2_geometry', '12g_clipboard', '12h_conversion', '13a1_regional', '13a2_masks_clips',
        '13a2b_byte_styles', '13a3_groups', '13a3b_styles', '13a3bii_styled_groups', '13a3c_knockout',
        '13a4_guide', '13a4b_nested_guide', '13a4bii_pass_through', '13a4biib_clip_members',
        '13a4biib2_styled_members', '13a6a_bounded_selection', '13a6b_bounded_artwork',
        '13a6c_bounded_strokes', '13a6c2_stroke_refusals']},
    'scripts/verify_phase10d_blend_decoder.py': 'records no code provenance; runs its checks at import, parsed only',
}
CODE_RECORD_KEYS = {'script_sha256', 'helper_sha256', 'verification_script_sha256', 'shared_helper_sha256',
                    'drive_sha256', 'dependency_sha256', 'source_hashes'}
NOT_CODE_MANIFESTS: dict[str, str] = {'scripts/phase6_full_reference_fixtures.py': 'pinned image references, not code'}
MANIFEST_NAMES = {'dependencies', 'provenance', 'manifest', 'measured_sources', 'helper_digests'}
LOADERS = {'spec_from_file_location': 1, 'run_path': 0}
CORE_RS = re.compile(r'^core/src/[\w/]+\.rs$')


class Unresolved(Exception):
    pass


def evaluate(node, env, here):
    """The value of a path expression, or Unresolved."""
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    if isinstance(node, ast.Name):
        if node.id == '__file__':
            return here
        if node.id in env:
            return evaluate(env[node.id], env, here)
        raise Unresolved(ast.unparse(node))
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Div):
        return Path(evaluate(node.left, env, here)) / evaluate(node.right, env, here)
    if isinstance(node, ast.Call) and callee(node) in ('Path', 'str') and len(node.args) == 1:
        return Path(evaluate(node.args[0], env, here))
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute):
        if node.func.attr == 'with_name' and len(node.args) == 1:
            return Path(evaluate(node.func.value, env, here)).with_name(evaluate(node.args[0], env, here))
        if node.func.attr == 'resolve' and not node.args:
            return Path(evaluate(node.func.value, env, here)).resolve()
    if isinstance(node, ast.Attribute) and node.attr == 'parent':
        return Path(evaluate(node.value, env, here)).parent
    if (isinstance(node, ast.Subscript) and isinstance(node.value, ast.Attribute)
            and node.value.attr == 'parents' and isinstance(node.slice, ast.Constant)):
        return Path(evaluate(node.value.value, env, here)).parents[node.slice.value]
    raise Unresolved(ast.unparse(node))


def environment(tree):
    """Each name bound to the first single-target assignment of it in the file."""
    env = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            env.setdefault(node.targets[0].id, node.value)
    return env


def callee(node):
    function = node.func
    if isinstance(function, ast.Name):
        return function.id
    return function.attr if isinstance(function, ast.Attribute) else None


def parse(path):
    return ast.parse(path.read_text(), str(path))


def loader_path(node):
    """The path argument of a `spec_from_file_location` / `run_path` call, else None."""
    if isinstance(node, ast.Call) and callee(node) in LOADERS and len(node.args) > LOADERS[callee(node)]:
        return node.args[LOADERS[callee(node)]]
    return None


def names_in(node):
    return {n.id for n in ast.walk(node) if isinstance(n, ast.Name)}


def loader_helpers():
    """Functions that pass one of their own parameters to a loader, by name.

    Each value lists (parameter indices and names, the loader's path expression,
    the helper file's environment, the helper file)."""
    helpers = {}
    for file in sorted(SCRIPTS.rglob('*.py')):
        if file.name.startswith('test_'):
            continue
        tree = parse(file)
        env = environment(tree)
        for function in ast.walk(tree):
            if not isinstance(function, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            params = [a.arg for a in function.args.args]
            for node in ast.walk(function):
                expr = loader_path(node)
                used = names_in(expr) & set(params) if expr is not None else set()
                if used:
                    slots = [(params.index(p), p) for p in sorted(used)]
                    helpers.setdefault(function.name, []).append((slots, expr, env, file))
    return helpers


_HELPERS = None


def _helpers():
    global _HELPERS
    if _HELPERS is None:
        _HELPERS = loader_helpers()
    return _HELPERS


def foreign_modules(tree, directories):
    """Names a file binds to a module from outside `scripts/` (`json`, `plistlib`, ...)."""
    def in_repo(module):
        return any((directory / f'{module.split(".")[0]}.py').is_file() for directory in directories)
    names = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            names |= {a.asname or a.name.split('.')[0] for a in node.names if not in_repo(a.name)}
        elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module and not in_repo(node.module):
            names |= {a.asname or a.name for a in node.names}
    return names


def helper_call(node, helper_file, file, foreign):
    """Whether a call by a helper's name can be that helper: a bare name only in the
    helper's own file, an attribute call only on a receiver that is not an outside
    module (so `json.load(a, b)` / `plistlib.load(a, b)` never match)."""
    if isinstance(node.func, ast.Name):
        return helper_file == file
    receiver = node.func.value
    while isinstance(receiver, ast.Attribute):
        receiver = receiver.value
    return isinstance(receiver, ast.Name) and receiver.id not in foreign


def _inside_helper(tree, node, expr):
    """A loader site inside a function whose own parameter names its path."""
    for function in ast.walk(tree):
        if isinstance(function, (ast.FunctionDef, ast.AsyncFunctionDef)) \
                and {a.arg for a in function.args.args} & names_in(expr) \
                and any(n is node for n in ast.walk(function)):
            return True
    return False


def _chain(entry, prune: frozenset[Path] = frozenset()):
    """(required files, unresolved loader sites, {loaded file: 'loader path:line: expr'}).

    Files in `prune` are never walked: neither they nor what only they load are required."""
    required, unresolved, via, _ = _walk(entry, prune)
    return required, unresolved, via


def _walk(entry, prune=frozenset()):
    """`_chain`'s three values plus every load as (loaded file, loading file, loader node)."""
    entry = Path(entry).resolve()
    helpers = _helpers()
    seen, data, unresolved, via, loads = set(), set(), [], {}, []
    pending, path_dirs = [entry], []

    def site(file, node, text):
        return f'{file.relative_to(ROOT)}:{node.lineno}: {text}'

    def target(path, file, node, text):
        path = Path(path)
        if not path.is_absolute() or not path.is_file():
            path = ROOT / path
        path = path.resolve()
        if path.suffix == '.py' and path.is_file():
            pending.append(path)
            via.setdefault(path, site(file, node, text))
            loads.append((path, file, node))
        elif path.suffix == '.py':
            unresolved.append(site(file, node, f'{text} -> {path} is not a file'))

    while pending:
        file = pending.pop()
        if file in seen or file in prune:
            continue
        seen.add(file)
        tree = parse(file)
        env = environment(tree)
        aliases = CHAIN_ALIASES.get(str(file.relative_to(ROOT)), {}) if file.is_relative_to(ROOT) else {}
        env.update({name: ast.Constant(str((ROOT / value).resolve())) for name, value in aliases.items()})
        local_dirs = []
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) \
                    and node.func.attr in ('insert', 'append') and ast.unparse(node.func.value) == 'sys.path':
                try:
                    local_dirs.append(Path(evaluate(node.args[-1], env, file)).resolve())
                except Unresolved:
                    unresolved.append(site(file, node, ast.unparse(node)))
        path_dirs.extend(local_dirs)
        foreign = foreign_modules(tree, [*local_dirs, file.parent, entry.parent, SCRIPTS, *path_dirs])
        for node in ast.walk(tree):
            names = []
            if isinstance(node, ast.Import):
                names = [a.name for a in node.names]
            elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
                names = [node.module]
            for name in names:
                top = name.split('.')[0]
                for directory in [*reversed(local_dirs), file.parent, entry.parent, SCRIPTS, *reversed(path_dirs)]:
                    candidate = directory / f'{top}.py'
                    if candidate.is_file():
                        target(candidate, file, node, f'import {name}')
                        break
            expr = loader_path(node)
            if expr is not None and not _inside_helper(tree, node, expr):
                try:
                    target(evaluate(expr, env, file), file, node, ast.unparse(expr))
                except Unresolved:
                    unresolved.append(site(file, node, ast.unparse(node)))
            if isinstance(node, ast.Call) and callee(node) in helpers:
                for slots, template, helper_env, helper_file in helpers[callee(node)]:
                    if not helper_call(node, helper_file, file, foreign) \
                            or any(len(node.args) <= index for index, _ in slots):
                        continue
                    try:
                        bound = {param: ast.Constant(str(evaluate(node.args[index], env, file)))
                                 for index, param in slots}
                        target(evaluate(template, {**helper_env, **bound}, helper_file), file, node,
                               ast.unparse(node))
                    except Unresolved:
                        unresolved.append(site(file, node, ast.unparse(node)))
            if isinstance(node, ast.Constant) and isinstance(node.value, str) and CORE_RS.match(node.value) \
                    and (ROOT / node.value).is_file():
                data.add((ROOT / node.value).resolve())
                via.setdefault((ROOT / node.value).resolve(), site(file, node, repr(node.value)))
    return seen | data, unresolved, via, loads


def load_chain(entry: Path) -> tuple[set[Path], list[str]]:
    """The files `entry` loads, statically, and the loader sites it cannot resolve."""
    required, unresolved, _ = _chain(entry)
    return required, unresolved


def static_manifest_keys(script: str, function: str, variable: str) -> set[Path]:
    """The resolved keys of the manifest the module-level `function` of `script` assigns to `variable`
    first, read from the source alone. The script is never imported."""
    path = (ROOT / script).resolve()
    tree = parse(path)
    body = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == function)
    local = {}
    for node in body.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            local.setdefault(node.targets[0].id, node.value)
    scope = {**environment(tree), **local}

    def values(node):
        if isinstance(node, (ast.List, ast.Tuple)):
            return [value for element in node.elts for value in values(element)]
        if isinstance(node, ast.Starred):
            return values(node.value)
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
            return values(node.left) + values(node.right)
        if isinstance(node, ast.Name) and node.id in local:
            return values(local[node.id])
        if isinstance(node, ast.Call) and callee(node) == 'sorted' and len(node.args) == 1:
            return values(node.args[0])
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) \
                and node.func.attr in ('glob', 'rglob') and len(node.args) == 1:
            pattern = evaluate(node.args[0], scope, path)
            if not pattern.endswith(('.py', '.rs')):
                return []
            return sorted(getattr(Path(evaluate(node.func.value, scope, path)), node.func.attr)(pattern))
        if isinstance(node, ast.ListComp):
            if values(node.generators[0].iter) == []:
                return []
            raise Unresolved(ast.unparse(node))
        return [Path(evaluate(node, scope, path))]

    return {(key if key.is_absolute() else ROOT / key).resolve() for key in map(Path, values(local[variable]))}


def unloaded_sites(script: str, unloaded: str) -> list[tuple[str, str | None]]:
    """Every loader site in `script`'s unpruned chain that loads `unloaded`, as ('path:line', the
    test of the innermost enclosing `if`, or None). A site in an `else` reports `not (test)`."""
    target = (ROOT / unloaded).resolve()
    sites = []
    for loaded, file, node in _walk(ROOT / script)[3]:
        if loaded != target:
            continue
        guard = None
        for branch in ast.walk(parse(file)):
            if not isinstance(branch, ast.If):
                continue
            for arm, negated in ((branch.body, False), (branch.orelse, True)):
                if any(type(n) is type(node) and n.lineno == node.lineno and n.col_offset == node.col_offset
                       for statement in arm for n in ast.walk(statement)):
                    test = ast.unparse(branch.test)
                    guard = f'not ({test})' if negated else test
        sites.append((f'{file.relative_to(ROOT)}:{node.lineno}', guard))
    return sites


def code_record_keys(source: str) -> set[str]:
    """The `CODE_RECORD_KEYS` a source names, as a string constant or a keyword argument."""
    found = set()
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.Constant) and isinstance(node.value, str) and node.value in CODE_RECORD_KEYS:
            found.add(node.value)
        elif isinstance(node, ast.keyword) and node.arg in CODE_RECORD_KEYS:
            found.add(node.arg)
    return found


def manifest_keys(script: str, function: str, args: tuple[str, ...]) -> set[Path]:
    """The resolved keys of `script`'s manifest, computed in a fresh child."""
    environment_ = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')
    result = subprocess.run([sys.executable, '-B', str(Path(__file__).resolve()), '--manifest', script, function,
                             *args], cwd=ROOT, env=environment_, capture_output=True, text=True, timeout=600)
    if result.returncode:
        raise AssertionError(f'{script}: {function}{tuple(args)} failed:\n{result.stderr[-2000:]}')
    keys = set()
    for key in json.loads(result.stdout.strip().splitlines()[-1]):
        path = Path(key)
        keys.add((path if path.is_absolute() else ROOT / path).resolve())
    return keys


def _print_manifest(script, function, args):
    entry = (ROOT / script).resolve()
    sys.path.insert(0, str(entry.parent))
    spec = importlib.util.spec_from_file_location('__provenance_check__', entry)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    folders = []
    values = []
    for arg in args:
        if arg == '@empty-dir':
            folders.append(Path(tempfile.mkdtemp(prefix='rasterize-provenance-check-')))
            values.append(folders[-1])
        else:
            values.append(arg)
    try:
        manifest = getattr(module, function)(*values)
    finally:
        for folder in folders:
            shutil.rmtree(folder, ignore_errors=True)
    print(json.dumps(sorted(str(key) for key in manifest)))


class ProvenanceManifestTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not (ROOT / 'app/Info.plist').is_file():
            raise AssertionError('app/Info.plist is missing: run `make project` first')

    def test_every_manifest_covers_its_load_chain(self):
        rows = [(script, function, args, elsewhere, lambda s=script, f=function, a=args: manifest_keys(s, f, a))
                for script, function, args, elsewhere in ENTRIES]
        rows += [(script, function, (), elsewhere,
                  lambda s=script, f=function, v=variable: static_manifest_keys(s, f, v))
                 for script, function, variable, elsewhere in STATIC_ENTRIES]
        for script, function, args, elsewhere, keys in rows:
            with self.subTest(script=script, manifest=function, args=args):
                prune = frozenset((ROOT / p).resolve() for p in UNLOADED.get((script, args), ()))
                required, _, via = _chain(ROOT / script, prune)
                recorded = keys() | {(ROOT / p).resolve() for p in elsewhere}
                missing = sorted(required - recorded)
                if missing:
                    self.fail(f'{script} {function}{tuple(args)} misses {len(missing)} loaded file(s):\n' + '\n'.join(
                        f'  {path.relative_to(ROOT)}  <- {via.get(path, script)}' for path in missing))

    def test_every_manifest_is_registered(self):
        registered = {script for script, *_ in ENTRIES} | set(NOT_CODE_MANIFESTS)
        unregistered = []
        for file in sorted(SCRIPTS.rglob('*.py')):
            if file.name.startswith('test_'):
                continue
            names = set()
            for node in parse(file).body:
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    names.add(node.name)
                elif isinstance(node, ast.Assign):
                    names |= {t.id for t in node.targets if isinstance(t, ast.Name)}
            if names & MANIFEST_NAMES and str(file.relative_to(ROOT)) not in registered:
                unregistered.append(f'{file.relative_to(ROOT)}: {sorted(names & MANIFEST_NAMES)}')
        if unregistered:
            self.fail('manifests missing from ENTRIES or NOT_CODE_MANIFESTS:\n' + '\n'.join(unregistered))

    def test_no_unresolved_loaders(self):
        for script in dict.fromkeys(entry[0] for entry in [*ENTRIES, *STATIC_ENTRIES]):
            with self.subTest(script=script):
                _, unresolved = load_chain(ROOT / script)
                if unresolved:
                    self.fail(f'{script}: loader sites this check cannot resolve:\n' + '\n'.join(unresolved))

    def test_every_scoped_script_is_registered(self):
        registered = ({script for script, *_ in ENTRIES} | {script for script, *_ in STATIC_ENTRIES}
                      | set(NO_CODE_MANIFEST) | set(NOT_CODE_MANIFESTS))
        scoped = sorted({str(file.relative_to(ROOT)) for pattern in SCOPE for file in SCRIPTS.glob(pattern)})
        self.assertEqual([script for script in scoped if script not in registered], [],
                         'scoped scripts missing from ENTRIES, STATIC_ENTRIES, NO_CODE_MANIFEST or NOT_CODE_MANIFESTS')
        with self.subTest(case='keyword and constant forms are both found'):
            self.assertEqual(code_record_keys('report = dict(app=1, script_sha256=digest(Path(__file__)))'),
                             {'script_sha256'})
            self.assertEqual(code_record_keys("report = {'drive_sha256': h, 'native_sha256': n}"), {'drive_sha256'})
        for script in NO_CODE_MANIFEST:
            with self.subTest(script=script):
                self.assertTrue((ROOT / script).is_file(), f'{script} does not exist')
                self.assertEqual(code_record_keys((ROOT / script).read_text()), set(),
                                 f'{script} records code provenance: register its manifest instead')

    def test_unloaded_files_are_mode_guarded(self):
        for (script, args), files in UNLOADED.items():
            for unloaded in files:
                with self.subTest(script=script, args=args, unloaded=unloaded):
                    self.assertIn((ROOT / unloaded).resolve(), _chain(ROOT / script)[0])
                    sites = unloaded_sites(script, unloaded)
                    self.assertTrue(sites, f'{unloaded} has no loader site in {script}')
                    for where, guard in sites:
                        self.assertEqual(where.rsplit(':', 1)[0], script, f'{where} is outside the entry script')
                        test = ast.parse(guard, mode='eval').body if guard else None
                        modes = (test.comparators[0].elts
                                 if isinstance(test, ast.Compare) and ast.unparse(test.left) == 'args.mode'
                                 and len(test.ops) == 1 and isinstance(test.ops[0], ast.In)
                                 and isinstance(test.comparators[0], ast.Tuple) else None)
                        self.assertTrue(modes is not None and all(isinstance(m, ast.Constant)
                                                                  and isinstance(m.value, str) for m in modes),
                                        f'{where}: guard {guard!r} is not `args.mode in (<str constants>)`')
                        self.assertNotIn(args[0], [m.value for m in modes], f'{where}: {guard} admits {args[0]}')


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--manifest':
        _print_manifest(sys.argv[2], sys.argv[3], tuple(sys.argv[4:]))
    else:
        unittest.main()
