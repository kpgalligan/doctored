#!/usr/bin/env python3
"""Qualify four native layer modes through an owned production Release app.

Uses independent f64 Color Dodge/Burn endpoint-priority and clamped-sum
oracles. Near singular division has its own measured f32 qualification bound;
no prior seven-mode test or tolerance is changed. See the phase status document.
"""
import argparse
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
MODES = {"Color Dodge":10, "Color Burn":11, "Linear Dodge (Add)":12, "Linear Burn":15}
BACKDROP = [(0,65535,0,65535), (106,65429,107,65535), (1,65534,2,65535),
            (32767,32768,32769,65535), (32767,32767,32768,65535),
            (8192,49152,23567,32771), (50000,23000,4000,1),
            (65001,55003,45005,0), (65535,65535,65535,0),
            (30000,50000,7000,7), (12345,23456,34567,65535), (170,65428,65535,65535)]
FOREGROUND = [(65535,0,65535,65535), (65428,107,65428,65535), (65533,2,65534,65535),
              (32768,32767,32766,65535), (32767,32768,32768,65535),
              (57000,10001,30000,45000), (12000,43000,61000,32769),
              (15001,25003,35005,32771), (1003,2005,3007,0),
              (62000,36000,11000,1), (54321,43210,32109,65535), (65365,107,65535,65535)]


def blend(mode, b, s):
    if mode == "Color Dodge":
        return 0 if b == 0 else 1 if s == 1 else min(1,b/(1-s))
    if mode == "Color Burn":
        return 1 if b == 1 else 0 if s == 0 else 1-min(1,(1-b)/s)
    if mode == "Linear Dodge (Add)":
        return min(1,b+s)
    if mode == "Linear Burn":
        return max(0,b+s-1)
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
    if channel == 3:
        return .65
    b = BACKDROP[component//4][channel]
    s = FOREGROUND[component//4][channel]
    # Cancellation in 1-Cs/1-Cb amplifies existing f32 division rounding only
    # where the divide contributes, including its exact saturation boundary.
    # Endpoint precedence and ordinary/sum components keep the .65 bound.
    # Integer codes avoid misclassifying the boundary through double rounding.
    sensitive = ((mode == "Color Dodge" and s >= 64225 and 0 < b <= 65535-s)
                 or (mode == "Color Burn" and s <= 1310 and 0 < 65535-b <= s))
    return 1.6 if sensitive else .65


def rendered(call, path, mode, opacity):
    call("save_copy",path=str(path))
    value=support.png_samples(path.read_bytes())
    assert (value["width"],value["height"],value["depth"]) == (4,3,16)
    actual=value["samples"];reference=expected(mode,opacity)
    errors=[abs(a-b) for a,b in zip(actual,reference,strict=True)]
    for i,error in enumerate(errors):
        assert error <= reference_bound(mode,i), (mode,opacity,i,error,reference_bound(mode,i),actual[i],reference[i])
    if mode == "Color Dodge" and opacity == 1:
        assert errors[4] > 1, "The admitted f32 division-neighbor witness must be exercised"
    return {"maximum_error_from_unrounded_f64_including_final_quantization":max(errors),
            "maximum_difference_from_rounded_f64":max(abs(a-math.floor(b+.5)) for a,b in zip(actual,reference,strict=True)),
            "component_errors":errors}


def native(call, path, mode, opacity, base_name, baseline):
    call("save_copy", path=str(path))
    result = support.read_native(path)
    doc = result["document"]
    assert result["version"] == 30 and (doc["width"], doc["height"], doc["depth"]) == (4, 3, 16)
    assert doc["profile"] == baseline["document"]["profile"]
    assert len(doc["layers"]) == len(doc["fallbacks"]) == 2
    for index, expected_pixels in enumerate((BACKDROP, FOREGROUND)):
        row = doc["layers"][index]
        value = support.png_samples(doc["fallbacks"][index])
        assert (value["width"], value["height"], value["depth"]) == (4, 3, 16)
        assert tuple(value["samples"]) == samples(expected_pixels)
        assert row["name"] == (base_name if index == 0 else mode + " native")
        assert (row["width"], row["height"]) == (4, 3)
        assert row["blend"] == (0 if index == 0 else MODES[mode])
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

    app.call("start_recording")
    call("set_layer_properties", index=1, blend_mode=mode, opacity=opacity, name=mode + " native")
    action = app.call("stop_recording", name="Native four " + mode + " " + str(opacity))
    assert [step["tool"] for step in action["steps"]] == ["set_layer_properties"]
    assert action["steps"][0]["arguments"]["blend_mode"] == mode
    assert history(call)[1] == before[1]+1
    error = rendered(call, folder / "output.png", mode, opacity)
    original = folder / "output.rz"
    sha = native(call, original, mode, opacity, base_name, baseline)
    undo("undone")
    call("redo")
    assert native(call, folder / "redo.rz", mode, opacity, base_name, baseline) == sha
    undo("before-replay")
    assert call("run_action", name="Native four " + mode + " " + str(opacity))["ok"]
    assert history(call)[1] == before[1]+1
    assert native(call, folder / "action.rz", mode, opacity, base_name, baseline) == sha
    identifier = app.call("open_document", path=str(original), precision="preserve")["document"]["id"]
    reopened = support.call_for(app, identifier, folder / "reopen-calls.jsonl")
    assert native(reopened, folder / "reopened.rz", mode, opacity, base_name, baseline) == sha
    assert rendered(reopened, folder / "reopened.png", mode, opacity) == error
    undo("action-undone")
    return {"numerical_error": error, "opacity": opacity, "native_sha256": sha, "exact_sources_and_plain_raster_layer": True,
            "one_undo_redo_and_action_replay": True, "native_reopen_exact": True}


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
        assert doc["blend_version"]==(30 if mode in MODES else 27 if mode=="Overlay" else 26 if mode=="Multiply" else None)
        assert len(doc["layers"])==len(doc["fallbacks"])==2
        assert doc["profile"]==fixtures.PROFILE.read_bytes()
        for index in range(2):
            row=doc["layers"][index];mask=row["mask"]
            assert row["name"]==names[index] and row["opacity"]==(1 if index==0 else .625)
            assert row["blend"]==(0 if index==0 or mode is None else MODES.get(mode,{"Overlay":3,"Multiply":1}.get(mode)))
            assert row["visible"] and (row["x"],row["y"])==(0,0)
            assert list(support.png_samples(doc["fallbacks"][index])["samples"])==expected["rgba"]
            assert mask["data"]==mask_bytes
            for key in ("x","y","width","height","density","feather","linked","enabled","outside_coverage"):
                assert mask[key]==expected["mask"][key],(key,mask,expected)
        return digest(path)

    baseline_sha=stored(output/"masked-baseline.rz")
    older={}
    for old in ("Multiply","Overlay"):
        call("set_layer_properties",index=1,blend_mode=old)
        older[old]=stored(output/("masked-old-"+old+".rz"),old)
        call("undo");assert history(call)[0]==baseline[0]
    result={}
    for mode in MODES:
        label=mode.lower().replace(" ","-")
        app.call("start_recording")
        call("set_layer_properties",index=1,blend_mode=mode)
        action_name="Masked four "+mode
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
        for old in ("Multiply","Overlay"):
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
                      "mask29_blend30_roundtrip":True,"exact_nested26_27_downgrade":True,"one_undo_redo_and_action":True}
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
            "fixture_sha256":{p.name:digest(p) for p in (back,front)},"modes":{}}
    with support.HostApp(output/"app",output/"recovery",executable) as app:
        identifier=app.call("open_document",path=str(back),precision="preserve")["document"]["id"]
        call=support.call_for(app,identifier,output/"calls.jsonl")
        assert call("get_precision")["mode"]=="rgba16"
        placed = call("place_image",path=str(front),storage="embedded")
        assert placed["layer"]==1 and placed["source"] is None, placed
        call("set_active_layer",index=1)
        for mode in MODES:
            report["modes"][mode]=[]
            for opacity in (1,.625):
                folder=output/(mode.lower().replace(" ","-")+"-"+str(opacity))
                report["modes"][mode].append(exercise(app,call,mode,opacity,folder))
        report["outside_mask_composition"]=masked_workflows(app,output)
    assert all(digest(p)==report["fixture_sha256"][p.name] for p in (back,front))
    report["fixtures_unchanged"]=True
    report["checks_passed"]=["four independent f64 equations with endpoint precedence and near-singular neighbors",
        "full and fractional opacity with partial/tiny source and backdrop alpha",
        "separate measured division-sensitive bounds and unchanged ordinary/sum bounds",
        "exact raw RGB, alpha and hidden samples", "conditional native30 roundtrip",
        "atomic property edit, exact Undo, Redo and Actions replay",
        "independent outside255 masks composed with all four modes in mask29/blend30 wrappers",
        "exact raw mask/sample persistence, nested26/27 downgrades, Actions and native reopen"]
    (output/"result.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({"output":str(output),"modes":report["modes"],"checks_passed":report["checks_passed"]},indent=2))


if __name__ == "__main__":
    main()
