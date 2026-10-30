#!/usr/bin/env python3
"""Reopen the frozen Doctored-written 10B PSD corpus in a private GIMP 3.2.6."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from run_author import EXPECTED_EXECUTABLE, digest

DEFAULT_CASES = ('layers_basic,groups_masks,flattened,p3_ppi,clipping,'
                 'styled_effects,depth16')


def dependencies():
    """The code files the hashed inputs include: the GIMP-side script, this runner and `run_author.py`."""
    runner = Path(__file__).resolve()
    return [runner.with_name('reopen_gimp.py'), runner, runner.with_name('run_author.py')]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gimp', type=Path, required=True)
    parser.add_argument('--source', type=Path,
                        default=Path(os.environ.get('DR_PSD_REOPEN_SOURCE', '')) or None)
    parser.add_argument('--output', type=Path,
                        default=Path(os.environ.get('DR_PSD_REOPEN_OUTPUT', '')) or None)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--cases', default=DEFAULT_CASES,
                        help='Comma-separated case folder names (default: the 10B corpus).')
    args = parser.parse_args()
    cases = {name for name in args.cases.split(',') if name}
    if args.source is None or args.output is None:
        parser.error('--source and --output (or DR_PSD_REOPEN_SOURCE/OUTPUT) are required')
    executable, source, output, state = (p.resolve() for p in (args.gimp, args.source, args.output, args.state))
    assert not output.exists() and not state.exists(), 'Use fresh output and private state'
    assert {p.name for p in source.iterdir()} == cases, 'Missing or extra corpus cases'
    assert digest(executable) == EXPECTED_EXECUTABLE, 'The receipt fixes the official ARM64 GIMP3.2.6 executable'
    code = dependencies()
    script = code[0]
    inputs = [executable, *code]
    inputs += [p for p in source.rglob('*') if p.is_file()]
    hashes = {str(p): digest(p) for p in inputs}
    state.mkdir(parents=True)
    env = os.environ.copy()
    env.update(GIMP3_DIRECTORY=str(state / 'config'), GIMP3_CACHEDIR=str(state / 'cache'),
               GIMP3_TEMPDIR=str(state / 'tmp'), DR_PSD_REOPEN_SOURCE=str(source),
               DR_PSD_REOPEN_OUTPUT=str(output))
    batch = f'exec(compile(open({str(script)!r}).read(), {str(script)!r}, "exec"))'
    command = [str(executable), '--new-instance', '--no-interface', '--no-data', '--no-fonts',
               '--console-messages', '--batch-interpreter', 'python-fu-eval', '--batch', batch, '--quit']
    with (state / 'gimp.log').open('wb') as log:
        child = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            code = child.wait(timeout=300)
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
    assert code == 0, f'GIMP failed with {code}; see {state / "gimp.log"}'
    assert {p.name for p in output.iterdir()} == cases, 'Incomplete or extra reopened cases'
    failures = {}
    for folder in sorted(output.iterdir()):
        record = json.loads((folder / 'gimp.json').read_text())
        assert record['schema'] == 1 and record['name'] == folder.name
        assert record['application'] == {'name': 'GIMP', 'version': '3.2.6'}
        assert record['source_psd_sha256'] == digest(source / folder.name / 'source.psd')
        assert set(record['files']) == {p.name for p in folder.iterdir() if p.name != 'gimp.json'}
        for name, sha in record['files'].items():
            assert digest(folder / name) == sha
        if record['error']:
            failures[folder.name] = record['error']
    assert all(digest(Path(path)) == sha for path, sha in hashes.items()), 'An input changed during the run'
    artifacts = {str(p.relative_to(output)): digest(p) for p in sorted(output.rglob('*')) if p.is_file()}
    receipt = dict(application='GIMP3.2.6', loaders=['file-psd-load', 'file-psd-load-merged'],
                   owned_pid=child.pid, exit_code=code, input_hashes=hashes,
                   artifact_hashes=artifacts, command=command, case_errors=failures)
    (state / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(dict(cases=len(cases), files=len(artifacts), errors=failures,
                          receipt=str(state / 'receipt.json'))))
    raise SystemExit(1 if failures else 0)


if __name__ == '__main__':
    main()
