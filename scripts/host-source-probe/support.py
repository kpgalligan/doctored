"""Owned app processes and explicit document calls for Phase 8F probes."""
import json
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import phase5_support
from phase5_support import App, wait_for
from verify_phase8b import source_call
# The historical helper imports insert their own fixture directory. Restore
# this probe's local namespace before callers import its verification helpers.
sys.path.insert(0, str(Path(__file__).resolve().parent))


class HostApp(App):
    owners = []

    def __init__(self, folder, recovery, executable):
        super().__init__(folder, recovery, interval=600)
        self.executable = executable

    # Every preset library the app reads, and DR_PRINTERS_DIR, is redirected
    # into this owner's own folder, so a probe can never read or write the
    # user's real libraries. The one list lives in `verification_isolation`,
    # re-exported by `phase5_support`.
    PRESET_DIRECTORIES = phase5_support.PRESET_DIRECTORIES

    def __enter__(self):
        self.owners.append(self)
        previous_app = phase5_support.APP
        previous = {name: os.environ.get(name) for name in self.PRESET_DIRECTORIES}
        phase5_support.APP = self.executable
        for name, folder in self.PRESET_DIRECTORIES.items():
            os.environ[name] = str(self.folder / folder)
        try:
            return super().__enter__()
        finally:
            phase5_support.APP = previous_app
            for name, value in previous.items():
                if value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value


def call_for(app, document, trace):
    trace = Path(trace)
    trace.parent.mkdir(parents=True, exist_ok=True)

    def call(tool, fail=False, **arguments):
        record = {'tool': tool, 'arguments': arguments, 'expected_refusal': fail}
        try:
            result = source_call(app, document, tool, fail=fail, **arguments)
            record['result'] = result
            return result
        except BaseException as error:
            record['error'] = repr(error)
            raise
        finally:
            with trace.open('a') as output:
                output.write(json.dumps(record) + '\n')
    return call
