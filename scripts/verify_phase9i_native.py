#!/usr/bin/env python3
"""Verify internal cutout persistence through an isolated production app.

The input is native-refined-cutout.rz from the AppKit selection source probe.
This does not expose an advanced refinement command or qualify image matting.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("native_cutout_support", ROOT / "scripts/retouch-probe/support.py")
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)
MATTE = (1,257,32771,65534,123,65535)
CHANNEL_NAME = "Reusable Cutout"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inspect(path, *, reusable_channel=False):
    parsed = support.read_native(path)
    value = parsed["document"]
    assert (value["width"], value["height"], value["depth"]) == (3, 2, 16)
    source = support.png_samples(value["fallbacks"][1])
    expected = [(101,202,303,1),(1101,2202,3303,7),(11001,22002,33003,32769),
                (4001,5003,6005,65535),(65001,55003,45005,0),(101,65534,513,65535)]
    assert source["depth"] == 16 and tuple(source["samples"]) == tuple(code for pixel in expected for code in pixel)
    original = support.png_samples(value["fallbacks"][0])
    assert tuple(original["samples"]) == (1003,2005,3007,1)*6
    assert not value["layers"][0]["visible"]
    mask = value["layers"][1]["mask"]
    assert struct.unpack("<6H", mask["data"]) == MATTE
    assert (mask["x"],mask["y"],mask["width"],mask["height"]) == (0,0,3,2)
    if reusable_channel:
        assert parsed["version"] == 25 and len(value["channels"]) == 1
        channel = value["channels"][0]
        assert channel["name"] == CHANNEL_NAME
        assert struct.unpack("<6H", channel["data"]) == MATTE
        assert tuple(channel["color"]) == (0x35,0x79,0xBD)
        assert channel["opacity"] == 0.625 and channel["indicates_selected"]
    else:
        assert value["channels"] == []
    return value["layers"][1]["name"]


def selection_samples(call):
    result = call("get_selection", points=[{"x":x,"y":y} for y in range(2) for x in range(3)])
    return tuple(sample["coverage16"] for sample in result["samples"])


def verify_reusable_channel(app, call, output):
    call("load_selection", **{"from":"layer_mask"}, layer=1)
    assert selection_samples(call) == MATTE
    call("save_selection", name=CHANNEL_NAME)
    call("set_channel_options", channel=CHANNEL_NAME, overlay_color="#3579BD",
         overlay_opacity=0.625, color_indicates="selected")
    saved = output / "reusable-channel.rz"
    call("save_copy", path=str(saved))
    assert inspect(saved, reusable_channel=True) == "Refined Cutout"
    document = app.call("open_document", path=str(saved), precision="preserve")["document"]["id"]
    reopened = support.call_for(app, document, output / "reusable-channel-calls.jsonl")
    assert reopened("get_precision")["mode"] == "rgba16"
    reopened("load_selection", channel=CHANNEL_NAME)
    assert selection_samples(reopened) == MATTE
    roundtrip = output / "reusable-channel-reopened.rz"
    reopened("save_copy", path=str(roundtrip))
    assert inspect(roundtrip, reusable_channel=True) == "Refined Cutout"
    # Restore the original owned document's clean pixel state. The reopened
    # channel selection is transient and does not dirty its saved document.
    call("undo")
    call("undo")
    restored = output / "reusable-channel-undone.rz"
    call("save_copy", path=str(restored))
    assert inspect(restored) == "Refined Cutout"
    return {"path":str(roundtrip),"sha256":digest(roundtrip),"native_version":25,
            "channel_samples":list(MATTE),"overlay_color":"#3579BD",
            "overlay_opacity":0.625,"color_indicates":"selected"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    executable = (args.app / "Contents/MacOS/Doctored" if args.app.suffix == ".app" else args.app).resolve()
    output = args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error("output must be new and outside the repository")
    output.mkdir(exist_ok=False)
    assert inspect(args.source) == "Refined Cutout"
    receipt = {"executable":str(executable),"executable_sha256":digest(executable),
               "source":str(args.source.resolve()),"source_sha256":digest(args.source),
               "native_rgba_and_matte_separate":True,"advanced_controls_enabled":False}
    with support.HostApp(output / "app", output / "recovery", executable) as app:
        document = app.call("open_document", path=str(args.source.resolve()), precision="preserve")["document"]["id"]
        call = support.call_for(app, document, output / "calls.jsonl")
        assert call("get_precision")["mode"] == "rgba16"
        first = output / "opened.rz"
        call("save_copy", path=str(first))
        assert inspect(first) == "Refined Cutout"
        call("set_layer_properties", index=1, name="Native cutout verified")
        edited = output / "edited.rz"
        call("save_copy", path=str(edited))
        assert inspect(edited) == "Native cutout verified"
        call("undo")
        undone = output / "undone.rz"
        call("save_copy", path=str(undone))
        assert inspect(undone) == "Refined Cutout"
        call("redo")
        redone = output / "redone.rz"
        call("save_copy", path=str(redone))
        assert inspect(redone) == "Native cutout verified"
        # Leave the owned document clean before HostApp stops only its process.
        # This app version has no public close-document MCP command.
        call("undo")
        receipt["reusable_channel"] = verify_reusable_channel(app, call, output)
    receipt["checks_passed"] = ["production native reopen", "exact RGBA16 source alpha and independent u16 matte",
                                "hidden original samples", "native save copy", "central edit/Undo/Redo",
                                "G public mask-to-selection-to-channel exact u16 transfer",
                                "native v25 channel options and exact RGBA/mask/channel reopen"]
    (output / "result.json").write_text(json.dumps(receipt,indent=2)+"\n")
    print(json.dumps(receipt,indent=2))


if __name__ == "__main__":
    main()
