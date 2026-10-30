#!/usr/bin/env python3
"""Qualify five native layer modes through an owned production Release app.

Independent f64 Subtract, Vivid Light, Linear Light, Pin Light and Divide
oracles retain the established encoded-RGB semantics. Only Vivid Light's
specified division neighborhoods use the separately measured 1.6-code bound;
all other RGB and alpha retain .65 codes including final quantization.
Hard Mix has since been admitted by the threshold slice. Prior verifier fixtures and tolerances are unchanged.
"""
import argparse
from fractions import Fraction
import importlib.util
import json
import math
from pathlib import Path
import sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("prior_separable_helpers", ROOT / "scripts/verify_phase10d_separable.py")
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)
# Reuse unchanged fixture framing, transport, history and exact native reader.
support, digest, samples, png, history = prior.support, prior.digest, prior.samples, prior.png, prior.history
MODES = {"Subtract":13, "Vivid Light":18, "Linear Light":19, "Pin Light":20, "Divide":22}
OLD_MODES = {"Normal":0, "Multiply":1, "Overlay":3, "Color Dodge":10}
MODE_IDS = OLD_MODES | MODES
VERSIONS = {"Multiply":26, "Overlay":27, "Color Dodge":30} | {name:31 for name in MODES}
BACKDROP = [(0,65535,0,65535), (212,231,340,65535), (212,212,212,65535),
            (65323,65195,1,65535), (16384,32768,49152,65535),
            (8192,49152,23567,32771), (50000,23000,4000,1),
            (65001,55003,45005,0), (65535,65535,65535,0),
            (30000,50000,7000,7), (12345,23456,34567,65535), (65169,12345,3,65535)]
FOREGROUND = [(0,0,65535,65535), (65428,65419,65365,65535), (65427,65428,65429,65535),
              (107,170,0,65535), (32767,32768,32769,65535),
              (57000,10001,30000,45000), (12000,43000,61000,32769),
              (15001,25003,35005,32771), (1003,2005,3007,0),
              (62000,36000,11000,1), (54321,43210,32109,0), (65331,12345,2,65535)]


def blend(mode, b, s):
    if mode == "Subtract":
        return max(0,b-s)
    if mode == "Vivid Light":
        if s <= .5:
            return 1 if b == 1 else 0 if s == 0 else 1-min(1,(1-b)/(2*s))
        return 0 if b == 0 else 1 if s == 1 else min(1,b/(2*(1-s)))
    if mode == "Linear Light":
        return min(1,max(0,b+2*s-1))
    if mode == "Pin Light":
        return min(b,2*s) if s <= .5 else max(b,2*s-1)
    if mode == "Divide":
        return 1 if s == 0 else min(1,b/s)
    raise AssertionError(mode)


def expected(mode, opacity):
    output = []
    for back, front in zip(BACKDROP,FOREGROUND,strict=True):
        ba,sa = back[3]/65535,front[3]/65535*opacity
        alpha = sa+ba*(1-sa)
        if alpha == 0:
            output.extend((0,0,0,0)); continue
        for bc,sc in zip(back[:3],front[:3],strict=True):
            b,s = bc/65535,sc/65535
            output.append(65535*(sa*((1-ba)*s+ba*blend(mode,b,s))+(1-sa)*ba*b)/alpha)
        output.append(65535*alpha)
    return tuple(output)


def reference_bound(mode, component):
    channel = component % 4
    if channel == 3 or mode != "Vivid Light":
        return .65
    b = BACKDROP[component//4][channel]
    s = FOREGROUND[component//4][channel]
    # Classify from exact authored codes, including saturation equality. The
    # upper branch inherits subtraction cancellation in Dodge; the lower one
    # inherits Burn's near-white numerator. These are finite qualification
    # bounds, not a universal f32 error proof for arbitrary nested documents.
    sensitive = ((s >= 64880 and 0 < b <= 2*(65535-s))
                 or (s <= 655 and b < 65535 and 65535-b <= 2*s))
    return 1.6 if sensitive else .65


def reference_contract():
    """Pin source-code witnesses against exact rational expressions, no product output."""
    assert MODES == {"Subtract":13,"Vivid Light":18,"Linear Light":19,"Pin Light":20,"Divide":22}
    assert blend("Divide",0,0) == 1 and blend("Divide",1,0) == 1
    assert blend("Vivid Light",1,0) == 1 and blend("Vivid Light",0,1) == 0
    for value in (0,1/65535,.25,.5,.75,65534/65535,1):
        for mode in ("Vivid Light","Linear Light","Pin Light"):
            assert abs(blend(mode,value,.5)-value) < 1e-14
    witnesses = [
        ("Vivid Light",212,65427,Fraction(65535*106,108)),
        ("Vivid Light",212,65428,Fraction(65535*106,107)),
        ("Vivid Light",212,65429,Fraction(65535)),
        ("Vivid Light",231,65419,Fraction(65535*231,232)),
        ("Vivid Light",340,65365,Fraction(65535)),
        ("Divide",65169,65331,Fraction(65535*65169,65331)),
    ]
    result = []
    for mode,b,s,rational in witnesses:
        assert abs(65535*blend(mode,b/65535,s/65535)-float(rational)) < 1e-7
        assert any((back[c],front[c],back[3],front[3]) == (b,s,65535,65535)
                   for back,front in zip(BACKDROP,FOREGROUND,strict=True) for c in range(3))
        result.append(dict(mode=mode,backdrop_code=b,source_code=s,exact_reference=str(rational)))
    return result


def rendered(call, path, mode, opacity):
    call("save_copy",path=str(path))
    value=support.png_samples(path.read_bytes())
    assert (value["width"],value["height"],value["depth"]) == (4,3,16)
    actual=value["samples"];reference=expected(mode,opacity)
    errors=[abs(a-b) for a,b in zip(actual,reference,strict=True)]
    for i,error in enumerate(errors):
        assert error <= reference_bound(mode,i), (mode,opacity,i,error,reference_bound(mode,i),actual[i],reference[i])
    if mode == "Vivid Light" and opacity == 1:
        assert errors[4] > 1, "The admitted f32 division-neighbor witness must be exercised"
    return {"maximum_error_from_unrounded_f64_including_final_quantization":max(errors),
            "maximum_difference_from_rounded_f64":max(abs(a-math.floor(b+.5)) for a,b in zip(actual,reference,strict=True)),
            "component_errors":errors}


def native(call, path, mode, opacity, base_name, baseline, blend_name=None):
    call("save_copy", path=str(path))
    result = support.read_native(path)
    doc = result["document"]
    blend_name = mode if blend_name is None else blend_name
    assert result["version"] == VERSIONS.get(blend_name,baseline["version"])
    assert doc["blend_version"] == VERSIONS.get(blend_name) and doc["mask_version"] is None
    assert doc.get("layer_blend_records",[]) == ([] if blend_name == "Normal" else [(1,MODE_IDS[blend_name])])
    assert (doc["width"], doc["height"], doc["depth"]) == (4,3,16)
    assert doc["profile"] == baseline["document"]["profile"]
    assert len(doc["layers"]) == len(doc["fallbacks"]) == 2
    for index, expected_pixels in enumerate((BACKDROP, FOREGROUND)):
        row = doc["layers"][index]
        value = support.png_samples(doc["fallbacks"][index])
        assert (value["width"], value["height"], value["depth"]) == (4, 3, 16)
        assert tuple(value["samples"]) == samples(expected_pixels)
        assert row["name"] == (base_name if index == 0 else mode + " native")
        assert (row["width"], row["height"]) == (4, 3)
        assert row["blend"] == (0 if index == 0 else MODE_IDS[blend_name])
        assert row["opacity"] == (1 if index == 0 else opacity)
        assert row["visible"] and row["mask"] is None and (row["x"], row["y"]) == (0, 0)
    # A placed PNG is an ordinary raster layer: the file keeps no master of it.
    assert not result["assets"] and not result["bindings"]
    for key in ("assets", "bindings", "resources"):
        assert result[key] == baseline[key], key
    return digest(path)


def exercise(app, call, mode, opacity, folder):
    folder.mkdir()
    before = history(call)
    base_name = call("get_document")["layers"][0]["name"]
    baseline_path = folder / "baseline.rz"
    call("save_copy", path=str(baseline_path))
    baseline = support.read_native(baseline_path)
    baseline_sha = digest(baseline_path)

    def undo(label):
        call("undo")
        assert history(call)[0] == before[0]
        saved = folder / (label + ".rz")
        call("save_copy", path=str(saved))
        assert digest(saved) == baseline_sha

    # Author reference states for the same name/opacity before the five-mode
    # edit. Downgrades must restore their complete bytes, not merely a header.
    call("set_layer_properties",index=1,opacity=opacity,name=mode+" native")
    prepared = history(call)
    older = {"Normal":native(call,folder/"old-Normal.rz",mode,opacity,base_name,baseline,"Normal")}
    for old in ("Multiply","Overlay","Color Dodge"):
        call("set_layer_properties",index=1,blend_mode=old)
        older[old] = native(call,folder/("old-"+old+".rz"),mode,opacity,base_name,baseline,old)
        call("undo")
        assert history(call)[0] == prepared[0]
    undo("before-recording")

    app.call("start_recording")
    call("set_layer_properties", index=1, blend_mode=mode, opacity=opacity, name=mode + " native")
    action = app.call("stop_recording", name="Native five " + mode + " " + str(opacity))
    assert [step["tool"] for step in action["steps"]] == ["set_layer_properties"]
    assert action["steps"][0]["arguments"]["blend_mode"] == mode
    assert action["steps"][0]["arguments"]["opacity"] == opacity
    assert history(call)[1] == before[1]+1
    error = rendered(call, folder / "output.png", mode, opacity)
    original = folder / "output.rz"
    sha = native(call, original, mode, opacity, base_name, baseline)
    for old in ("Color Dodge","Overlay","Multiply","Normal"):
        call("set_layer_properties",index=1,blend_mode=old)
        assert native(call,folder/("down-"+old+".rz"),mode,opacity,base_name,baseline,old) == older[old]
        call("undo")
        assert native(call,folder/("restored-"+old+".rz"),mode,opacity,base_name,baseline) == sha
    undo("undone")
    call("redo")
    assert native(call, folder / "redo.rz", mode, opacity, base_name, baseline) == sha
    undo("before-replay")
    assert call("run_action", name="Native five " + mode + " " + str(opacity))["ok"]
    assert history(call)[1] == before[1]+1
    assert native(call, folder / "action.rz", mode, opacity, base_name, baseline) == sha
    identifier = app.call("open_document", path=str(original), precision="preserve")["document"]["id"]
    reopened = support.call_for(app, identifier, folder / "reopen-calls.jsonl")
    assert native(reopened, folder / "reopened.rz", mode, opacity, base_name, baseline) == sha
    assert rendered(reopened, folder / "reopened.png", mode, opacity) == error
    undo("action-undone")
    return {"numerical_error": error, "opacity": opacity, "native_sha256": sha, "exact_sources_and_plain_raster_layer": True,
            "one_undo_redo_and_action_replay": True, "native_reopen_exact": True,
            "exact_downgrade_versions":[30,27,26,baseline["version"]]}


def atomic_refusals(call, output):
    baseline = history(call)
    selection = call("get_selection")
    dirty = call("get_document")["unsaved_changes"]
    path = output/"before-refusals.rz"
    call("save_copy",path=str(path))
    before = digest(path)
    results = []
    cases = [
        ({"blend_mode":"Pass Through","name":"Must not publish"},"Pass Through requires a group"),
        ({"blend_mode":"Vivid Light","name":"x"*65537},"64 KiB"),
        ({"blend_mode":"Divide","opacity":"invalid"},"Invalid native layer property: opacity"),
    ]
    for index,(arguments,fragment) in enumerate(cases):
        error = call("set_layer_properties",index=1,fail=True,**arguments)
        assert fragment.lower() in error.lower(),(fragment,error)
        assert history(call) == baseline and call("get_selection") == selection
        assert call("get_document")["unsaved_changes"] == dirty
        path = output/f"refusal-{index}.rz"
        call("save_copy",path=str(path))
        assert digest(path) == before
        results.append(dict(refusal=error,bytes_history_selection_dirty_unchanged=True))
    return results


def masked_expected(mode, expected, opacity=.625):
    output=[];mask=expected["mask"];width,height=expected["canvas"]
    for y in range(height):
        for x in range(width):
            mx,my=x-mask["x"],y-mask["y"]
            code=mask["samples"][my*mask["width"]+mx] if 0<=mx<mask["width"] and 0<=my<mask["height"] else 65535
            coverage=1-mask["density"]+mask["density"]*code/65535
            rgba=expected["rgba"][(y*width+x)*4:][:4]
            ba=rgba[3]/65535*coverage;sa=ba*opacity;alpha=sa+ba*(1-sa)
            if alpha==0:
                output.extend((0,0,0,0));continue
            for value in rgba[:3]:
                color=value/65535
                output.append(65535*(sa*((1-ba)*color+ba*blend(mode,color,color))+(1-sa)*ba*color)/alpha)
            output.append(65535*alpha)
    return output


def masked_workflows(app, output):
    spec=importlib.util.spec_from_file_location("composed_mask_fixtures",ROOT/"scripts/mask-probe/fixtures.py")
    fixtures=importlib.util.module_from_spec(spec);spec.loader.exec_module(fixtures)
    cases=fixtures.write(output/"mask-fixtures")
    source,expected=cases["16-outside-white"]
    fixture_sha=digest(source)
    identifier=app.call("open_document",path=str(source),precision="preserve")["document"]["id"]
    call=support.call_for(app,identifier,output/"masked-calls.jsonl")
    call("duplicate_layer",index=0)
    call("set_active_layer",index=1)
    call("set_layer_properties",index=1,opacity=.625,name="Masked native")
    baseline=history(call)
    mask_bytes=fixtures.mask_bytes(expected["mask"],16)
    names=[row["name"] for row in call("get_document")["layers"]]

    def stored(path,mode=None):
        call("save_copy",path=str(path));parsed=support.read_native(path);doc=parsed["document"]
        assert parsed["version"]==29 and doc["mask_version"]==29
        assert doc["blend_version"]==VERSIONS.get(mode)
        assert len(doc["layers"])==len(doc["fallbacks"])==2
        assert doc["profile"]==fixtures.PROFILE.read_bytes()
        assert not any(parsed[key] for key in ("assets","bindings","resources","artwork"))
        assert doc["mask_outside_records"]==[0,1]
        assert doc.get("layer_blend_records",[])==([] if mode is None else [(1,MODE_IDS[mode])])
        for index in range(2):
            row=doc["layers"][index];mask=row["mask"]
            assert row["name"]==names[index] and row["opacity"]==(1 if index==0 else .625)
            assert row["blend"]==(0 if index==0 or mode is None else MODE_IDS[mode])
            assert row["visible"] and (row["x"],row["y"])==(0,0)
            assert list(support.png_samples(doc["fallbacks"][index])["samples"])==expected["rgba"]
            assert mask["data"]==mask_bytes
            for key in ("x","y","width","height","density","feather","linked","enabled","outside_coverage"):
                assert mask[key]==expected["mask"][key],(key,mask,expected)
        return digest(path)

    baseline_sha=stored(output/"masked-baseline.rz")
    older={}
    for old in ("Multiply","Overlay","Color Dodge"):
        call("set_layer_properties",index=1,blend_mode=old)
        older[old]=stored(output/("masked-old-"+old+".rz"),old)
        call("undo");assert history(call)[0]==baseline[0]
    result={}
    for mode in MODES:
        label=mode.lower().replace(" ","-")
        app.call("start_recording")
        call("set_layer_properties",index=1,blend_mode=mode)
        action_name="Masked five "+mode
        action=app.call("stop_recording",name=action_name)
        assert [step["tool"] for step in action["steps"]]==["set_layer_properties"]
        assert action["steps"][0]["arguments"]["blend_mode"]==mode
        assert history(call)[1]==baseline[1]+1
        path=output/("masked-"+label+".rz");sha=stored(path,mode)
        png_path=output/("masked-"+label+".png");call("save_copy",path=str(png_path))
        actual=support.png_samples(png_path.read_bytes());reference=masked_expected(mode,expected)
        assert (actual["width"],actual["height"],actual["depth"])==(4,3,16)
        error=max(abs(a-b) for a,b in zip(actual["samples"],reference,strict=True))
        assert error<=.65,(mode,error,actual["samples"],reference)
        for old in ("Multiply","Overlay","Color Dodge"):
            call("set_layer_properties",index=1,blend_mode=old)
            assert stored(output/("masked-"+label+"-to-"+old+".rz"),old)==older[old]
            call("undo");assert stored(output/("masked-"+label+"-restored.rz"),mode)==sha
        call("undo");assert history(call)[0]==baseline[0]
        assert stored(output/("masked-"+label+"-undone.rz"))==baseline_sha
        call("redo");assert stored(output/("masked-"+label+"-redone.rz"),mode)==sha
        call("undo");assert call("run_action",name=action_name)["ok"]
        assert history(call)[1]==baseline[1]+1 and stored(output/("masked-"+label+"-replayed.rz"),mode)==sha
        again=app.call("open_document",path=str(path),precision="preserve")["document"]["id"]
        reopened=support.call_for(app,again,output/("masked-"+label+"-reopen-calls.jsonl"))
        reopened_path=output/("masked-"+label+"-reopened.rz");reopened("save_copy",path=str(reopened_path))
        assert digest(reopened_path)==sha
        call("undo");assert history(call)[0]==baseline[0]
        result[mode]={"native_sha256":sha,"maximum_error_from_unrounded_f64_including_quantization":error,
                      "exact_raw_pixels_masks_and_profile":True,"outside255_preserved":True,
                      "mask29_blend31_roundtrip":True,"exact_nested26_27_30_downgrade":True,"one_undo_redo_and_action":True}
    assert digest(source)==fixture_sha
    return {"fixture_sha256":fixture_sha,"cases":result}


def dependencies():
    """The code files `dependency_sha256` hashes: the script's whole load chain."""
    return [Path(__file__), Path(prior.__file__),
            ROOT / "scripts/host-source-probe/support.py", ROOT / "scripts/live-filter-probe/native.py",
            ROOT / "scripts/mask-probe/fixtures.py", ROOT / "scripts/phase5_support.py",
            ROOT / "scripts/precision-probe/fixtures.py",
            ROOT / "scripts/precision-probe/retained_fixtures.py",
            ROOT / "scripts/precision-probe/retained_linked_fixtures.py", ROOT / "scripts/raw-probe/native.py",
            ROOT / "scripts/retouch-probe/fixtures.py", ROOT / "scripts/retouch-probe/support.py",
            ROOT / "scripts/verification_isolation.py",
            ROOT / "scripts/app_under_test.py", ROOT / "scripts/verify_phase1.py",
            ROOT / "scripts/verify_phase2.py", ROOT / "scripts/verify_phase8b.py",
            *sorted((ROOT / "core/src").rglob("*.rs"))]


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app",type=Path,required=True)
    parser.add_argument("--output",type=Path,required=True)
    args=parser.parse_args();output=args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error("output must be new and outside the repository")
    output.mkdir(exist_ok=False)
    back,front=output/"backdrop16.png",output/"foreground16.png"
    png(back,BACKDROP);png(front,FOREGROUND)
    executable=(args.app/"Contents/MacOS/Doctored" if args.app.suffix==".app" else args.app).resolve()
    report={"app":str(executable),"app_sha256":digest(executable),"script_sha256":digest(Path(__file__)),
            "shared_helper_sha256":digest(Path(prior.__file__)),
            "dependency_sha256":{str(p):digest(p) for p in dependencies()},
            "fixture_sha256":{p.name:digest(p) for p in (back,front)},"modes":{},
            "exact_rational_witnesses":reference_contract(),
            "numerical_policy":"At most0.65 native codes from unrounded f64 including quantization; only exact-code-classified Vivid Light division neighborhoods allow1.6. Finite fixture qualification, not a universal nested-document bound."}
    with support.HostApp(output/"app",output/"recovery",executable) as app:
        identifier=app.call("open_document",path=str(back),precision="preserve")["document"]["id"]
        call=support.call_for(app,identifier,output/"calls.jsonl")
        assert call("get_precision")["mode"]=="rgba16"
        placed = call("place_image",path=str(front),storage="embedded")
        assert placed["layer"]==1 and placed["source"] is None, placed
        call("set_active_layer",index=1)
        report["atomic_refusals"]=atomic_refusals(call,output)
        for mode in MODES:
            report["modes"][mode]=[]
            for opacity in (1,.625):
                folder=output/(mode.lower().replace(" ","-")+"-"+str(opacity))
                report["modes"][mode].append(exercise(app,call,mode,opacity,folder))
        report["outside_mask_composition"]=masked_workflows(app,output)
    assert all(digest(p)==report["fixture_sha256"][p.name] for p in (back,front))
    assert set(report["modes"]) == set(MODES)
    assert all([case["opacity"] for case in report["modes"][mode]] == [1,.625] for mode in MODES)
    assert set(report["outside_mask_composition"]["cases"]) == set(MODES)
    report["fixtures_unchanged"]=True
    report["checks_passed"]=["five independent f64 equations with midpoint/endpoints and exact division-neighbor witnesses",
        "full and fractional opacity with partial/tiny source and backdrop alpha",
        "separate measured division-sensitive bounds and unchanged ordinary/sum bounds",
        "exact raw RGB, alpha and hidden samples", "conditional native31/schema4 roundtrip and exact older-version downgrades",
        "atomic property edit, exact Undo, Redo and Actions replay",
        "independent outside255 masks composed with all five modes in mask29/blend31 wrappers",
        "exact raw mask/sample persistence, nested26/27/30 downgrades, Actions and native reopen"]
    (output/"result.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({"output":str(output),"modes":report["modes"],"checks_passed":report["checks_passed"]},indent=2))


if __name__ == "__main__":
    main()
