#!/usr/bin/env python3
"""Reproduce the fixed GIMP corpus in a new private application state directory."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

EXPECTED_EXECUTABLE = 'a771abf6241f43796dd843d332d968fe943da9e981c61adde7a0db1c21b79764'
EXPECTED_P3 = '0ff6958f98684c61f6bbdce1368ddeaf3873baf84545baba482e920d92a914c0'
CASES = {'rgb_mask', 'rgb_mask_disabled', 'rgb_partial_alpha', 'rgb_offcanvas',
         'rgb_group_isolated', 'rgb_group_pass', 'rgb_nested_hidden', 'gray_mask',
         'rgb_p3_ppi', 'rgb_saved_channel'}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def dependencies():
    """The code files `input_hashes` records: the GIMP-side script and this runner."""
    runner = Path(__file__).resolve()
    return [runner.with_name('author_gimp.py'), runner]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gimp', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--state', type=Path, required=True)
    args = parser.parse_args()
    executable, output, state = (p.resolve() for p in (args.gimp, args.output, args.state))
    script, runner = dependencies()
    p3 = Path('/System/Library/ColorSync/Profiles/Display P3.icc')
    assert not output.exists() and not state.exists(), 'Use fresh output and private state directories'
    assert digest(executable) == EXPECTED_EXECUTABLE, 'The receipt fixes the official ARM64 GIMP3.2.6 executable'
    assert digest(p3) == EXPECTED_P3, 'The authoring script fixes its Display P3 source profile'
    inputs = {str(p): digest(p) for p in (executable, script, p3, runner)}
    state.mkdir(parents=True)
    env = os.environ.copy()
    env.update(GIMP3_DIRECTORY=str(state / 'config'), GIMP3_CACHEDIR=str(state / 'cache'),
               GIMP3_TEMPDIR=str(state / 'tmp'), DR_PSD_EXCHANGE_OUTPUT=str(output))
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
    assert {p.name for p in output.iterdir()} == CASES, 'Incomplete or extra corpus cases'
    artifacts = {}
    for folder in sorted(output.iterdir()):
        manifest = json.loads((folder / 'manifest.json').read_text())
        assert manifest['application'] == {'name': 'GIMP', 'version': '3.2.6'}
        assert set(manifest['files']) == {p.name for p in folder.iterdir() if p.name != 'manifest.json'}
        for name, sha in manifest['files'].items():
            assert digest(folder / name) == sha
        artifacts.update({str(p.relative_to(output)): digest(p) for p in sorted(folder.iterdir())})
    assert all(digest(Path(path)) == sha for path, sha in inputs.items()), 'Authoring inputs changed during execution'
    receipt = dict(application='GIMP3.2.6', owned_pid=child.pid, exit_code=code,
                   input_hashes=inputs, artifacts=artifacts, command=command)
    (state / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(dict(cases=len(CASES), files=len(artifacts), receipt=str(state / 'receipt.json'))))


if __name__ == '__main__':
    main()
