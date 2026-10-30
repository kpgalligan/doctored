#!/usr/bin/env python3
"""Drive: recovery autosave yields the worker slot to waiting MCP calls.

At HEAD a recovery tick polled a finished write and began the next due one in
the same main-thread turn, so with many dirty documents the slot was never
seen free and a worker-backed call refused "Another background operation is
running" after its 5 s wait. The expectations are fixed here before anything
is measured:

- **Yield.** With 24 dirty documents writing back to back (interval 1 s),
  three 16-bit `fill` calls succeed, and then every open document reaches
  `protected` with nothing pending within 60 s.
- **No starvation.** All 24 documents are dirtied again and a back-to-back
  stream of 16-bit `fill` calls runs for longer than the interval. A recovery
  write still begins while the stream runs (a document reads `writing`, or its
  captured serial advances, between two of the stream's calls), and no call
  is refused.

The instance is owned by this file (`phase5_support.App`: a free port, a dummy
API key, private recovery, Actions and preference folders) and only it is
terminated. `--receipts <dir>` keeps `report.json` there.
"""
import argparse
from pathlib import Path
import sys
import tempfile
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from phase5_support import App, fixture, wait_for, write_report

DOCUMENTS = 24
INTERVAL = 1
FILLS = 3
SETTLE_SECONDS = 60
STREAM_SECONDS = 4 * INTERVAL
COLOURS = ['#E03020', '#2040E0']


def open_states(app):
    return {entry['document_id']: entry for entry in app.call('get_recovery_status')['open_documents']}


def fill(app, document, index):
    # `Client.call` asserts the reply is not an error.
    app.call('fill', document_id=document, x=5, y=5, color=COLOURS[index % 2])


def settled(states):
    return all(entry.get('state') == 'protected' and not entry.get('pending') for entry in states.values())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='recovery-yield-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    report = {'documents': DOCUMENTS, 'interval_seconds': INTERVAL}
    with App(folder / 'app', folder / 'recovery', interval=INTERVAL) as app:
        created = app.call('new_document', width=64, height=64, depth=16, background='white')
        deep = created['document']['id']
        documents = []
        for index in range(DOCUMENTS):
            path = folder / f'fixture-{index:02d}.png'
            fixture(path)
            document = app.open(path)
            app.call('set_layer_properties', document_id=document, index=0, name=f'Dirty {index}')
            documents.append(document)

        # Yield: three worker-backed calls while recovery writes back to back.
        writing = wait_for(lambda: open_states(app),
                           lambda states: any(entry.get('state') == 'writing' for entry in states.values()),
                           timeout=20, detail='a recovery write in progress')
        report['writing_before_fills'] = sorted(key for key, entry in writing.items()
                                                if entry.get('state') == 'writing')
        for index in range(FILLS):
            fill(app, deep, index)
        report['fills'] = FILLS
        states = wait_for(lambda: open_states(app), settled, timeout=SETTLE_SECONDS,
                          detail='every open document protected')
        report['protected_after_yield'] = len(states)
        assert len(states) == DOCUMENTS + 1, states

        # No starvation: a stream longer than the interval still lets a write begin.
        captured = {key: entry.get('captured_serial') for key, entry in states.items()}
        for index, document in enumerate(documents):
            app.call('set_layer_properties', document_id=document, index=0, name=f'Again {index}')
        began = None
        calls = 0
        started = time.monotonic()
        while time.monotonic() - started < STREAM_SECONDS:
            fill(app, deep, calls)
            calls += 1
            current = open_states(app)
            if began is None:
                for key, entry in current.items():
                    if key == deep:
                        continue
                    if entry.get('state') == 'writing' or entry.get('captured_serial') != captured.get(key):
                        began = {'document_id': key, 'after_calls': calls,
                                 'seconds': round(time.monotonic() - started, 3)}
                        break
        report['stream'] = {'calls': calls, 'seconds': STREAM_SECONDS, 'write_began': began}
        assert began is not None, ('no recovery write began during the call stream', report)
        states = wait_for(lambda: open_states(app), settled, timeout=SETTLE_SECONDS,
                          detail='every open document protected after the stream')
        report['protected_after_stream'] = len(states)
    write_report(output / 'report.json', report)
    print(f'report: {output / "report.json"}', flush=True)
    print('PASS', flush=True)


if __name__ == '__main__':
    main()
