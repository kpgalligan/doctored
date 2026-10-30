#!/usr/bin/env python3
"""Extended 9D native retouch MCP gate. Uses only owned apps/private stores.

Fixtures are synthetic adjacent-code ramps and a red pupil. Pixel correctness
uses independent PNG decoding, exact preserved samples and an analytical
fractional source reference. Real-photo synthesis qualification is separate.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts/retouch-probe'))
import app_under_test
from support import HostApp, fixtures, call_for, wait_for


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


probe = load('retouch_extended_mcp', ROOT / 'scripts/retouch-probe/mcp.py')
driver = load('retouch_extended_driver', ROOT / 'scripts/verify_phase9d.py')


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def dependencies():
    result=driver.dependencies();result[str(Path(__file__).relative_to(ROOT))]=digest(__file__)
    return result


def fixture(folder, depth, eye=False):
    maximum = (1 << depth) - 1
    values = []
    for y in range(48):
        for x in range(64):
            v = 12001 + 97*x + 71*y
            rgb = (v, v+3001, v+6002)
            if 27 <= x < 34 and 19 <= y < 26:
                rgb = (3001, 3002, 3003)
            if eye and (x-30)**2 + (y-22)**2 <= 9:
                rgb = (57001, 6002, 4003)
            if depth == 8:
                rgb = tuple(round(c/257) for c in rgb)
            values.append((*rgb, maximum))
    path = folder / f'{"eye" if eye else "ramp"}-{depth}.png'
    path.write_bytes(fixtures.png(64, 48, depth, values, fixtures.srgb_profile()))
    return path


def capture(call, folder, name):
    return probe.samples(call, folder, name)


def dimensions(layer):
    return tuple(layer['dimensions']) if 'dimensions' in layer else (layer['width'], layer['height'])


def restored(call, before, folder, name):
    call('undo')
    assert capture(call, folder, name+'-undo') == before, name
    call('redo')


def run_depth(folder, executable, depth):
    folder.mkdir(parents=True)
    input_path = fixture(folder, depth)
    groups = []
    with probe.TracedApp(folder/'app', folder/'recovery', executable) as app:
        call = probe.opened(app, input_path, depth, folder/'calls.jsonl')
        initial = capture(call, folder, 'initial')
        stroke = dict(layer=0, points=[[30,22]], source_x=18, source_y=22, size=14)
        matrix = [1,0,0,1,-12.75,0]
        app.call('start_recording')
        before_history = probe.history(call)
        call('clone_stamp', sample='current', source_transform=matrix, **stroke)
        after = capture(call, folder, 'fractional')
        assert probe.history(call)[1] == before_history[1]+1
        expected = tuple(round(.75*a+.25*b) for a,b in zip(probe.at(initial,17,22), probe.at(initial,18,22)))
        actual = probe.at(after,30,22)
        assert max(abs(a-b) for a,b in zip(actual,expected)) <= 2, (actual,expected)
        assert actual != probe.at(initial,18,22), 'fractional coordinate must be observable'
        recorded = app.call('stop_recording', name=f'Fractional clone {depth}')
        assert [s['tool'] for s in recorded['steps']] == ['clone_stamp'], recorded
        assert recorded['steps'][0]['arguments']['source_transform'] == matrix
        restored(call, initial, folder, 'fractional')
        assert capture(call,folder,'fractional-redo') == after
        call('undo')
        assert call('run_action',name=f'Fractional clone {depth}')['ok']
        assert capture(call,folder,'fractional-replay') == after
        call('undo')
        groups.append('fractional native sample, one Undo, Redo and exact Action replay')

        # Selection is the WRITE domain; source taps lie outside it.
        call('select_rect', x=29, y=21, width=3, height=3)
        selected_before = capture(call,folder,'selected-before')
        call('clone_stamp',sample='current',source_transform=matrix,**stroke)
        selected_after = capture(call,folder,'selected-after')
        assert selected_after != selected_before
        for y in range(48):
            for x in range(64):
                if not (29 <= x < 32 and 21 <= y < 24):
                    assert probe.at(selected_after,x,y)==probe.at(selected_before,x,y),(x,y)
        call('undo'); call('deselect')
        groups.append('exact samples outside write selection with unselected source taps')

        # The same native source can be sampled beneath a strong live grade.
        probe.add_grade(call,0,brightness=.2)
        graded = capture(call,folder,'graded')
        call('clone_stamp',sample='all',source_transform=matrix,**stroke)
        all_result = capture(call,folder,'graded-all')
        call('undo')
        call('clone_stamp',sample='current_and_below',source_transform=matrix,**stroke)
        below_result = capture(call,folder,'graded-below')
        threshold = 8000 if depth == 16 else 30
        assert probe.at(all_result,30,22)[0]-probe.at(below_result,30,22)[0] > threshold
        call('undo')
        call('clone_stamp',sample='all',ignore_adjustments=True,source_transform=matrix,**stroke)
        assert capture(call,folder,'graded-ignore') == below_result
        call('undo')
        call('clone_stamp',sample='current_and_below',output='new_layer',source_transform=matrix,**stroke)
        assert len(call('get_document')['layers'])==3
        layered = capture(call,folder,'graded-new-layer')
        assert max(abs(a-b) for a,b in zip(layered[3],below_result[3])) <= 2
        call('undo')
        assert capture(call,folder,'graded-restored') == graded
        call('undo')  # Remove the live grade.
        groups.append('sampling below live grade, ignore adjustments, reversible new-layer output')

        for tool, arguments in [('heal_stroke',dict(stroke,opacity=50,source_transform=matrix)),
                                 ('spot_heal_stroke',dict(layer=0,points=[[30,22]],size=22,opacity=75,seed=19))]:
            baseline=capture(call,folder,tool+'-before')
            call(tool,sample='current',**arguments)
            repaired=capture(call,folder,tool+'-result')
            assert repaired != baseline, tool
            assert repaired[3][3::4] == baseline[3][3::4], tool
            for y in range(48):
                for x in range(64):
                    if x<16 or x>44 or y<8 or y>36:
                        assert probe.at(repaired,x,y)==probe.at(baseline,x,y), (tool,x,y)
            restored(call,baseline,folder,tool)
            saved=folder/(tool+'.rz')
            call('save_copy',path=str(saved))
            reopened=probe.opened(app,saved,depth,folder/(tool+'-reopened.jsonl'))
            assert capture(reopened,folder,tool+'-reopened')==repaired
            call('undo')
            groups.append(tool+' native alpha, bounded footprint, Undo/Redo and native reopen')

        artwork=probe.opened(app,input_path,depth,folder/'artwork.jsonl')
        artwork('add_shape_layer',kind='rect',x=27,y=19,w=7,h=7,fill='#080808')
        shape_before=artwork('get_document')['layers'][1]
        artwork('spot_heal_stroke',layer=1,points=[[30,22]],size=22,opacity=100,
                sample='current_and_below',output='new_layer',seed=19)
        shape_after=artwork('get_document')['layers'][1]
        assert shape_after==shape_before, 'new repair must retain the original editable artwork description'
        assert artwork('edit_shape_layer',layer=1,fill='#090909')['ok']
        groups.append('new-layer spot healing retains original editable artwork')

        # A small offset raster can grow only after an explicit request.
        call('transform_layer',layer=0,scale=.5,sampler='nearest')
        small=call('get_document')['layers'][0]
        small_pixels=capture(call,folder,'small')
        growth=dict(layer=0,points=[[3,3]],source_x=24,source_y=20,size=8,sample='current')
        call('clone_stamp',**growth)
        assert capture(call,folder,'not-expanded')==small_pixels
        call('clone_stamp',expand_destination=True,**growth)
        expanded=call('get_document')['layers'][0]
        assert dimensions(expanded)==(64,48), expanded
        assert probe.at(capture(call,folder,'expanded'),3,3)[3]>0
        call('undo')
        restored_small=call('get_document')['layers'][0]
        assert dimensions(restored_small)==dimensions(small)
        assert capture(call,folder,'small-undo')==small_pixels
        groups.append('explicit destination growth and exact geometry Undo')

        eye=probe.opened(app,fixture(folder,depth,eye=True),depth,folder/'eye.jsonl')
        original_eye=capture(eye,folder,'eye-before')
        eye('red_eye',layer=0,x=23,y=15,width=14,height=14,pupil_size=100,darken=50)
        corrected=capture(eye,folder,'eye-corrected')
        assert probe.at(corrected,30,22)[0]<probe.at(original_eye,30,22)[0]
        assert corrected[3][3::4]==original_eye[3][3::4]
        for y in range(48):
            for x in range(64):
                if not (23<=x<37 and 15<=y<29):
                    assert probe.at(corrected,x,y)==probe.at(original_eye,x,y)
        restored(eye,original_eye,folder,'eye')
        groups.append('red-eye native RGB, alpha, exact outside rectangle and Undo')

        expected_recovery=capture(artwork,folder,'recovery-expected')
        snapshot=artwork('create_snapshot',name='Retouch and editable artwork')['snapshot_id']
        snapshot_path=folder/'retouch.drsnapshot'
        artwork('save_snapshot',snapshot_id=snapshot,path=str(snapshot_path))
        artwork('edit_shape_layer',layer=1,fill='#101010')
        loaded=artwork('load_snapshot',path=str(snapshot_path))['snapshot_id']
        artwork('restore_snapshot',snapshot_id=loaded)
        assert capture(artwork,folder,'snapshot-restored')==expected_recovery
        groups.append('saved snapshot restores retouch pixels and editable artwork')
        artwork('write_recovery',wait=True)
        checkpoint=wait_for(lambda:artwork('get_recovery_status')['document'],
            lambda v:v.get('state')=='protected' and not v.get('pending'),timeout=60,
            detail='native retouch durable recovery')
        recovery_id=checkpoint['recovery_id']
        app.crash()
    with probe.TracedApp(folder/'recovery-app',folder/'recovery',executable) as recovered_app:
        assert recovery_id in {v['recovery_id'] for v in recovered_app.call('get_recovery_status')['available']}
        identifier=recovered_app.call('recover_document',recovery_id=recovery_id,show=True)['document']['id']
        recovered=call_for(recovered_app,identifier,folder/'recovered.jsonl')
        assert capture(recovered,folder,'recovered')==expected_recovery
        assert recovered('edit_shape_layer',layer=1,fill='#111111')['ok']
        recovered('undo')
        assert capture(recovered,folder,'recovered-undo')==expected_recovery
        assert recovered('get_document')['unsaved_changes']
        groups.append('owned-process crash recovery preserves retouch, artwork editability and Undo')
    return dict(depth=depth,groups=groups,group_count=len(groups),fixture_sha256=digest(input_path))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app',type=Path,default=app_under_test.default_app_executable())
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--require-current',action='store_true')
    args=parser.parse_args()
    folder=Path(tempfile.mkdtemp(prefix='rasterize-phase9d-native-'))
    before=dependencies()
    report=dict(passed=False,scratch=str(folder),app_sha256=digest(args.app))
    try:
        report['cases']=[]
        for depth in (8,16):
            report['cases'].append(run_depth(folder/f'rgba{depth}',args.app.resolve(),depth))
        after=dependencies()
        changes=sorted(k for k in before.keys()|after.keys() if before.get(k)!=after.get(k))
        report['source_check']=dict(files=len(before),changes=changes,file_set_exact=before.keys()==after.keys())
        if args.require_current: assert not changes,changes
        assert digest(args.app)==report['app_sha256']
        report['passed']=True
    except BaseException as error:
        report['error']=repr(error)
        raise
    finally:
        report['all_owned_apps_exited']=all(a.process is None or a.process.poll() is not None for a in HostApp.owners)
        args.report.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
        print('Native retouch receipt:',args.report,flush=True)


if __name__=='__main__': main()
