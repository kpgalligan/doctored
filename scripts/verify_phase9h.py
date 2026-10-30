#!/usr/bin/env python3
"""Release MCP qualification of native paint/fill with independent sample oracles.

Compatibility with old apps is qualified separately by verify_phase9h_compat.
This probe reads native storage using the independent DRDC/PNG decoder; no
rendered screenshot is an authoritative source. All apps and settings are owned.
"""
import argparse
import base64
import json
import math
from pathlib import Path
import runpy
import struct
import sys
import tempfile
sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]
f = runpy.run_path(str(ROOT/'scripts/verify_phase9f.py'))
probe, shared = f['probe'], f['shared']
raw_local_reader = runpy.run_path(str(ROOT/'scripts/raw-local-probe/native.py'))
legacy_native_reader = runpy.run_path(str(ROOT/'scripts/live-filter-probe/native.py'))
W = H = 32
M = 65535


def dependencies():
    result = f['dependencies']()
    result[str(Path(__file__).relative_to(ROOT))] = f['digest'](__file__)
    local_reader = ROOT/'scripts/raw-local-probe/native.py'
    result[str(local_reader.relative_to(ROOT))] = f['digest'](local_reader)
    host_reader = ROOT/'scripts/host-source-probe/native.py'
    result[str(host_reader.relative_to(ROOT))] = f['digest'](host_reader)
    return result


def fixture(folder, name='native', color=None):
    values = [color or (3001+(x*109+y*59)%12000, 27003+(x*157+y*43)%16000,
                       13001+(x*241+y*89)%21000, [0,1,257,20003,65535][(x+2*y)%5])
              for y in range(H) for x in range(W)]
    path = folder/(name+'.png')
    path.write_bytes(f['fixtures'].png(W,H,16,values,f['fixtures'].srgb_profile()))
    return path


def capture(call, folder, name):
    path = folder/(name+'.rz')
    call('save_copy',path=str(path))
    # 12F-4's independent reader validates the v45/46 LIDS and optional RSPJ
    # wrappers before returning their unchanged v21/22 body. The 9H oracle
    # then reads precisely its original native prefix from those body bytes.
    encoded = path.read_bytes()
    outer = struct.unpack_from('<I', encoded, 4)[0]
    if outer in (45, 46):
        start = 8
        # Typed word 55 sits first inside 46; its LOPN table is the
        # last one before LIDS.
        group_open = outer == 46 and struct.unpack_from('<I', encoded, start)[0] == 55
        if group_open:
            start += 4
        projective = struct.unpack_from('<I', encoded, start)[0]
        if projective in (51, 52):
            start += 4
        inner = struct.unpack_from('<I', encoded, start)[0]
        if inner in (21, 22):
            body, _, _ = raw_local_reader['_legacy_body'](encoded)
        else:
            # The 9H fixture is typed, whose v13 body has no 8G source
            # framing. Validate the same outer identity table, then let 9H's
            # existing typed reader inspect that unchanged v13 prefix.
            assert inner in (13, 15, 17, 19, 20, 22, 24), inner
            lids = encoded.rfind(b'LIDS')
            assert lids > start and len(encoded) >= lids + 12
            schema, count = struct.unpack_from('<II', encoded, lids + 4)
            assert schema == 1 and count > 0 and lids + 12 + count * 8 == len(encoded)
            ids = struct.unpack_from('<' + 'Q' * count, encoded, lids + 12)
            assert all(ids) and len(set(ids)) == count
            end = lids
            if group_open:
                end = encoded.rfind(b'LOPN', start, lids)
                opened, closed = struct.unpack_from('<II', encoded, end + 4)
                assert end >= start and opened == 1 and closed > 0 and end + 12 + 4 * closed == lids
            body = encoded[:4] + encoded[start:end]
    else:
        body = encoded
    native = legacy_native_reader['prefix'](legacy_native_reader['Reader'](body))
    pixels = probe.png_samples(native['fallbacks'][0])
    return dict(width=pixels['width'],height=pixels['height'],samples=tuple(pixels['samples']),
                profile=native['profile'],layers=native['layers'],channels=native['channels'])


def coverage(call):
    return tuple(p['coverage16'] for p in call('get_selection',points=[dict(x=x,y=y)
                 for y in range(H) for x in range(W)])['samples'])


def set_coverage(call):
    values = tuple([0,1,2,256,257,32768,65535][(x+3*y)%7] for y in range(H) for x in range(W))
    encoded = b''.join(struct.pack('<IHH',1,v,65535) for v in values)
    call('quick_mask_stroke',width=W,height=H,coverage_rle16=base64.b64encode(encoded).decode())
    assert coverage(call)==values
    return values


def canonical(dabs, hardness=.23, roundness=.71, angle=27, pixel=False):
    return dict(version=1,encoding='le_f64_xy_diameter_opacity',
                dabs=base64.b64encode(b''.join(struct.pack('<dddd',*d) for d in dabs)).decode(),
                hardness=hardness,roundness=roundness,angle_degrees=angle,pixel=pixel)


def field(stroke):
    """Independent point-center ellipse and smoothstep/product reference."""
    data = base64.b64decode(stroke['dabs'])
    dabs = list(struct.iter_unpack('<dddd',data))
    out=[]
    angle=math.radians(stroke['angle_degrees']); c,s=math.cos(angle),math.sin(angle)
    for y in range(H):
        for x in range(W):
            remaining=1.
            for cx,cy,d,opacity in dabs:
                if stroke['pixel']:
                    value=max(0,min(x+1,cx+d/2)-max(x,cx-d/2))*max(0,min(y+1,cy+d/2)-max(y,cy-d/2))
                else:
                    dx,dy=x+.5-cx,y+.5-cy
                    radius=math.hypot((c*dx-s*dy)/(d/2),(s*dx+c*dy)/(d/2*stroke['roundness']))
                    hard=stroke['hardness']
                    q=min(1,max(0,(radius-hard)/(1-hard)))
                    value=1-3*q*q+2*q*q*q
                remaining*=1-value*opacity
            out.append(math.floor((1-remaining)*M+.5))
    return out


def paint_reference(before, footprint, selection, color, opacity=1, erase=False):
    expected=[]
    for i in range(W*H):
        p=before['samples'][4*i:4*i+4]
        weight=footprint[i]/M*selection[i]/M*opacity
        if erase:
            expected.extend((*p[:3],math.floor(p[3]*(1-weight)+.5)));continue
        alpha=color[3]*weight
        if alpha==0:
            expected.extend(p);continue
        total=alpha+p[3]/M*(1-alpha)
        rgb=[(color[k]*alpha+p[k]/M*p[3]/M*(1-alpha))/total for k in range(3)]
        expected.extend([*(math.floor(v*M+.5) for v in rgb),math.floor(total*M+.5)])
    return tuple(expected)


def compare(expected, actual, before, footprint, selection, tolerance=1):
    assert len(expected)==len(actual['samples'])
    difference=max(abs(x-y) for x,y in zip(expected,actual['samples']))
    assert difference<=tolerance,(difference,tolerance)
    for i,(a,b) in enumerate(zip(footprint,selection)):
        if a==0 or b==0:
            assert actual['samples'][4*i:4*i+4]==before['samples'][4*i:4*i+4],i
    assert actual['profile']==before['profile']
    return difference


def replay(app,call,folder,name,tool,arguments):
    before=capture(call,folder,name+'-before'); history=probe.history(call)
    app.call('start_recording')
    call(tool,**arguments)
    expected=capture(call,folder,name+'-after')
    action=app.call('stop_recording',name=name)
    assert len(action['steps'])==1,action
    changed=expected!=before
    assert probe.history(call)[1]==history[1]+int(changed)
    if changed:
        call('undo'); assert capture(call,folder,name+'-undo')==before
        call('redo'); assert capture(call,folder,name+'-redo')==expected
        call('undo')
    call('run_action',name=name)
    assert capture(call,folder,name+'-replay')==expected
    if changed:call('undo')
    return expected,action['steps'][0]['arguments']



def scalar_cases(app,folder):
    folder.mkdir()
    call=probe.opened(app,fixture(folder,'scalars'),16,folder/'calls.jsonl')
    selection=set_coverage(call)
    call('add_layer_mask',layer=0,kind='from_selection')
    call('add_channel',name='Native',**{'from':'empty'})
    stroke=canonical([[15.13,15.71,35,.37],[12.2,11.7,17,.13]])
    fp=field(stroke)
    def plane(state,target):
        if target=='mask':return struct.unpack('<'+'H'*(W*H),state['layers'][0]['mask']['data'])
        if target=='channel:Native':return struct.unpack('<'+'H'*(W*H),state['channels'][0]['data'])
        return state['samples'][['red','green','blue','alpha'].index(target)::4]
    rows=[]
    for index,target in enumerate(['mask','channel:Native','red','green','blue','alpha']):
        before=capture(call,folder,'scalar'+str(index)+'-original')
        old=plane(before,target)
        history=probe.history(call)
        call('fill',layer=0,target=target,x=0,y=0,tolerance=255,contiguous=False,fill_mode='color_match',
             rgba=[.2,.4,.7,.8],color_space='document')
        assert capture(call,folder,'scalar'+str(index)+'-excluded-seed')==before and probe.history(call)==history
        after,_=replay(app,call,folder,'ScalarBrush'+str(index),'brush_stroke',
            dict(layer=0,target=target,points=[[15.13,15.71]],size=35,native_stroke=stroke,opacity=.731))
        expected=[math.floor(v+(M-v)*(a/M)*(b/M)*.731+.5) for v,a,b in zip(old,fp,selection)]
        error=max(abs(a-b) for a,b in zip(expected,plane(after,target)))
        assert error<=1,(target,error)
        for i,weight in enumerate(selection):
            if weight==0:assert plane(after,target)[i]==old[i]
        if target in ['mask','channel:Native']:assert after['samples']==before['samples']
        else:
            changed=['red','green','blue','alpha'].index(target)
            for channel in range(4):
                if channel!=changed:assert after['samples'][channel::4]==before['samples'][channel::4]
        gradient,_=replay(app,call,folder,'ScalarGradient'+str(index),'gradient',
            dict(layer=0,target=target,x0=1.5,y0=2.5,x1=28.5,y1=29.5,shape='radial' if index%2 else 'linear',
                 start_rgba=[.12345,.12345,.12345,.41],end_rgba=[.87654,.87654,.87654,0],color_space='document',opacity=.73))
        for i,weight in enumerate(selection):
            if weight==0:assert plane(gradient,target)[i]==old[i]
        rows.append(dict(target=target,brush_maximum_error=error,gradient_actions=True))
    return rows


def fill_opacity_boundary(app,folder):
    folder.mkdir()
    call=probe.opened(app,fixture(folder,'transparent',(1001,2003,3007,0)),16,folder/'calls.jsonl')
    alpha=.0027696169880990083
    args=dict(layer=0,x=0,y=0,fill_mode='color_match',tolerance=0,contiguous=True,
              rgba=[.19,.37,.73,alpha],color_space='document',opacity=.73)
    after,recorded=replay(app,call,folder,'FillAlphaBoundary','fill',args)
    assert recorded['rgba'][3]==alpha and recorded['opacity']==.73
    # Native f32 evaluation rounds each independent authored parameter first.
    # Collapsing alpha*opacity before request transfer produces code133 instead.
    assert set(after['samples'][3::4])=={132},set(after['samples'][3::4])
    return dict(separate_alpha=alpha,opacity=.73,expected_alpha_code=132)


def profile_cases(app,folder):
    folder.mkdir()
    colors=[(10001,31003,47005,65535),(42001,17003,7005,65535)]
    converted=[]
    for index,color in enumerate(colors):
        source=probe.opened(app,fixture(folder,'color'+str(index),color),16,folder/('color'+str(index)+'.jsonl'))
        # Match the explicit authored-sRGB tag used by native paint; the fixture's
        # system sRGB profile has a distinct quantized matrix/TRC payload.
        source('assign_profile',profile='srgb')
        source('convert_profile',profile='display_p3')
        converted.append(capture(source,folder,'converted'+str(index))['samples'][:4])
    call=probe.opened(app,fixture(folder,'p3-target'),16,folder/'p3.jsonl')
    call('assign_profile',profile='display_p3')
    before=capture(call,folder,'original')
    rgba=[[v/M for v in color[:3]]+[.57] for color in colors]
    doc=[[v/M for v in color[:3]]+[.57] for color in converted]
    stroke=canonical([[11.5,12.5,1,1]],pixel=True)
    results=[]
    for tool,args in [('brush_stroke',dict(points=[[11.5,12.5]],size=1,native_stroke=stroke,rgba=rgba[0])),
                      ('fill',dict(x=0,y=0,fill_mode='selection',rgba=rgba[0])),
                      ('gradient',dict(x0=0,y0=0,x1=W,y1=H,start_rgba=rgba[0],end_rgba=rgba[1]))]:
        call(tool,layer=0,color_space='srgb',**args)
        authored=capture(call,folder,tool+'-srgb');call('undo')
        counterpart=dict(args)
        if tool=='gradient':counterpart.update(start_rgba=doc[0],end_rgba=doc[1])
        else:counterpart['rgba']=doc[0]
        call(tool,layer=0,color_space='document',**counterpart)
        mapped=capture(call,folder,tool+'-document');call('undo')
        error=max(abs(a-b) for a,b in zip(authored['samples'],mapped['samples']))
        assert error<=2,(tool,error)
        assert authored['profile']==mapped['profile']==before['profile']
        assert authored['samples'][3::4]==mapped['samples'][3::4]
        results.append(dict(tool=tool,maximum_native_code_difference=error))
    return results


def run(folder,executable):
    image=fixture(folder); rows=[]
    with probe.TracedApp(folder/'app',folder/'recovery',executable) as app:
        call=probe.opened(app,image,16,folder/'calls.jsonl')
        catalog=app.client.rpc('tools/list',{})
        (folder/'catalog.json').write_text(json.dumps(catalog,indent=2)+'\n')
        entries={t['name']:t for t in catalog['tools']}
        for tool in ['brush_stroke','eraser_stroke','dodge_burn','quick_mask_stroke']:
            assert {'stroke_version','native_stroke','pressures','times','pressure_opacity','airbrush'}<=entries[tool]['inputSchema']['properties'].keys()
        selection=set_coverage(call); before=capture(call,folder,'original')
        # Color-matched bucket clicks outside the selection are a no-op,
        # even when matching selected pixels exist elsewhere on the canvas.
        history=probe.history(call)
        call('fill',layer=0,x=0,y=0,tolerance=255,contiguous=False,fill_mode='color_match',
             rgba=[.2,.4,.7,.8],color_space='document')
        assert capture(call,folder,'excluded-seed')==before and probe.history(call)==history
        stroke=canonical([[15.13,15.71,35,.37],[12.2,11.7,17,.13]])
        fp=field(stroke);assert len(set(fp))>256
        ink=[.19377,.57319,.82731,.611]
        args=dict(layer=0,points=[[15.13,15.71]],size=35,native_stroke=stroke,rgba=ink,color_space='document',opacity=.73)
        painted,recorded=replay(app,call,folder,'NativeSoftBrush','brush_stroke',args)
        error=compare(paint_reference(before,fp,selection,ink,.73),painted,before,fp,selection)
        assert recorded['native_stroke']==stroke and coverage(call)==selection
        rows.append(dict(case='native_soft_rotation_tiny_selection',levels=len(set(fp)),maximum_error=error))
        erased,_=replay(app,call,folder,'NativeErase','eraser_stroke',{k:v for k,v in args.items() if k not in ['rgba','color_space']})
        error=compare(paint_reference(before,fp,selection,ink,.73,True),erased,before,fp,selection)
        assert erased['samples'][0::4]==before['samples'][0::4] and erased['samples'][1::4]==before['samples'][1::4] and erased['samples'][2::4]==before['samples'][2::4]
        rows.append(dict(case='native_erase_latent_rgb',maximum_error=error))
        generated=dict(layer=0,points=[[5.5,12.5],[24.5,17.5]],pressures=[.2,.9],times=[0,.2],duration=.4,
                       stroke_version=1,pressure_size=True,pressure_opacity=True,airbrush=True,size=17,hardness=21,flow=37,
                       rgba=ink,color_space='document',opacity=.73)
        painted,recorded=replay(app,call,folder,'PressureTiming','brush_stroke',generated)
        assert 'native_stroke' in recorded and 'stroke_version' not in recorded
        resolved=recorded['native_stroke']; fp2=field(resolved)
        error=compare(paint_reference(before,fp2,selection,ink,.73),painted,before,fp2,selection)
        rows.append(dict(case='generated_pressure_airbrush',dabs=len(base64.b64decode(resolved['dabs']))//32,maximum_error=error))
        modes=call('get_precision')['paint_blend_modes'];assert len(modes)==27
        for index,mode in enumerate(modes):
            painted,_=replay(app,call,folder,'Blend'+str(index),'brush_stroke',dict(args,blend_mode=mode))
            for i,s in enumerate(selection):
                if not s or not fp[i]:assert painted['samples'][4*i:4*i+4]==before['samples'][4*i:4*i+4]
        rows.append(dict(case='all_existing_paint_blends',modes=modes))
        for shape in ['linear','radial']:
            for alpha in [.0,.61]:
                painted,_=replay(app,call,folder,'Gradient'+shape+str(alpha),'gradient',dict(layer=0,x0=3.25,y0=2.5,x1=25.5,y1=21.25,
                    shape=shape,start_rgba=ink,end_rgba=[.7,.2,.3,alpha],color_space='document',opacity=.67))
                for i,s in enumerate(selection):
                    if not s:assert painted['samples'][4*i:4*i+4]==before['samples'][4*i:4*i+4]
        for burn in [False,True]:
            for tonal in ['shadows','midtones','highlights']:
                painted,_=replay(app,call,folder,'Dodge'+str(burn)+tonal,'dodge_burn',dict(layer=0,points=[[15.13,15.71]],native_stroke=stroke,size=35,
                    burn=burn,range=tonal,exposure=43))
                assert painted['samples'][3::4]==before['samples'][3::4]
        for contiguous in [False,True]:
            replay(app,call,folder,'Bucket'+str(contiguous),'fill',dict(layer=0,x=9,y=9,tolerance=255,
                contiguous=contiguous,fill_mode='color_match',rgba=ink,color_space='document'))
        rows.append(dict(case='gradient_bucket_six_tone_modes_actions'))
        call('deselect');call('transform_layer',layer=0,scale=.5,sampler='nearest')
        small=capture(call,folder,'small');assert small['width']<W
        outside=canonical([[31.5,31.5,1,1]],pixel=True)
        history=probe.history(call)
        call('eraser_stroke',layer=0,points=[[31.5,31.5]],size=1,native_stroke=outside)
        assert capture(call,folder,'outside-erase')==small and probe.history(call)==history
        call('brush_stroke',layer=0,points=[[31.5,31.5]],size=1,native_stroke=outside,rgba=ink,color_space='document')
        expanded=capture(call,folder,'expanded');assert expanded['width']>small['width'] or expanded['height']>small['height']
        call('undo');assert capture(call,folder,'expansion-undo')==small
        call('fill',layer=0,x=31,y=31,tolerance=0,contiguous=True,fill_mode='color_match',rgba=ink,color_space='document')
        filled=capture(call,folder,'expanded-fill');assert filled['width']>small['width'] or filled['height']>small['height']
        rows.append(dict(case='brush_fill_storage_expansion_absent_erase_noop'))
        snapshot=call('create_snapshot',name='Native paint expanded')['snapshot_id']
        path=folder/'native-paint.drsnapshot';call('save_snapshot',snapshot_id=snapshot,path=str(path))
        call('undo'); loaded=call('load_snapshot',path=str(path))['snapshot_id'];call('restore_snapshot',snapshot_id=loaded)
        assert capture(call,folder,'snapshot-restored')==filled
        reopen=probe.opened(app,folder/'expanded-fill.rz',16,folder/'reopen.jsonl')
        assert capture(reopen,folder,'native-reopened')==filled
        call('write_recovery',wait=True)
        status=f['wait_for'](lambda:call('get_recovery_status')['document'],lambda r:r.get('state')=='protected' and not r.get('pending'),timeout=60,detail='native paint recovery')
        recovery_id=status['recovery_id']
        p3=profile_cases(app,folder/'p3')
        scalars=scalar_cases(app,folder/'scalars')
        boundary=fill_opacity_boundary(app,folder/'fill-boundary')
        app.crash()
    with probe.TracedApp(folder/'recovered-app',folder/'recovery',executable) as app:
        identifier=app.call('recover_document',recovery_id=recovery_id,show=True)['document']['id']
        call=f['call_for'](app,identifier,folder/'recovered.jsonl')
        assert capture(call,folder,'recovered')==filled
    return dict(cases=rows,display_p3=p3,scalar_targets=scalars,fill_opacity_boundary=boundary,native_snapshot_recovery=True,fixture_sha256=f['digest'](image))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app',type=Path,default=app_under_test.default_app_executable())
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--require-current',action='store_true')
    args=parser.parse_args();folder=Path(tempfile.mkdtemp(prefix='rasterize-phase9h-mcp-'))
    print(folder,flush=True); before=dependencies(); report=dict(passed=False,scratch=str(folder),app_sha256=f['digest'](args.app))
    try:
        report['result']=run(folder,args.app.resolve()); after=dependencies()
        report['source_changes']=sorted(k for k in before.keys()|after.keys() if before.get(k)!=after.get(k))
        if args.require_current:assert not report['source_changes'],report['source_changes']
        assert report['app_sha256']==f['digest'](args.app)
        report['passed']=True
    except BaseException as error:
        report['error']=repr(error);raise
    finally:
        report['all_owned_apps_exited']=all(a.process is None or a.process.poll() is not None for a in shared.HostApp.owners)
        args.report.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
        print(args.report,flush=True)
if __name__=='__main__':main()
