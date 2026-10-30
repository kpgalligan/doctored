#!/usr/bin/env python3
"""Check the independent native26/27/30/31/33/34/37 blend, 32/LCLP clipping, 35/LBIF Blend If and 36/LSTY layer-style decoders against synthetic valid/corrupt tails.

Schema7 (v37) adds no mode to schema6's twenty-six; what it adds is the kind-3
adjustment TARGET, which every older schema still refuses
(designs/phase-10d-adjustment-blending-status.md).
"""
import importlib.util
from pathlib import Path
import struct
import sys
sys.dont_write_bytecode = True
root = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("native", root / "scripts/live-filter-probe/native.py")
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)
def tail(records, schema=1, count=None):
    return b"LBLD" + struct.pack("<II", schema, len(records) if count is None else count) + b"".join(struct.pack("<II", *r) for r in records)
def layers():
    return {0: dict(kind=0, blend=0), 1: dict(kind=1, blend=0), 2: dict(kind=2, blend=27), 3: dict(kind=3, blend=0)}
valid = native.Reader(tail([(0,1),(1,2)])); result = layers()
assert native.read_layer_blending(valid,result)==[(0,1),(1,2)]
assert result[0]['blend']==1 and result[1]['blend']==2 and valid.at==len(valid.data)
bad = [tail([],count=0),tail([(0,1)],schema=2),tail([(0,1)],count=5),tail([(4,1)]),tail([(0,3)]),
       tail([(0,1),(0,2)]),tail([(1,1),(0,2)]),tail([(2,1)]),tail([(3,2)]),tail([(0,1),(3,2)]),tail([(0,1)])[:-1]]
for data in bad:
    result=layers(); before={k:dict(v) for k,v in result.items()}
    try: native.read_layer_blending(native.Reader(data), result)
    except AssertionError: pass
    else: raise AssertionError("Malformed blend tail was accepted")
    assert result==before, "Refused tail changed independent decoded properties"
print(f"PASS: independent native26 LBLD decoder valid table and {len(bad)} malformed/atomic-refusal cases")

valid27 = [(0,3),(1,9)]
reader = native.Reader(tail(valid27, schema=2)); result = layers()
assert native.read_layer_blending(reader, result, 27) == valid27
assert result[0]['blend'] == 3 and result[1]['blend'] == 9
bad27 = [tail([],schema=2,count=0),tail([(0,3)],schema=1),tail([(0,3)],schema=3),
         tail([(0,3)],schema=2,count=5),tail([(4,3)],schema=2),
         tail([(0,3),(0,9)],schema=2),tail([(1,3),(0,9)],schema=2),
         tail([(2,3)],schema=2),tail([(3,9)],schema=2),tail([(0,3),(3,9)],schema=2),
         tail([(0,3)],schema=2)[:-1]]
bad27 += [tail([(0,mode)],schema=2) for mode in (0,10,14,23,24,25,26,27,0xffffffff)]
for data in bad27:
    result=layers(); before={k:dict(v) for k,v in result.items()}
    try: native.read_layer_blending(native.Reader(data), result, 27)
    except AssertionError: pass
    else: raise AssertionError("Malformed separable blend tail was accepted")
    assert result==before, "Refused separable tail changed decoded properties"
for mode in range(1,10):
    result=layers()
    assert native.read_layer_blending(native.Reader(tail([(0,mode)],schema=2)),result,27)==[(0,mode)]
print(f"PASS: independent native27 schema2 all nine modes and {len(bad27)} malformed/atomic-refusal cases")

valid30 = [(0,10),(1,15)]
reader = native.Reader(tail(valid30, schema=3)); result = layers()
assert native.read_layer_blending(reader,result,30)==valid30
bad30 = [tail([],schema=3),tail([(0,10)],schema=1),tail([(0,10)],schema=2),
         tail([(0,10)],schema=4),tail([(0,10)],schema=3,count=5),
         tail([(0,10),(0,15)],schema=3),tail([(1,10),(0,15)],schema=3),
         tail([(2,10)],schema=3),tail([(3,15)],schema=3),tail([(0,10),(3,15)],schema=3),
         tail([(0,10)],schema=3)[:-1]]
bad30 += [tail([(0,mode)],schema=3) for mode in (0,13,14,16,17,18,19,20,21,22,23,24,25,26,27,0xffffffff)]
for data in bad30:
    result=layers();before={k:dict(v) for k,v in result.items()}
    try: native.read_layer_blending(native.Reader(data),result,30)
    except AssertionError: pass
    else: raise AssertionError("Malformed four-mode blend tail was accepted")
    assert result==before
for mode in (1,2,3,4,5,6,7,8,9,10,11,12,15):
    result=layers();assert native.read_layer_blending(native.Reader(tail([(0,mode)],schema=3)),result,30)==[(0,mode)]
for version,schema in ((26,1),(27,2)):
    for mode in (10,11,12,15):
        try: native.read_layer_blending(native.Reader(tail([(0,mode)],schema=schema)),layers(),version)
        except AssertionError: pass
        else: raise AssertionError("An older schema acquired a new mode")
print(f"PASS: independent native30 schema3 all thirteen modes, {len(bad30)} malformed/atomic-refusal cases, and eight frozen-old-schema refusals")

# Schema4 is exactly the five-mode addition; no historical schema widens.
valid31 = [(0,13),(1,22)]
reader=native.Reader(tail(valid31,schema=4));result=layers()
assert native.read_layer_blending(reader,result,31)==valid31
assert reader.at==len(reader.data) and result[0]['blend']==13 and result[1]['blend']==22
bad31=[tail([],schema=4),tail([(0,13)],schema=4,count=5),tail([(4,13)],schema=4),
       tail([(0,13),(0,22)],schema=4),tail([(1,18),(0,20)],schema=4),
       tail([(2,19)],schema=4),tail([(3,22)],schema=4),tail([(0,13),(3,22)],schema=4),
       tail([(0,13),(1,21)],schema=4)]
bad31 += [tail([(0,13)],schema=schema) for schema in (1,2,3,5,0xffffffff)]
bad31 += [tail([(0,mode)],schema=4) for mode in (0,14,16,17,21,23,24,25,26,27,0xffffffff)]
bad31 += [tail(valid31,schema=4)[:end] for end in range(len(tail(valid31,schema=4)))]
for data in bad31:
    result=layers();before={k:dict(v) for k,v in result.items()}
    try:native.read_layer_blending(native.Reader(data),result,31)
    except (AssertionError,struct.error):pass
    else:raise AssertionError('Malformed five-mode blend tail accepted')
    assert result==before,'Refused schema4 tail partially changed decoded properties'
for mode in (1,2,3,4,5,6,7,8,9,10,11,12,13,15,18,19,20,22):
    result=layers()
    assert native.read_layer_blending(native.Reader(tail([(0,mode)],schema=4)),result,31)==[(0,mode)]
for version,schema in ((26,1),(27,2),(30,3)):
    for mode in (13,18,19,20,22):
        result=layers();before={k:dict(v) for k,v in result.items()}
        try:native.read_layer_blending(native.Reader(tail([(0,mode)],schema=schema)),result,version)
        except AssertionError:pass
        else:raise AssertionError('An older schema acquired a five-mode addition')
        assert result==before
print(f'PASS: independent native31 schema4 all eighteen modes, {len(bad31)} malformed/atomic-refusal cases, and fifteen frozen-old-schema refusals')

# Schema5 is exactly the four nonseparable HSL modes; no historical schema widens.
valid33 = [(0,23),(1,26)]
reader=native.Reader(tail(valid33,schema=5));result=layers()
assert native.read_layer_blending(reader,result,33)==valid33
assert reader.at==len(reader.data) and result[0]['blend']==23 and result[1]['blend']==26
bad33=[tail([],schema=5),tail([(0,23)],schema=5,count=5),tail([(4,23)],schema=5),
       tail([(0,23),(0,26)],schema=5),tail([(1,24),(0,25)],schema=5),
       tail([(2,24)],schema=5),tail([(3,26)],schema=5),tail([(0,23),(3,26)],schema=5),
       tail([(0,23),(1,21)],schema=5)]
bad33 += [tail([(0,23)],schema=schema) for schema in (1,2,3,4,6,0xffffffff)]
bad33 += [tail([(0,mode)],schema=5) for mode in (0,14,16,17,21,27,0xffffffff)]
bad33 += [tail(valid33,schema=5)[:end] for end in range(len(tail(valid33,schema=5)))]
for data in bad33:
    result=layers();before={k:dict(v) for k,v in result.items()}
    try:native.read_layer_blending(native.Reader(data),result,33)
    except (AssertionError,struct.error):pass
    else:raise AssertionError('Malformed HSL blend tail accepted')
    assert result==before,'Refused schema5 tail partially changed decoded properties'
for mode in (1,2,3,4,5,6,7,8,9,10,11,12,13,15,18,19,20,22,23,24,25,26):
    result=layers()
    assert native.read_layer_blending(native.Reader(tail([(0,mode)],schema=5)),result,33)==[(0,mode)]
frozen33=0
for version,schema in ((26,1),(27,2),(30,3),(31,4)):
    for mode in (23,24,25,26):
        result=layers();before={k:dict(v) for k,v in result.items()}
        try:native.read_layer_blending(native.Reader(tail([(0,mode)],schema=schema)),result,version)
        except AssertionError:pass
        else:raise AssertionError('An older schema acquired an HSL addition')
        assert result==before
        frozen33+=1
print(f'PASS: independent native33 schema5 all twenty-two modes, {len(bad33)} malformed/atomic-refusal cases, and {frozen33} frozen-old-schema refusals')

# Schema6 is exactly the four threshold modes; no historical schema widens.
valid34 = [(0,14),(1,21)]
reader=native.Reader(tail(valid34,schema=6));result=layers()
assert native.read_layer_blending(reader,result,34)==valid34
assert reader.at==len(reader.data) and result[0]['blend']==14 and result[1]['blend']==21
bad34=[tail([],schema=6),tail([(0,14)],schema=6,count=5),tail([(4,14)],schema=6),
       tail([(0,14),(0,21)],schema=6),tail([(1,16),(0,17)],schema=6),
       tail([(2,16)],schema=6),tail([(3,21)],schema=6),tail([(0,14),(3,21)],schema=6),
       tail([(0,14),(1,27)],schema=6)]
bad34 += [tail([(0,14)],schema=schema) for schema in (1,2,3,4,5,7,0xffffffff)]
bad34 += [tail([(0,mode)],schema=6) for mode in (0,27,0xffffffff)]
bad34 += [tail(valid34,schema=6)[:end] for end in range(len(tail(valid34,schema=6)))]
for data in bad34:
    result=layers();before={k:dict(v) for k,v in result.items()}
    try:native.read_layer_blending(native.Reader(data),result,34)
    except (AssertionError,struct.error):pass
    else:raise AssertionError('Malformed threshold blend tail accepted')
    assert result==before,'Refused schema6 tail partially changed decoded properties'
for mode in (1,2,3,4,5,6,7,8,9,10,11,12,13,15,18,19,20,22,23,24,25,26,14,16,17,21):
    result=layers()
    assert native.read_layer_blending(native.Reader(tail([(0,mode)],schema=6)),result,34)==[(0,mode)]
frozen34=0
for version,schema in ((26,1),(27,2),(30,3),(31,4),(33,5)):
    for mode in (14,16,17,21):
        result=layers();before={k:dict(v) for k,v in result.items()}
        try:native.read_layer_blending(native.Reader(tail([(0,mode)],schema=schema)),result,version)
        except AssertionError:pass
        else:raise AssertionError('An older schema acquired a threshold addition')
        assert result==before
        frozen34+=1
print(f'PASS: independent native34 schema6 all twenty-six modes, {len(bad34)} malformed/atomic-refusal cases, and {frozen34} frozen-old-schema refusals')

# Schema7 is exactly schema6's modes plus the kind-3 adjustment TARGET.
MODES26 = (1,2,3,4,5,6,7,8,9,10,11,12,13,15,18,19,20,22,23,24,25,26,14,16,17,21)
valid37 = [(0,1),(3,14)]
reader=native.Reader(tail(valid37,schema=7));result=layers()
assert native.read_layer_blending(reader,result,37)==valid37
assert reader.at==len(reader.data) and result[0]['blend']==1 and result[3]['blend']==14
bad37=[tail([],schema=7),tail([(0,1)],schema=7,count=5),tail([(4,1)],schema=7),
       tail([(0,1),(0,2)],schema=7),tail([(3,1),(0,2)],schema=7),
       tail([(2,1)],schema=7),tail([(0,1),(2,2)],schema=7),
       tail([(0,1),(3,27)],schema=7)]
bad37 += [tail([(0,1)],schema=schema) for schema in (1,2,3,4,5,6,8,0xffffffff)]
bad37 += [tail([(0,mode)],schema=7) for mode in (0,27,0xffffffff)]
bad37 += [tail(valid37,schema=7)[:end] for end in range(len(tail(valid37,schema=7)))]
for data in bad37:
    result=layers();before={k:dict(v) for k,v in result.items()}
    try:native.read_layer_blending(native.Reader(data),result,37)
    except (AssertionError,struct.error):pass
    else:raise AssertionError('Malformed adjustment blend tail accepted')
    assert result==before,'Refused schema7 tail partially changed decoded properties'
for mode in MODES26:
    for index in (0,1,3):
        result=layers()
        assert native.read_layer_blending(native.Reader(tail([(index,mode)],schema=7)),result,37)==[(index,mode)]
frozen37=0
for version,oldschema in ((26,1),(27,2),(30,3),(31,4),(33,5),(34,6)):
    for mode in (1,14):
        result=layers();before={k:dict(v) for k,v in result.items()}
        try:native.read_layer_blending(native.Reader(tail([(3,mode)],schema=oldschema)),result,version)
        except AssertionError:pass
        else:raise AssertionError('An older schema acquired the adjustment target')
        assert result==before
        frozen37+=1
print(f'PASS: independent native37 schema7 all twenty-six modes on kinds 0/1/3, {len(bad37)} malformed/atomic-refusal cases, and {frozen37} frozen-old-schema kind-3 refusals')

# Independently compose MOUT outside the documented blend table/body.
import tempfile
fixture_spec=importlib.util.spec_from_file_location("mask_fixtures",root/"scripts/mask-probe/fixtures.py")
mask_fixtures=importlib.util.module_from_spec(fixture_spec);fixture_spec.loader.exec_module(mask_fixtures)
expected=mask_fixtures.description(16)
original=mask_fixtures.native(expected,mask_fixtures.PROFILE.read_bytes())
assert original[-16:-12]==b'MOUT'
combined_cases=0
with tempfile.TemporaryDirectory(prefix="rz-blend-mask-decoder-") as scratch:
    path=Path(scratch)/"combined.rz"
    for mode,version,schema in [(1,26,1),(3,27,2),(10,30,3),(11,30,3),(12,30,3),(15,30,3),(13,31,4),(18,31,4),(19,31,4),(20,31,4),(22,31,4),(23,33,5),(24,33,5),(25,33,5),(26,33,5),(14,34,6),(16,34,6),(17,34,6),(21,34,6),(14,37,7)]:
        body=b'RZDC'+struct.pack('<II',29,version)+original[8:-16]+tail([(0,mode)],schema=schema)+original[-16:]
        path.write_bytes(body);value=native.read_native(path)
        assert value['version']==29 and value['document']['blend_version']==version
        assert value['document']['layers'][0]['blend']==mode
        mask=value['document']['layers'][0]['mask']
        assert mask['outside_coverage']==255 and mask['data']==mask_fixtures.mask_bytes(expected['mask'],16)
        for bad in [body[:-1], body+b'\0', body[:4]+struct.pack('<II',version,29)+body[12:],
                    body[:-16]+b'MOUT'+struct.pack('<III',2,1,0)]:
            path.write_bytes(bad)
            try: native.read_native(path)
            except (AssertionError,struct.error): pass
            else: raise AssertionError('Malformed combined wrappers accepted')
        combined_cases+=1
print(f"PASS: {combined_cases} independently authored mask29/blend26/27/30/31/33/34/37 compositions and{combined_cases*4} malformed wrapper cases")

# Qualify unwrapped31 framing too, and refuse each addition under old headers.
plain_expected=mask_fixtures.description(16,outside=0)
plain=mask_fixtures.native(plain_expected,mask_fixtures.PROFILE.read_bytes())
with tempfile.TemporaryDirectory(prefix="rz-five-blend-decoder-") as scratch:
    path=Path(scratch)/"plain.rz"
    unwrapped_refusals=0
    unwrapped_files=0
    for mode,current,schema in [(13,31,4),(18,31,4),(19,31,4),(20,31,4),(22,31,4),(23,33,5),(24,33,5),(25,33,5),(26,33,5),(14,34,6),(16,34,6),(17,34,6),(21,34,6),(14,37,7)]:
        body=b'RZDC'+struct.pack('<I',current)+plain[4:]+tail([(0,mode)],schema=schema)
        path.write_bytes(body);value=native.read_native(path)
        assert value['version']==current and value['document']['blend_version']==current
        assert value['document']['mask_version'] is None
        assert value['document']['layers'][0]['blend']==mode
        assert value['document']['layers'][0]['mask']['outside_coverage']==0
        unwrapped_files+=1
        for version,oldschema in ((26,1),(27,2),(30,3),(31,4),(33,5)):
            if version>=current: continue
            bad=b'RZDC'+struct.pack('<I',version)+plain[4:]+tail([(0,mode)],schema=oldschema)
            path.write_bytes(bad)
            try:native.read_native(path)
            except (AssertionError,struct.error):pass
            else:raise AssertionError('Old whole-file blend framing acquired a newer addition')
            unwrapped_refusals+=1
print(f'PASS: {unwrapped_files} independently authored unwrapped31/33/34/37 files and {unwrapped_refusals} old whole-file schema refusals')

# LCLP (clipping32) and LBIF (Blend If35) tails, independently framed.
def lclp(indexes, count=None):
    return b"LCLP" + struct.pack("<II", 1, len(indexes) if count is None else count) + b"".join(struct.pack("<I", i) for i in indexes)
def lbif(entries, schema=1, count=None):
    out = b"LBIF" + struct.pack("<II", schema, len(entries) if count is None else count)
    for index, channel, this, under in entries:
        out += struct.pack("<II", index, channel) + bytes(this) + bytes(under)
    return out
def typed_layers():
    return {0: dict(kind=0, blend=0, clipped=False, blend_if=None), 1: dict(kind=1, blend=0, clipped=False, blend_if=None),
            2: dict(kind=2, blend=27, clipped=False, blend_if=None), 3: dict(kind=3, blend=0, clipped=False, blend_if=None)}
reader = native.Reader(lclp([1, 3])); result = typed_layers()
assert native.read_clipping(reader, result) == [1, 3] and reader.at == len(reader.data)
assert result[1]['clipped'] and result[3]['clipped'] and not result[0]['clipped']
bad_lclp = [lclp([], count=0), lclp([0], count=5), lclp([4]), lclp([1, 1]), lclp([3, 1]), lclp([0])[:-1], b"LCLP" + struct.pack("<III", 2, 1, 0)]
for data in bad_lclp:
    result = typed_layers(); before = {k: dict(v) for k, v in result.items()}
    try: native.read_clipping(native.Reader(data), result)
    except (AssertionError, struct.error): pass
    else: raise AssertionError("Malformed clipping tail was accepted")
    assert result == before, "Refused clipping tail changed decoded properties"
print(f"PASS: independent native32 LCLP decoder valid table and {len(bad_lclp)} malformed/atomic-refusal cases")

FULL = [0, 0, 255, 255]
valid35 = [(0, 3, [10, 20, 240, 255], FULL), (1, 0, FULL, [64, 96, 255, 255])]
reader = native.Reader(lbif(valid35)); result = typed_layers()
assert native.read_blend_if(reader, result) == [(0, dict(channel='blue', this_layer=[10, 20, 240, 255], underlying=FULL)),
                                                (1, dict(channel='gray', this_layer=FULL, underlying=[64, 96, 255, 255]))]
assert reader.at == len(reader.data) and result[2]['blend_if'] is None
bad35 = [lbif([], count=0), lbif(valid35, count=5), lbif([(4, 0, [1, 1, 255, 255], FULL)]),
         lbif([(0, 0, [1, 1, 255, 255], FULL), (0, 0, [1, 1, 255, 255], FULL)]),
         lbif([(1, 0, [1, 1, 255, 255], FULL), (0, 0, [1, 1, 255, 255], FULL)]),
         lbif([(0, 4, [1, 1, 255, 255], FULL)]), lbif([(0, 0, [20, 10, 255, 255], FULL)]),
         lbif([(0, 0, FULL, [0, 0, 220, 200])]), lbif([(0, 0, FULL, FULL)]),
         lbif([(2, 0, [1, 1, 255, 255], FULL)]), lbif([(3, 0, [1, 1, 255, 255], FULL)]),
         lbif([(0, 0, [1, 1, 255, 255], FULL), (3, 0, [1, 1, 255, 255], FULL)]),
         lbif(valid35, schema=2), lbif(valid35, schema=0xffffffff)]
bad35 += [lbif(valid35)[:end] for end in range(len(lbif(valid35)))]
for data in bad35:
    result = typed_layers(); before = {k: dict(v) for k, v in result.items()}
    try: native.read_blend_if(native.Reader(data), result)
    except (AssertionError, struct.error): pass
    else: raise AssertionError("Malformed Blend If tail was accepted")
    assert result == before, "Refused Blend If tail partially changed decoded properties"
print(f"PASS: independent native35 LBIF decoder valid table and {len(bad35)} malformed/atomic-refusal cases")

# Whole-file compositions: 32 -> 29 -> 34 (LCLP after MOUT) and 35 -> 32 -> 29 -> 34 (LBIF after LCLP).
with tempfile.TemporaryDirectory(prefix="rz-clip-blendif-decoder-") as scratch:
    path = Path(scratch) / "composed.rz"
    clip_body = (b'RZDC' + struct.pack('<III', 32, 29, 34) + original[8:-16] + tail([(0, 14)], schema=6)
                 + original[-16:] + lclp([0]))
    path.write_bytes(clip_body); value = native.read_native(path)
    assert value['version'] == 32 and value['document']['clipping_version'] == 32
    assert value['document']['mask_version'] == 29 and value['document']['blend_version'] == 34
    assert value['document']['layers'][0]['clipped'] and value['document']['layers'][0]['blend'] == 14
    assert value['document']['layers'][0]['mask']['outside_coverage'] == 255
    assert value['document']['layers'][0]['blend_if'] is None
    entry = [(0, 1, [10, 20, 240, 255], [0, 3, 250, 255])]
    both_body = (b'RZDC' + struct.pack('<IIII', 35, 32, 29, 34) + original[8:-16] + tail([(0, 14)], schema=6)
                 + original[-16:] + lclp([0]) + lbif(entry))
    path.write_bytes(both_body); value = native.read_native(path)
    assert value['version'] == 35 and value['document']['blend_if_version'] == 35
    assert value['document']['clipping_version'] == 32 and value['document']['mask_version'] == 29
    assert value['document']['layers'][0]['clipped'] and value['document']['layers'][0]['blend'] == 14
    assert value['document']['layers'][0]['blend_if'] == dict(channel='red', this_layer=[10, 20, 240, 255], underlying=[0, 3, 250, 255])
    malformed = [
        clip_body[:-1], clip_body + b'\0',
        clip_body[:4] + struct.pack('<III', 29, 32, 34) + clip_body[16:],
        clip_body + lbif(entry),                                  # LBIF under a 32 header
        both_body[:-1], both_body + b'\0',
        both_body[:4] + struct.pack('<IIII', 32, 35, 29, 34) + both_body[20:],
        both_body[:4] + struct.pack('<IIII', 35, 29, 32, 34) + both_body[20:],
        both_body[:-28] + lbif([(0, 1, FULL, FULL)]),            # identity entry
        both_body[:-28] + lbif([], count=0),                       # empty table
        both_body[:-28] + lbif([(0, 4, [10, 20, 240, 255], FULL)]),
        both_body[:-44] + lbif(entry),                             # LBIF replacing LCLP
        b'RZDC' + struct.pack('<III', 35, 29, 34) + original[8:-16] + tail([(0, 14)], schema=6) + original[-16:] + lclp([0]) + lbif(entry),
    ]
    for bad in malformed:
        path.write_bytes(bad)
        try: native.read_native(path)
        except (AssertionError, struct.error): pass
        else: raise AssertionError('Malformed clipping/Blend If wrapper accepted')
print(f"PASS: independently authored 32 -> 29 -> 34 and 35 -> 32 -> 29 -> 34 whole files and {len(malformed)} malformed wrapper cases")

# LSTY (layer styles36): canonical style bodies on raster/isolated-group records.
import json as _json
def canonical(style):
    return _json.dumps(style, sort_keys=True, separators=(',', ':')).encode()
def lsty(entries, schema=1, count=None):
    out = b"LSTY" + struct.pack("<II", schema, len(entries) if count is None else count)
    for index, body in entries:
        out += struct.pack("<II", index, len(body)) + body
    return out
def style_of(effects, fill=1.0):
    return dict(version=1, fill_opacity=fill, blend_if=None, effects=effects)
SHADOW = dict(type='drop_shadow', enabled=True, blend='multiply', color='#000000', opacity=0.75, angle=120.0,
              use_global_light=True, distance=5.0, spread=0.0, size=5.0, layer_knocks_out=True)
OVERLAY = dict(type='color_overlay', enabled=True, blend='normal', color='#ff0000', opacity=1.0)
def typed_style_layers():
    return {k: dict(v, style=None) for k, v in typed_layers().items()}
valid36 = [(0, canonical(style_of([SHADOW]))), (1, canonical(style_of([], 0.5)))]
reader = native.Reader(lsty(valid36)); result = typed_style_layers()
records = native.read_layer_style(reader, result)
assert [i for i, _ in records] == [0, 1] and reader.at == len(reader.data)
assert result[0]['style']['effects'][0]['type'] == 'drop_shadow' and result[1]['style']['fill_opacity'] == 0.5
assert result[2]['style'] is None
bad36 = [lsty([], count=0), lsty(valid36, count=5), lsty([(4, canonical(style_of([SHADOW])))]),
         lsty([(0, canonical(style_of([SHADOW]))), (0, canonical(style_of([OVERLAY])))]),
         lsty([(1, canonical(style_of([SHADOW]))), (0, canonical(style_of([OVERLAY])))]),
         lsty([(2, canonical(style_of([SHADOW])))]), lsty([(3, canonical(style_of([SHADOW])))]),
         lsty([(0, canonical(style_of([SHADOW]))), (3, canonical(style_of([OVERLAY])))]),
         lsty(valid36, schema=2), lsty(valid36, schema=0xffffffff),
         lsty([(0, b'')]), lsty([(0, b'x' * 65537)]),
         lsty([(0, canonical(style_of([])))]),                                   # identity
         lsty([(0, canonical(style_of([dict(SHADOW, enabled=False)])))]),         # identity, disabled effect
         lsty([(0, canonical(dict(style_of([SHADOW]), blend_if=dict(channel='red'))))]),
         lsty([(0, canonical(dict(style_of([SHADOW]), extra=1)))]),
         lsty([(0, canonical(style_of([dict(SHADOW, type='drop_shadox')])))]),
         lsty([(0, canonical(style_of([SHADOW, SHADOW])))]),
         lsty([(0, _json.dumps(style_of([SHADOW])).encode())]),                   # non-canonical spacing
         lsty([(0, b'{"effects":[}')]), lsty([(0, canonical(style_of([SHADOW], 2.0)))])]
bad36 += [lsty(valid36)[:end] for end in range(len(lsty(valid36)))]
for data in bad36:
    result = typed_style_layers(); before = {k: dict(v) for k, v in result.items()}
    try: native.read_layer_style(native.Reader(data), result)
    except (AssertionError, struct.error, ValueError, KeyError, AttributeError, UnicodeDecodeError): pass
    else: raise AssertionError("Malformed layer style tail was accepted")
    assert result == before, "Refused layer style tail partially changed decoded properties"
print(f"PASS: independent native36 LSTY decoder valid table and {len(bad36)} malformed/atomic-refusal cases")

# Whole-file composition: 36 -> 35 -> 32 -> 29 -> 34 (LSTY after LBIF).
with tempfile.TemporaryDirectory(prefix="rz-style-decoder-") as scratch:
    path = Path(scratch) / "styled.rz"
    style_entry = [(0, canonical(style_of([SHADOW], 0.5)))]
    styled_body = (b'RZDC' + struct.pack('<IIIII', 36, 35, 32, 29, 34) + original[8:-16] + tail([(0, 14)], schema=6)
                   + original[-16:] + lclp([0]) + lbif(entry) + lsty(style_entry))
    path.write_bytes(styled_body); value = native.read_native(path)
    assert value['version'] == 36 and value['document']['style_version'] == 36
    assert value['document']['blend_if_version'] == 35 and value['document']['clipping_version'] == 32
    assert value['document']['mask_version'] == 29 and value['document']['blend_version'] == 34
    layer0 = value['document']['layers'][0]
    assert layer0['clipped'] and layer0['blend'] == 14 and layer0['mask']['outside_coverage'] == 255
    assert layer0['blend_if'] == dict(channel='red', this_layer=[10, 20, 240, 255], underlying=[0, 3, 250, 255])
    assert layer0['style']['fill_opacity'] == 0.5 and layer0['style']['effects'][0]['type'] == 'drop_shadow'
    # A 36 header straight over a mask29 body (no Blend If, no clipping).
    effects_only = b'RZDC' + struct.pack('<I', 36) + original[4:] + lsty(style_entry)
    path.write_bytes(effects_only); value = native.read_native(path)
    assert value['version'] == 36 and value['document']['style_version'] == 36 and value['document']['blend_if_version'] is None
    assert value['document']['layers'][0]['style']['fill_opacity'] == 0.5
    malformed36 = [
        styled_body[:-1], styled_body + b'\0',
        styled_body[:4] + struct.pack('<IIIII', 35, 36, 32, 29, 34) + styled_body[24:],
        styled_body[:4] + struct.pack('<IIIII', 36, 32, 35, 29, 34) + styled_body[24:],
        both_body + lsty(style_entry),                                                 # LSTY under a 35 header
        styled_body[:-len(lsty(style_entry))] + lsty([(0, canonical(style_of([])))]),   # identity
        styled_body[:-len(lsty(style_entry))] + lsty([], count=0),
        styled_body[:-len(lsty(style_entry))] + lsty([(0, canonical(dict(style_of([SHADOW]), blend_if=dict(channel='red', this_layer=[0, 0, 100, 100], underlying=[0, 0, 255, 255]))))]),
        styled_body[:-len(lsty(style_entry)) - 28] + lsty(style_entry),                # LSTY replacing LBIF
        b'RZDC' + struct.pack('<IIII', 36, 32, 29, 34) + original[8:-16] + tail([(0, 14)], schema=6) + original[-16:] + lclp([0]) + lbif(entry) + lsty(style_entry),
    ]
    for bad in malformed36:
        path.write_bytes(bad)
        try: native.read_native(path)
        except (AssertionError, struct.error, ValueError, KeyError): pass
        else: raise AssertionError('Malformed layer style wrapper accepted')
print(f"PASS: independently authored 36 -> 35 -> 32 -> 29 -> 34 and 36 -> 29 -> 34 whole files and {len(malformed36)} malformed wrapper cases")
