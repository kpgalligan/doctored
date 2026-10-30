#!/usr/bin/env python3
"""Owned MCP cancellation, replacement and retained-candidate lifetime checks."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import time

sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('caf_workflow', ROOT/'scripts/verify_phase9e.py')
workflow = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workflow)
probe, wait_for = workflow.probe, workflow.wait_for


def dependencies():
    result=workflow.dependencies()
    result[str(Path(__file__).relative_to(ROOT))]=workflow.digest(__file__)
    return result


def rpc(app, identifier, tool, **arguments):
    return app.client.rpc('tools/call', {'name':tool, 'arguments':dict(document_id=identifier, **arguments)})


def run_depth(folder, executable, depth):
    folder.mkdir(parents=True)
    large, bounds = workflow.fixture(folder, depth, large=True)
    small, small_bounds = workflow.fixture(folder, depth)
    with probe.TracedApp(folder/'app', folder/'recovery', executable) as app:
        call = probe.opened(app, large, depth, folder/'calls.jsonl')
        workflow.select(call, bounds)
        initial = workflow.capture(call, folder, 'initial')
        history = probe.history(call)
        parameters = dict(layer=0, ring=0, seed=12345, sample='current')
        pending = call('preview_content_aware_fill', background=True, **parameters)
        token = pending['preview_id']
        active = wait_for(lambda:call('preview_content_aware_fill',preview_id=token),
            lambda r:r.get('stage') in (3,4) and r.get('progress',0)>0,
            timeout=15, detail='native synthesis stage before cancellation')
        assert active['state']=='running' and active['scratch_bytes']>0, active
        busy = rpc(app,call.identifier,'preview_content_aware_fill',**parameters)
        assert busy.get('isError'),busy
        text=' '.join(b.get('text','')for b in busy.get('content',[]))
        assert 'background operation' in text.lower(),text
        # The refused replacement must preserve the original ticket.
        assert call('preview_content_aware_fill',preview_id=token)['preview_id']==token
        started=time.monotonic()
        call('discard_content_aware_fill_preview',preview_id=token)
        released=wait_for(lambda:call('resource_usage'),
            lambda r:r['transient_estimate_bytes']==0,timeout=2,detail='cancelled preview mailbox and scratch release')
        latency=time.monotonic()-started
        assert probe.history(call)==history
        assert workflow.capture(call,folder,'cancelled')==initial
        workflow.refusal(call,'content_aware_fill',preview_id=token)

        # Ordinary calls retain their original job_status/cancel_job contract.
        job=call('content_aware_fill',background=True,**parameters)
        job_id=job['job_id']
        wait_for(lambda:call('job_status',job_id=job_id),
            lambda r:r.get('progress',0)>0 and r.get('state')=='running',
            timeout=15,detail='ordinary final fill running')
        call('cancel_job',job_id=job_id)
        stopped=wait_for(lambda:call('job_status',job_id=job_id),
            lambda r:r.get('state')!='running',timeout=2,detail='ordinary fill cancellation')
        assert stopped['state']=='cancelled' and stopped['scratch_estimate_bytes']==0,stopped
        assert probe.history(call)==history
        assert workflow.capture(call,folder,'job-cancelled')==initial

        small_call=probe.opened(app,small,depth,folder/'small.jsonl')
        workflow.select(small_call,small_bounds)
        baseline=small_call('resource_usage')['precision_capture_payload_bytes']
        review=small_call('preview_content_aware_fill',layer=0,seed=19,sample='current')
        token=review['preview_id']
        assert review['state']=='ready'
        ready_bytes=small_call('resource_usage')['precision_capture_payload_bytes']
        assert ready_bytes>baseline, 'candidate coverage/display lease was not counted'
        workflow.select(small_call,(1,1,2,2))
        workflow.refusal(small_call,'content_aware_fill',preview_id=token)
        cleared=wait_for(lambda:small_call('resource_usage'),
            lambda r:r['precision_capture_payload_bytes']<=baseline,
            timeout=2,detail='stale Apply releases candidate payload')
        workflow.refusal(small_call,'preview_content_aware_fill',preview_id=token)
        return dict(depth=depth,groups=4,observed_stage=active['stage'],
            observed_progress=active['progress'],cancellation_seconds=latency,
            scratch_after_cancel=released['transient_estimate_bytes'],
            baseline_payload_bytes=baseline,ready_payload_bytes=ready_bytes,
            stale_payload_bytes=cleared['precision_capture_payload_bytes'])


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app',type=Path,default=app_under_test.default_app_executable())
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--require-current',action='store_true')
    args=parser.parse_args()
    folder=Path(tempfile.mkdtemp(prefix='rasterize-phase9e-lifecycle-'))
    before=dependencies()
    report=dict(passed=False,scratch=str(folder),app_sha256=workflow.digest(args.app))
    try:
        report['cases']=[run_depth(folder/f'rgba{depth}',args.app.resolve(),depth)for depth in (8,16)]
        after=dependencies()
        changed=sorted(k for k in before.keys()|after.keys()if before.get(k)!=after.get(k))
        report['source_changes']=changed
        if args.require_current:assert not changed,changed
        report['passed']=True
    except BaseException as error:
        report['error']=repr(error)
        raise
    finally:
        report['all_owned_apps_exited']=all(a.process is None or a.process.poll()is not None for a in workflow.HostApp.owners)
        args.report.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
        print('Content-aware lifecycle receipt:',args.report,flush=True)


if __name__=='__main__':main()
