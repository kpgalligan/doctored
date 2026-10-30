#!/usr/bin/env python3
"""Owned production MCP qualification for native selections, masks and channels.

PNG/native framing and scalar/coverage arithmetic are checked independently.
Camera-container provenance and actual UI gestures have separate companion
receipts; this runner covers public tools, Actions, Undo and persistence.
"""
import argparse
import base64
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
spec = importlib.util.spec_from_file_location('phase9g_existing_support',ROOT/'scripts/verify_phase9f.py')
f = importlib.util.module_from_spec(spec)
spec.loader.exec_module(f)
shared, probe, fixtures = f.shared, f.probe, f.fixtures
oracle = shared.load('phase9g_independent_oracles',ROOT/'scripts/native-selection-probe/oracles.py')
W,H = f.WIDTH,f.HEIGHT
N = W*H


def dependencies():
    inputs = f.dependencies()
    for path in [Path(__file__).resolve(),ROOT/'scripts/verify_phase9f.py',ROOT/'scripts/native-selection-probe/oracles.py']:
        inputs[str(path.relative_to(ROOT))] = f.digest(path)
    return inputs


def selected(call):
    result = call('get_selection',points=[dict(x=x,y=y) for y in range(H) for x in range(W)])
    return tuple(s['coverage16'] for s in result['samples'])


def select16(call,values):
    call('set_quick_mask',active=True)
    call('quick_mask_stroke',width=W,height=H,coverage_rle16=oracle.rle16(values))
    call('set_quick_mask',active=False)
    assert selected(call) == tuple(values)


def coverage(depth=16):
    result = []
    codes = [0,1,127,257,12345,32769,65534,65535]
    for y in range(H):
        for x in range(W):
            value = codes[(x+3*y)%len(codes)] if 3 <= x < W-3 and 2 <= y < H-2 else 0
            result.append(value if depth == 16 else ((value+128)//257)*257)
    return tuple(result)


class Captures:
    def __init__(self,folder):
        self.folder,self.counter = folder,0
    def native(self,call,label):
        self.counter += 1
        path = self.folder/f'{self.counter:04}-{label}.rz'
        call('save_copy',path=str(path))
        return probe.read_native(path)
    def channel(self,call,label,index=0):
        return oracle.channel(self.native(call,label),index)
    def raw(self,call,label,layer=0):
        parsed = self.native(call,label)
        image = probe.png_samples(parsed['document']['fallbacks'][layer])
        return tuple(image['samples'])


def history_action(app,call,name,operation,read):
    before = read('before')
    history = probe.history(call)
    app.call('start_recording')
    response = operation()
    after = read('after')
    action = app.call('stop_recording',name=name)
    changed = after != before
    assert probe.history(call)[1] == history[1]+int(changed),(name,response)
    if changed:
        assert len(action['steps']) == 1,(name,action)
        call('undo'); assert read('undo') == before,name
        call('redo'); assert read('redo') == after,name
        call('undo');call('run_action',name=name);assert read('replay') == after,name
    return after,changed,action


def source_selections(app,folder,depth):
    path = f.fixture(folder,depth,alpha=True)
    call = probe.opened(app,path,depth,folder/'source-calls.jsonl')
    captures = Captures(folder)
    original = captures.raw(call,'source')
    maximum = (1<<depth)-1
    scale = 257 if depth == 8 else 1
    call('set_layer_properties',index=0,offset_x=-2,offset_y=1)
    alpha = tuple(original[((y-1)*W+x+2)*4+3]*scale if y>=1 and x+2<W else 0 for y in range(H) for x in range(W))
    call('load_selection',**{'from':'layer_alpha'},layer=0)
    assert selected(call) == alpha
    call('save_selection',name='Native Alpha')
    assert captures.channel(call,'alpha-channel') == tuple(v//scale for v in alpha)
    call('modify_selection',operation='invert');assert selected(call) == tuple(65535-v for v in alpha)
    call('load_selection',channel='Native Alpha');assert selected(call) == alpha
    call('add_layer_mask',layer=0,kind='from_selection')
    call('load_selection',**{'from':'layer_mask'},layer=0)
    # Native masks keep canvas storage; byte masks keep raster storage and map
    # their own local pixels. Both should still reproduce the supplied plane.
    assert selected(call) == alpha
    assert captures.raw(call,'unchanged-source') == original
    call('remove_layer_mask',layer=0)
    groups = ['raw layer alpha and channel/mask selection transfer with offset']

    # Composite selection must ignore latent color at alpha0, while layer
    # inspection retains the native raw samples above.
    opaque = f.fixture(folder,depth)
    call = probe.opened(app,opaque,depth,folder/'plane-calls.jsonl')
    raw = captures.raw(call,'opaque')
    for plane,index in [('red',0),('green',1),('blue',2),('alpha',3)]:
        call('load_selection',**{'from':'plane'},plane=plane)
        assert selected(call) == tuple(v*scale for v in raw[index::4]),plane
    call('load_selection',**{'from':'plane'},plane='luma')
    expected = tuple(oracle.q(sum(v*k for v,k in zip(raw[i:i+3],map(oracle.f32,[0.2126,0.7152,0.0722])))*scale) for i in range(0,len(raw),4))
    assert max(abs(a-b) for a,b in zip(selected(call),expected)) <= scale
    groups.append('composite alpha/RGB/luminance source planes')

    # A one-code distinction shares the same displayed byte in native mode.
    values=[]
    for y in range(H):
        for x in range(W):
            code = (30000+[0,1,3,0][x%4]) if depth==16 else (100+[0,1,3,0][x%4])
            values.append((code,code,code,maximum))
    sampled=folder/f'fine-selection-{depth}.png'
    sampled.write_bytes(fixtures.png(W,H,depth,values,fixtures.srgb_profile()))
    call=probe.opened(app,sampled,depth,folder/'fine-source-calls.jsonl')
    tolerance=1/257 if depth==16 else 1
    call('select_magic_wand',x=0,y=0,tolerance=tolerance,contiguous=True)
    assert selected(call)==tuple(65535 if x<2 else 0 for y in range(H) for x in range(W))
    sample=[30001]*3 if depth==16 else [101]*3
    field='samples16' if depth==16 else 'samples'
    call('select_color_range',range='sampled',**{field:[sample]},tolerance=0,softness=0)
    assert selected(call)==tuple(65535 if x%4==1 else 0 for y in range(H) for x in range(W))
    call('quick_select',seeds=[[1,0]],tolerance=0,edge=0)
    assert selected(call)==tuple(65535 if x==1 else 0 for y in range(H) for x in range(W))
    groups.append('wand/native sampled Color Range/Quick sub-byte boundaries')
    return groups


def selection_edits(app,call,captures,depth):
    values=coverage(16)
    select16(call,values)
    call('save_selection',name='Coverage')
    expected=values if depth==16 else tuple((v+128)//257 for v in values)
    assert captures.channel(call,'coverage-saved')==expected
    select16(call,values)
    for dx in [1,0.5]:
        call('transform_selection',translate_x=dx,filter='nearest' if dx==1 else 'bilinear')
        assert selected(call)==oracle.translate(values,W,H,dx)
        call('undo');assert selected(call)==values
    for kind in ['grow','shrink','border']:
        args={'width':0.731} if kind=='border' else {'radius':0.731}
        call('modify_selection',operation=kind,**args)
        assert max(abs(a-b) for a,b in zip(selected(call),oracle.morphology(values,W,H,kind,0.731)))<=1
        call('undo');assert selected(call)==values
    call('set_quick_mask',active=True)
    gray=tuple(min(v,12345) for v in values)
    alpha=tuple(max(g,32769) for g in gray)
    app.call('start_recording')
    call('quick_mask_stroke',width=W,height=H,coverage_rle16=oracle.rle16(gray,alpha))
    action=app.call('stop_recording',name=f'Native Quick Mask {depth}')
    expected=tuple(oracle.q(g+b*(65535-a)/65535) for b,g,a in zip(values,gray,alpha))
    assert selected(call)==expected
    assert action['steps'][0]['arguments']['coverage_rle16']==oracle.rle16(gray,alpha)
    call('undo');assert selected(call)==values
    call('run_action',name=f'Native Quick Mask {depth}');assert selected(call)==expected
    call('set_quick_mask',active=False)
    return ['u16 Quick Mask Actions/Undo','native affine/morphology selection','native coverage channel transfer']


def scalar_filters(app,call,captures,depth):
    call('deselect')
    call('add_channel',name='Gray',**{'from':'plane'},plane='red',overlay_color='#13579B',overlay_opacity=0.37)
    channels=call('list_channels')['channels']
    index=next(i for i,c in enumerate(channels) if c['name']=='Gray')
    original=captures.channel(call,'gray-original',index)
    source=captures.raw(call,'gray-raster')
    weights=coverage(depth)
    report=[]
    cases=f.filters() if depth==16 else [entry for entry in f.filters() if entry[0] in {'invert','levels','blur','sepia','threshold','channel_mixer'}]
    for name,arguments in cases:
        call('deselect');before_history=probe.history(call)
        call('apply_filter',layer=0,target='channel:Gray',**arguments)
        full=captures.channel(call,name+'-full',index)
        if full!=original:
            call('undo');assert captures.channel(call,name+'-full-undo',index)==original
        else:
            assert probe.history(call)==before_history
        if name=='sepia':
            maximum=(1<<depth)-1
            for value,actual in zip(original,full):
                triple=[min(maximum,value*k) for k in [1.351,1.203,0.937]]
                if depth==8: triple=[math.floor(v+0.5) for v in triple]
                reference=math.floor(sum(v*k for v,k in zip(triple,[0.2126,0.7152,0.0722]))+0.5)
                assert abs(reference-actual)<=2,(value,actual,reference)
        select16(call,weights)
        read=lambda label:captures.channel(call,name+'-'+label,index)
        after,changed,action=history_action(app,call,f'Scalar {depth} {name}',
            lambda:call('apply_filter',layer=0,target='channel:Gray',**arguments),read)
        difference=oracle.assert_gate(original,full,after,weights)
        assert captures.raw(call,name+'-other-planes')==source
        if changed:call('undo');assert read('reset')==original
        report.append(dict(filter=name,changed=changed,maximum_gate_error=difference,action_steps=len(action['steps'])))
    return report,index,original


def rgba_masks(app,folder):
    path=f.fixture(folder,16,alpha=True)
    call=probe.opened(app,path,16,folder/'rgba-calls.jsonl');captures=Captures(folder)
    call('set_layer_properties',index=0,offset_x=-3,offset_y=2)
    original=captures.raw(call,'rgba-source');weights=coverage()
    for component,target in enumerate(['red','green','blue','alpha']):
        select16(call,weights)
        after,changed,_=history_action(app,call,'RGBA '+target,
            lambda:call('apply_filter',layer=0,target=target,filter='invert'),lambda label:captures.raw(call,target+'-'+label))
        assert changed
        for y in range(H):
            for x in range(W):
                i=y*W+x;cx,cy=x-3,y+2;c=weights[cy*W+cx] if 0<=cx<W and 0<=cy<H else 0
                for k in range(4):
                    before=original[i*4+k]
                    expected=oracle.q(before+(65535-2*before)*c/65535) if k==component else before
                    assert abs(after[i*4+k]-expected)<=(1 if k==component and c not in (0,65535) else 0),(target,x,y,k)
        call('undo')
    select16(call,weights);call('add_layer_mask',layer=0,kind='from_selection')
    call('set_mask_properties',layer=0,density=0.625,feather=0,linked=False)
    call('set_layer_mask_enabled',layer=0,enabled=False)
    call('deselect');call('remove_layer_mask',layer=0,apply=True)
    baked=captures.raw(call,'baked-mask')
    for y in range(H):
        for x in range(W):
            i=y*W+x;cx,cy=x-3,y+2;c=weights[cy*W+cx] if 0<=cx<W and 0<=cy<H else 0
            assert baked[i*4:i*4+3]==original[i*4:i*4+3]
            assert abs(baked[i*4+3]-oracle.q(original[i*4+3]*(1-0.625*(1-c/65535))))<=1
    return ['four raw RGBA selected inversions preserve other/hidden/tiny components','effective disabled-mask Apply preserves RGB']


def scalar_automatic(app, folder):
    """All Auto commands independently measure raw scalar storage/placement."""
    codes = tuple(2001+i*91 for i in range(N))
    pixels = [(code, (code+11003)%65536, (code+17009)%65536, code) for code in codes]
    pixels[0] = (*pixels[0][:3], 0)
    pixels[1] = (*pixels[1][:3], 1)
    path = folder/'scalar-auto-source.png'
    path.write_bytes(fixtures.png(W,H,16,pixels,fixtures.srgb_profile()))
    call = probe.opened(app,path,16,folder/'scalar-auto-calls.jsonl')
    captures = Captures(folder/'scalar-auto')
    captures.folder.mkdir()
    call('add_channel',name='Auto',**{'from':'plane'},plane='red')
    select16(call,codes)
    call('add_layer_mask',layer=0,kind='from_selection')
    call('set_mask_properties',layer=0,linked=False,density=0.371,feather=0.731)
    call('transform_mask',layer=0,matrix=[1,0,0,1,2,-1],filter='nearest')
    call('set_layer_mask_enabled',layer=0,enabled=False)
    call('set_layer_properties',index=0,offset_x=-3,offset_y=2)
    weights = tuple((32769 if x%2==0 else 65535) if 4<=x<18 and 3<=y<12 else 0
                    for y in range(H) for x in range(W))
    select16(call,weights)

    def state(label):
        parsed = captures.native(call,label)['document']
        raw = tuple(probe.png_samples(parsed['fallbacks'][0])['samples'])
        mask = dict(parsed['layers'][0]['mask'])
        data = mask.pop('data')
        mask['samples'] = struct.unpack('<'+'H'*(mask['width']*mask['height']),data)
        channel = struct.unpack('<'+'H'*N,parsed['channels'][0]['data'])
        return raw,mask,channel

    initial = state('initial')
    cases = []
    for target in ['channel:Auto','mask','alpha']:
        original = initial[2] if target.startswith('channel:') else initial[1]['samples'] if target=='mask' else initial[0][3::4]
        if target=='mask':
            width,height,dx,dy = (initial[1][key] for key in ['width','height','x','y'])
            assert (dx,dy)==(2,-1),initial[1]
        else:
            width,height = W,H
            dx,dy = (0,0) if target.startswith('channel:') else (-3,2)
        write_weights = tuple(weights[(y+dy)*W+x+dx] if 0<=x+dx<W and 0<=y+dy<H else 0
                              for y in range(height) for x in range(width))
        for tool in ['auto_tone','auto_contrast','auto_color']:
            results = []
            for scope in ['whole_target','selection']:
                name = f'{tool}-{target}-{scope}'
                response = {}
                def operation():
                    response.update(call(tool,layer=0,target=target,clip=0.013,analysis_scope=scope))
                    return response
                after,changed,action = history_action(app,call,name,operation,state)
                assert changed and response['changed'],(name,response)
                assert response['analysis_scope']==scope and response['write_scope']=='selection'
                arguments = action['steps'][0]['arguments']
                assert arguments['target']==target and arguments['analysis_scope']==scope
                analysis_weights = write_weights if scope=='selection' else (65535,)*len(original)
                low,high = oracle.scalar_auto_endpoints(original,analysis_weights,0.013)
                assert response['black']==[low]*3 and response['white']==[high]*3,(name,response,low,high)
                # Opaque grayscale has identical neutral means in every channel.
                assert response['gamma']==[1.0]*3,(name,response)
                actual = after[2] if target.startswith('channel:') else after[1]['samples'] if target=='mask' else after[0][3::4]
                expected = oracle.scalar_levels(original,write_weights,low,high,1)
                assert max(abs(a-b) for a,b in zip(actual,expected,strict=True))<=1,(name,'derived Levels writeback')
                assert all(a==b for a,b,c in zip(actual,original,write_weights,strict=True) if c==0)
                if target=='alpha':
                    assert after[1:]==initial[1:]
                    assert all(after[0][k::4]==initial[0][k::4] for k in range(3))
                elif target=='mask':
                    assert after[0]==initial[0] and after[2]==initial[2]
                    assert {k:v for k,v in after[1].items() if k!='samples'}=={k:v for k,v in initial[1].items() if k!='samples'}
                else:
                    assert after[:2]==initial[:2]
                call('undo');assert state(name+'-reset')==initial
                # Reapply exactly the reported endpoints as public Levels.
                call('apply_filter',layer=0,target=target,filter='levels',black=low,white=high,gamma=1)
                assert state(name+'-explicit-levels')==after,(name,'reported levels must reproduce Auto exactly')
                call('undo');assert state(name+'-levels-undo')==initial
                results.append(actual)
                cases.append(dict(tool=tool,target=target,analysis_scope=scope,black=low,white=high,
                                  maximum_oracle_error=max(abs(a-b) for a,b in zip(actual,expected,strict=True)),
                                  exact_derived_levels=True,action_steps=len(action['steps'])))
            assert results[0]!=results[1],(target,tool,'analysis scopes must have visible independent effects')
    return cases


def scalar_tools(app,call,captures,index,original):
    target='channel:Gray';weights=coverage()
    groups=[]
    for kind in ['invert','feather','smooth','grow','shrink','border']:
        call('deselect');call('modify_scalar',target=target,operation=kind,radius=1.3)
        full=captures.channel(call,kind+'-full',index)
        if full!=original:call('undo')
        select16(call,weights)
        after,changed,_=history_action(app,call,'Scalar morphology '+kind,
            lambda:call('modify_scalar',target=target,operation=kind,radius=1.3),
            lambda label:captures.channel(call,kind+'-'+label,index))
        oracle.assert_gate(original,full,after,weights)
        if changed:call('undo')
        groups.append('scalar '+kind)
    call('deselect');call('add_channel',name='Stroke',**{'from':'empty'})
    stroke_index=len(call('list_channels')['channels'])-1
    points=[[7,7],[16,7]]
    call('brush_stroke',layer=0,target='channel:Stroke',points=points,size=4,opacity=1)
    footprint=captures.channel(call,'full-brush',stroke_index);assert any(footprint)
    call('undo');select16(call,weights)
    after,changed,_=history_action(app,call,'Native scalar brush',
        lambda:call('brush_stroke',layer=0,target='channel:Stroke',points=points,size=4,opacity=0.37),
        lambda label:captures.channel(call,'brush-'+label,stroke_index))
    expected=tuple(oracle.q(a*c*0.37/65535) for a,c in zip(footprint,weights))
    assert max(abs(a-b) for a,b in zip(after,expected))<=1
    assert changed
    groups.append('scalar brush sampled footprint/selection/opacity Actions')
    call('deselect')
    select16(call,weights)
    before=captures.channel(call,'before-fill',index)
    after,changed,_=history_action(app,call,'Native scalar fill',
        lambda:call('fill',layer=0,target=target,x=6,y=6,tolerance=255,contiguous=False,
                    rgba=[0.314159,0.314159,0.314159,0.731],color_space='document'),
        lambda label:captures.channel(call,'fill-'+label,index))
    ink=oracle.q(0.314159*65535)
    expected=tuple(oracle.q(v*(1-c/65535*0.731)+ink*c/65535*0.731) for v,c in zip(before,weights))
    assert max(abs(a-b) for a,b in zip(after,expected))<=1
    groups.append('scalar native-color bucket fill with selected writeback')
    return groups


def channel_math(app,call,captures,index):
    call('deselect');source=captures.raw(call,'math-source');base=captures.channel(call,'math-base',index)
    weights=coverage();select16(call,weights)
    call('apply_image',source=0,source_plane='red',target='channel:Gray',blend_mode='multiply',opacity=0.371)
    output=captures.channel(call,'apply-image',index)
    for i,(a,actual,c) in enumerate(zip(base,output,weights)):
        b=source[i*4];mixed=oracle.q(a*(1-0.371)+a*b/65535*0.371);expected=oracle.q(a*(1-c/65535)+mixed*c/65535)
        assert abs(actual-expected)<=1
    call('undo');call('deselect')
    call('calculations',source1=0,source1_plane='red',source2=0,source2_plane='blue',blend_mode='screen',opacity=0.731,result='selection')
    calculated=selected(call)
    for i,actual in enumerate(calculated):
        a,b=source[i*4+2]/65535,source[i*4]/65535
        assert abs(actual-oracle.q((a*(1-0.731)+(a+b-a*b)*0.731)*65535))<=1
    call('deselect');before_count=len(call('list_channels')['channels'])
    call('add_luminosity_masks');native=captures.native(call,'luminosity')
    assert len(native['document']['channels'])==before_count+9
    for group,band in enumerate(['Lights','Darks','Midtones']):
        for level in range(1,4):
            index=before_count+group*3+level-1;channel=native['document']['channels'][index]
            assert channel['name']==f'{band} {level}'
            for i,actual in enumerate(oracle.channel(native,index)):
                luma=oracle.q(sum(v*k for v,k in zip(source[i*4:i*4+3],map(oracle.f32,[0.2126,0.7152,0.0722]))))/65535
                expected=luma**level if group==0 else (1-luma)**level if group==1 else 1-luma**(level+1)-(1-luma)**(level+1)
                assert abs(actual-oracle.q(expected*65535))<=1
    return ['selected Apply Image scalar multiply','Calculations selection Screen','all nine native luminosity masks']


def display_planes(app,call,captures,folder,depth):
    native=captures.native(call,'display-source')
    index=next(i for i,c in enumerate(native['document']['channels']) if c['name']=='Gray')
    channel=oracle.channel(native,index)
    raw=tuple(probe.png_samples(native['document']['fallbacks'][0])['samples'])
    before=probe.history(call)
    cases=[('saved-channel',dict(channel='Gray'),channel),('raw-red-plane',dict(channel='red',layer=0),raw[0::4])]
    for label,arguments,values in cases:
        result=app.client.rpc('tools/call',dict(name='render',arguments=dict(document_id=call.identifier,max_side=1024,**arguments)))
        assert not result.get('isError'),result
        data=base64.b64decode(next(c['data'] for c in result['content'] if c['type']=='image'))
        (folder/(label+'.png')).write_bytes(data)
        w,h,channels,pixels=oracle.png(data);assert (w,h)==(W,H)
        for i,value in enumerate(values):
            expected=(value+128)//257 if depth==16 else value
            assert all(abs(v-expected)<=1 for v in pixels[i*channels:i*channels+min(channels,3)]),(label,i,value,pixels[i*channels:i*channels+channels])
    assert probe.history(call)==before
    return ['independent saved-channel and per-layer scalar display PNGs']


def persistence(app,call,captures,folder,depth):
    call('set_channel_options',channel='Gray',overlay_color='#3579BD',overlay_opacity=0.625,color_indicates='selected')
    values=coverage();select16(call,values);call('set_quick_mask',active=True)
    expected=captures.native(call,'persistence')
    gray_index=next(i for i,c in enumerate(expected['document']['channels']) if c['name']=='Gray')
    metadata=oracle.metadata(expected['document']['channels'][gray_index]);codes=oracle.channel(expected,gray_index)
    assert metadata['color']==[0x35,0x79,0xBD] and metadata['opacity']==0.625 and metadata['selected']
    if depth==16:assert expected['version']==25
    saved=folder/'native-reopen.rz';call('save_copy',path=str(saved))
    reopened=probe.opened(app,saved,depth,folder/'native-reopen.jsonl')
    native=captures.native(reopened,'reopened')
    assert oracle.channel(native,gray_index)==codes and oracle.metadata(native['document']['channels'][gray_index])==metadata
    snapshot=call('create_snapshot',name='Native scalar selection')['snapshot_id'];archive=folder/'scalar.drsnapshot'
    call('save_snapshot',snapshot_id=snapshot,path=str(archive))
    call('set_quick_mask',active=False);call('deselect');call('invert_channel',channel='Gray')
    loaded=call('load_snapshot',path=str(archive))['snapshot_id'];call('restore_snapshot',snapshot_id=loaded)
    assert selected(call)==values and call('get_selection')['quick_mask_active']
    restored=captures.native(call,'restored')
    assert oracle.channel(restored,gray_index)==codes and oracle.metadata(restored['document']['channels'][gray_index])==metadata
    call('write_recovery',wait=True)
    status=shared.wait_for(lambda:call('get_recovery_status')['document'],lambda r:r.get('state')=='protected' and not r.get('pending'),timeout=60,detail='native scalar recovery')
    return dict(recovery_id=status['recovery_id'],channel=gray_index,codes=codes,metadata=metadata,selection=values)


def run_depth(folder,executable,depth):
    folder.mkdir(parents=True)
    with probe.TracedApp(folder/'app',folder/'recovery',executable) as app:
        groups=source_selections(app,folder,depth)
        fixture=f.fixture(folder,depth)
        call=probe.opened(app,fixture,depth,folder/'calls.jsonl');captures=Captures(folder)
        catalog=app.client.rpc('tools/list',{});(folder/'catalog.json').write_text(json.dumps(catalog,indent=2)+'\n')
        groups+=selection_edits(app,call,captures,depth)
        filters,index,original=scalar_filters(app,call,captures,depth)
        if depth==16:
            groups+=rgba_masks(app,folder)
            automatic=scalar_automatic(app,folder)
            groups+=scalar_tools(app,call,captures,index,original)
            groups+=channel_math(app,call,captures,index)
            shared.refusal(call,'apply_image',target='channel:Gray',blend_mode='hue')
            shared.refusal(call,'modify_scalar',target='layer',operation='grow',radius=1)
        call('set_quick_mask',active=True)
        shared.refusal(call,'apply_filter',layer=0,target='channel:Gray',filter='invert')
        shared.refusal(call,'quick_mask_stroke',width=W,height=H,coverage_rle16=base64.b64encode(struct.pack('<IHH',N,65535,1)).decode())
        call('set_quick_mask',active=False)
        groups+=display_planes(app,call,captures,folder,depth)
        saved=persistence(app,call,captures,folder,depth)
        app.crash()
    with probe.TracedApp(folder/'recovered-app',folder/'recovery',executable) as app:
        available=app.call('get_recovery_status')['available']
        assert saved['recovery_id'] in {r['recovery_id'] for r in available}
        identifier=app.call('recover_document',recovery_id=saved['recovery_id'],show=True)['document']['id']
        call=shared.call_for(app,identifier,folder/'recovery-calls.jsonl')
        assert selected(call)==saved['selection'] and call('get_selection')['quick_mask_active']
        native=captures.native(call,'recovered');index=saved['channel']
        assert oracle.channel(native,index)==saved['codes'] and oracle.metadata(native['document']['channels'][index])==saved['metadata']
    return dict(depth=depth,groups=groups,filters=filters,
                scalar_automatic=automatic if depth==16 else [],native_snapshot_recovery=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app',type=Path,default=app_under_test.default_app_executable())
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--require-current',action='store_true')
    args=parser.parse_args();folder=Path(tempfile.mkdtemp(prefix='rasterize-phase9g-'))
    before=dependencies();report=dict(passed=False,scratch=str(folder),app_sha256=f.digest(args.app))
    print('Owned native scalar qualification:',folder,flush=True)
    try:
        report['cases']=[run_depth(folder/f'rgba{d}',args.app.resolve(),d) for d in (8,16)]
        after=dependencies();changes=sorted(k for k in before.keys()|after.keys() if before.get(k)!=after.get(k))
        report['source_check']=dict(files=len(before),changes=changes,file_set_exact=before.keys()==after.keys())
        if args.require_current:assert not changes,changes
        assert f.digest(args.app)==report['app_sha256'];report['passed']=True
    except BaseException as error:
        report['error']=repr(error);raise
    finally:
        report['all_owned_apps_exited']=all(a.process is None or a.process.poll() is not None for a in shared.HostApp.owners)
        args.report.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
        print('Native scalar receipt:',args.report,flush=True)


if __name__=='__main__':main()
