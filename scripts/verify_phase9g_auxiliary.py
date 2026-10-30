#!/usr/bin/env python3
"""Actual HEIC10 import with independent auxiliary payload/EXIF/resize oracles.

Generate the corpus first with auxiliary-matte-probe/fixtures.swift. References
are decoded raw ImageIO payloads before any product normalization or orientation.
Synthetic files qualify transfer/geometry; they are not camera-quality evidence.
"""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('auxiliary_workflow_support', ROOT/'scripts/verify_phase9f.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)


def dependencies():
    result = s.dependencies()
    extra = [Path(__file__), ROOT/'scripts/auxiliary-matte-probe/fixtures.swift']
    result.update({str(p):s.digest(p) for p in extra})
    return result


def oriented(values, width, height, orientation):
    dw, dh = (height, width) if orientation >= 5 else (width, height)
    mappings = {
        1: lambda x, y: (x, y), 2: lambda x, y: (width-1-x, y),
        3: lambda x, y: (width-1-x, height-1-y), 4: lambda x, y: (x, height-1-y),
        5: lambda x, y: (y, x), 6: lambda x, y: (y, height-1-x),
        7: lambda x, y: (width-1-y, height-1-x), 8: lambda x, y: (width-1-y, x),
    }
    output = []
    for y in range(dh):
        for x in range(dw):
            sx, sy = mappings[orientation](x, y)
            output.append(values[sy*width+sx])
    return dw, dh, output


def bilinear(values, width, height, out_width, out_height):
    result = []
    for y in range(out_height):
        sy = (y+0.5)*height/out_height-0.5
        y0 = math.floor(sy)
        ty = sy-y0
        for x in range(out_width):
            sx = (x+0.5)*width/out_width-0.5
            x0 = math.floor(sx)
            tx = sx-x0
            value = 0.0
            for dy, wy in [(0, 1-ty), (1, ty)]:
                for dx, wx in [(0, 1-tx), (1, tx)]:
                    ix = min(width-1, max(0, x0+dx))
                    iy = min(height-1, max(0, y0+dy))
                    value += values[iy*width+ix]*wx*wy
            result.append(math.floor(value+0.5))
    return result


def oracle(folder, fixture, plane):
    description = plane['description']
    width, height = description['Width'], description['Height']
    stride = description['BytesPerRow']
    raw = (folder/plane['raw_file']).read_bytes()
    assert len(raw) >= stride*height
    pixel_format = description['PixelFormat']
    if pixel_format == 1278226488:  # L008
        values = [raw[y*stride+x]*257 for y in range(height) for x in range(width)]
        source_bits, source_format, finite = 8, 'one_component8', None
    else:
        assert pixel_format == 1717856627, pixel_format  # fdis
        values = [struct.unpack_from('<f', raw, y*stride+4*x)[0]
                  for y in range(height) for x in range(width)]
        low, high = min(values), max(values)
        assert all(math.isfinite(v) for v in values) and high > low
        values = [math.floor((v-low)/(high-low)*65535+0.5) for v in values]
        # ImageIO exposes Float32 storage, but this HEIC encoder quantizes its
        # encoded disparity. Retain the measured source diversity separately;
        # neither interpolation nor normalization creates new camera detail.
        source_bits, source_format, finite = 32, 'disparity_float32', [low, high]
    width, height, values = oriented(values, width, height, fixture['orientation'])
    dw, dh = (fixture['height'], fixture['width']) if fixture['orientation'] >= 5 else (fixture['width'], fixture['height'])
    return dict(width=dw, height=dh, values=bilinear(values,width,height,dw,dh),
                source_width=width, source_height=height, source_bits=source_bits,
                source_format=source_format, finite_range=finite,
                distinct_source_codes=len(set(values)))


def native(call, folder, name):
    path = folder/(name+'.rz')
    call('save_copy', path=str(path))
    value = s.shared.probe.read_native(path)
    assert value['version'] == 25
    return value, path


def state(value):
    # Native session IDs are deliberately reminted, and are not serialized.
    return dict(document=value['document'], assets=value['assets'], bindings=value['bindings'],
                resources=value['resources'], artwork=value['artwork'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, default=app_under_test.default_app_executable())
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--require-current', action='store_true')
    args = parser.parse_args()
    corpus = json.loads((args.fixtures/'fixtures.json').read_text())
    fixtures = corpus['cases']
    assert [f['orientation'] for f in fixtures] == list(range(1,9))
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase9g-auxiliary-'))
    before = dependencies()
    extra = [p for p in args.fixtures.iterdir() if p.is_file()]
    before.update({str(p):s.digest(p) for p in extra})
    report = dict(passed=False, scratch=str(folder), app_sha256=s.digest(args.app), cases=[])
    recoveries = []
    try:
        with s.probe.TracedApp(folder/'app',folder/'recovery',args.app.resolve()) as app:
            report['opaque_layout_refusal'] = app.call('open_document',
                path=str(args.fixtures/corpus['refused_opaque']), precision='preserve', fail=True)
            for fixture in fixtures:
                orientation = fixture['orientation']
                destination = folder/str(orientation)
                destination.mkdir()
                call = s.probe.opened(app,args.fixtures/fixture['file'],16,destination/'calls.jsonl')
                parsed,path = native(call,destination,'imported')
                expected_state = state(parsed)
                planes = {c['name']:c for c in parsed['document']['channels']}
                assert set(planes) == {'Depth','Portrait Matte'}
                result = dict(orientation=orientation,planes=[])
                for plane in fixture['planes']:
                    expected = oracle(args.fixtures,fixture,plane)
                    channel = planes[plane['name']]
                    actual = struct.unpack('<'+'H'*(len(channel['data'])//2),channel['data'])
                    errors = [abs(a-b) for a,b in zip(actual,expected['values'],strict=True)]
                    assert max(errors) <= 1, (orientation,plane['name'],max(errors))
                    provenance = channel['provenance']
                    assert provenance['version'] == 1 and provenance['kind'] == 'camera_auxiliary'
                    assert provenance['precision_basis'] == 'decoded_auxiliary_buffer'
                    assert provenance['encoded_precision'] == 'not_inspected'
                    for key in ('source_bits','source_format','source_width','source_height'):
                        assert provenance[key] == expected[key], (key,provenance,expected[key])
                    assert provenance['exif_orientation'] == orientation and provenance['orientation_applied']
                    assert provenance['alignment'] == 'oriented_full_image_bilinear'
                    if expected['finite_range']:
                        assert provenance['finite_range'] == expected['finite_range']
                    result['planes'].append(dict(name=plane['name'],maximum_error=max(errors),
                        distinct_native_codes=len(set(actual)),distinct_source_codes=expected['distinct_source_codes'],provenance=provenance))
                snapshot = call('create_snapshot',name='Auxiliary planes')['snapshot_id']
                archive = destination/'auxiliary.drsnapshot'
                call('save_snapshot',snapshot_id=snapshot,path=str(archive))
                call('invert_channel',channel='Depth')
                call('undo')
                assert state(native(call,destination,'undo')[0]) == expected_state
                call('redo')
                restored = call('load_snapshot',path=str(archive))['snapshot_id']
                call('restore_snapshot',snapshot_id=restored)
                assert state(native(call,destination,'snapshot-restored')[0]) == expected_state
                reopened = s.probe.opened(app,path,16,destination/'native-reopen.jsonl')
                assert state(native(reopened,destination,'reopened')[0]) == expected_state
                call('write_recovery',wait=True)
                status = s.wait_for(lambda:call('get_recovery_status')['document'],
                    lambda v:v.get('state')=='protected' and not v.get('pending'),
                    timeout=60,detail='auxiliary recovery publication')
                recoveries.append((status['recovery_id'],expected_state,destination))
                report['cases'].append(result)
            app.crash()
        with s.probe.TracedApp(folder/'recovery-app',folder/'recovery',args.app.resolve()) as app:
            for identifier, expected, destination in recoveries:
                document = app.call('recover_document',recovery_id=identifier,show=True)['document']['id']
                call = s.call_for(app,document,destination/'recovery.jsonl')
                assert state(native(call,destination,'recovered')[0]) == expected
        after=dependencies()
        after.update({str(p):s.digest(p) for p in extra})
        changes=sorted(k for k in before.keys()|after.keys() if before.get(k)!=after.get(k))
        report['source_check']=dict(files=len(before),changes=changes,file_set_exact=before.keys()==after.keys())
        if args.require_current: assert not changes,changes
        assert s.digest(args.app)==report['app_sha256']
        report['passed']=True
    except BaseException as error:
        report['error']=repr(error)
        raise
    finally:
        report['all_owned_apps_exited']=all(a.process is None or a.process.poll() is not None for a in s.shared.HostApp.owners)
        args.report.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
        print(args.report,flush=True)


if __name__=='__main__':
    main()
