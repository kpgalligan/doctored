#!/usr/bin/env python3
"""Capture merged-only GIMP references from a frozen existing PSD corpus."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from run_author import CASES, EXPECTED_EXECUTABLE, digest


def dependencies():
    """The code files the hashed inputs include: the GIMP-side script, this runner and `run_author.py`."""
    runner = Path(__file__).resolve()
    return [runner.with_name('capture_merged_gimp.py'), runner, runner.with_name('run_author.py')]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for argument in ('gimp', 'source', 'output', 'state'):
        parser.add_argument('--' + argument, type=Path, required=True)
    args = parser.parse_args()
    executable, source, output, state = (getattr(args, key).resolve() for key in ('gimp', 'source', 'output', 'state'))
    assert not output.exists() and not state.exists(), 'Use fresh output and private state'
    assert {p.name for p in source.iterdir()} == CASES
    assert digest(executable) == EXPECTED_EXECUTABLE
    code = dependencies()
    script = code[0]
    inputs = [executable, *code]
    inputs += [p for p in source.rglob('*') if p.is_file()]
    hashes = {str(p): digest(p) for p in inputs}
    state.mkdir(parents=True)
    env = os.environ.copy()
    env.update(GIMP3_DIRECTORY=str(state / 'config'), GIMP3_CACHEDIR=str(state / 'cache'),
               GIMP3_TEMPDIR=str(state / 'tmp'), DR_PSD_MERGED_SOURCE=str(source),
               DR_PSD_MERGED_OUTPUT=str(output))
    batch = f'exec(compile(open({str(script)!r}).read(), {str(script)!r}, "exec"))'
    command = [str(executable), '--new-instance', '--no-interface', '--no-data', '--no-fonts',
               '--console-messages', '--batch-interpreter', 'python-fu-eval', '--batch', batch, '--quit']
    with (state / 'gimp.log').open('wb') as log:
        child = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            code = child.wait(timeout=180)
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
    assert code == 0, f'GIMP failed with {code}; see {state / "gimp.log"}'
    assert {p.name for p in output.iterdir()} == CASES
    for folder in sorted(output.iterdir()):
        manifest = json.loads((folder / 'manifest.json').read_text())
        assert manifest['loader'] == 'file-psd-load-merged'
        assert manifest['source_psd_sha256'] == digest(source / folder.name / 'source.psd')
        assert set(manifest['files']) == {'merged.png', 'merged.icc'}
        assert {p.name for p in folder.iterdir()} == {'manifest.json', *manifest['files']}
        assert all(digest(folder / name) == sha for name, sha in manifest['files'].items())
    assert all(digest(Path(path)) == sha for path, sha in hashes.items()), 'An original or capture input changed'
    artifacts = {str(p.relative_to(output)): digest(p) for p in output.rglob('*') if p.is_file()}
    receipt = dict(application='GIMP3.2.6', loader='file-psd-load-merged', owned_pid=child.pid,
                   exit_code=code, input_hashes=hashes, artifact_hashes=artifacts, command=command)
    (state / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(dict(cases=len(CASES), files=len(artifacts), receipt=str(state / 'receipt.json'))))


if __name__ == '__main__':
    main()
