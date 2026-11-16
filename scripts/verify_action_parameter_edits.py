#!/usr/bin/env python3
"""MCP drive for editing an action's declared parameters without JSON (`edit_action_parameters`,
the Actions window's Parameters… sheet), against the Debug app.

- **Add and rename in one call**: `unused` (integer, default 3) is added and `sigma` renamed to
  `radius`; the action's step now reads `{"$param": "radius"}`, and `run_action {parameters:
  {radius: 8}}` renders byte for byte as a direct `apply_filter blur sigma 8` (and with no value as
  sigma 4, the default; 8 and 4 differ).
- **Refusals by sentence**: removing `radius` while step 1 uses it names the step; retyping it to
  string with no new default; an unknown op; an edit list whose second edit fails leaves the file's
  bytes exactly as they were.
- **Retype and required**: `radius` retyped to integer keeps its default 4; `set_default null`
  makes it required, so a run without it is refused naming it.

The app is this script's own child (`phase5_support.App`). Prints one PASS line per check; exits
non-zero unless every line is PASS.
"""
import argparse
import hashlib
import os
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
os.environ.setdefault('DR_BACKGROUND', '1')
from phase5_support import App  # noqa: E402

RESULTS = []
NAME = 'Parameter Edit Drive'


def ok(label):
    RESULTS.append(True)
    print(f'PASS: {label}', flush=True)


def check(label, function):
    try:
        function()
        ok(label)
    except Exception as error:  # one failed check must not hide the others
        RESULTS.append(False)
        print(f'FAIL: {label} raised {type(error).__name__} — {str(error)[:700]}', flush=True)


def run(app):
    def picture():
        doc = app.call('new_document', width=120, height=90, background='white')['document']['id']
        app.call('brush_stroke', document_id=doc, points=[[10, 10], [110, 80]], size=9,
                 color='#102030', hardness=100, opacity=1, flow=100)
        return doc

    def digest(doc):
        reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': {'document_id': doc}})
        assert not reply.get('isError'), reply
        data = next(c['data'] for c in reply['content'] if c['type'] == 'image')
        app.call('close_document', document_id=doc, discard_changes=True)
        return hashlib.sha256(data.encode()).hexdigest()

    def played(**parameters):
        doc = picture()
        arguments = {'document_id': doc, 'name': NAME}
        if parameters:
            arguments['parameters'] = parameters
        ran = app.call('run_action', **arguments)
        assert ran.get('ok') is True, ran
        return digest(doc)

    def direct(sigma):
        doc = picture()
        app.call('apply_filter', document_id=doc, filter='blur', sigma=sigma)
        return digest(doc)

    saved = app.call('save_action', name=NAME, overwrite=True, parameters=[
        {'name': 'sigma', 'type': 'number', 'default': 4}], steps=[
        {'tool': 'apply_filter', 'arguments': {'filter': 'blur', 'sigma': {'$param': 'sigma'}}}])
    path = Path(saved['path'])

    def add_and_rename():
        reply = app.call('edit_action_parameters', name=NAME, edits=[
            {'op': 'add', 'name': 'unused', 'type': 'integer', 'default': 3},
            {'op': 'rename', 'name': 'sigma', 'to': 'radius'}])
        names = [row['name'] for row in reply['parameters']]
        assert names == ['radius', 'unused'], reply
        action = app.call('list_actions', name=NAME)['action']
        assert action['steps'][0]['arguments']['sigma'] == {'$param': 'radius'}, action
        assert played(radius=8) == direct(8), 'radius 8 differs from a direct sigma 8'
        assert played() == direct(4), 'the default differs from a direct sigma 4'
        assert direct(8) != direct(4)
    check('add + rename in one call rewrite the step, and the renamed parameter drives the run',
          add_and_rename)

    def refusals():
        before = path.read_bytes()
        cases = [
            ([{'op': 'remove', 'name': 'radius'}], 'step 1 (apply_filter)'),
            ([{'op': 'retype', 'name': 'radius', 'type': 'string'}], 'give a new default'),
            ([{'op': 'explode', 'name': 'radius'}], 'op must be add, rename'),
            ([{'op': 'rename', 'name': 'radius', 'to': 'r2'},
              {'op': 'remove', 'name': 'nope'}], 'no parameter named "nope"'),
            ([{'op': 'rename', 'name': 'radius', 'to': 'unused'}], 'already declared'),
            ([{'op': 'add', 'name': '9lives', 'type': 'number'}], 'is not a parameter name'),
        ]
        for edits, sentence in cases:
            refusal = app.client.call('edit_action_parameters', fail=True, name=NAME, edits=edits)
            assert sentence in refusal, (edits, refusal)
        assert path.read_bytes() == before, 'a refused edit list changed the file'
        # A library edit is never an action step.
        refusal = app.client.call('save_action', fail=True, name='Nested Edit', steps=[
            {'tool': 'edit_action_parameters', 'arguments': {'name': NAME, 'edits': []}}])
        assert 'cannot be a step' in refusal and 'editing parameters' in refusal, refusal
    check('refusals name the step, the type rule and the missing parameter, and change no byte; '
          'edit_action_parameters is not a step', refusals)

    def retype_and_require():
        reply = app.call('edit_action_parameters', name=NAME, edits=[
            {'op': 'retype', 'name': 'radius', 'type': 'integer'},
            {'op': 'set_description', 'name': 'radius', 'description': 'Blur radius in px'}])
        radius = next(row for row in reply['parameters'] if row['name'] == 'radius')
        assert radius['type'] == 'integer' and radius['default'] == 4, radius
        assert radius['description'] == 'Blur radius in px', radius
        app.call('edit_action_parameters', name=NAME, edits=[
            {'op': 'set_default', 'name': 'radius', 'default': None},
            {'op': 'remove', 'name': 'unused'}])
        doc = picture()
        refusal = app.client.call('run_action', fail=True, document_id=doc, name=NAME)
        assert 'needs a value for "radius"' in refusal, refusal
        app.call('close_document', document_id=doc, discard_changes=True)
    check('retype keeps an exact default; a null default makes the parameter required',
          retype_and_require)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-param-edits-'))).expanduser()
    output = output.resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    with App(output / 'app', output / 'recovery', interval=600) as app:
        run(app)
    raise SystemExit(0 if RESULTS and all(RESULTS) else 1)


if __name__ == '__main__':
    main()
