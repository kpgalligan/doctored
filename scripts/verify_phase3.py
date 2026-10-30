#!/usr/bin/env python3
"""Built-app Phase 3 MCP verification.

Run the color-metadata source probe with --keep first and pass its files folder
as --fixtures. DR_VERIFY_APP selects the isolated app executable. All application
processes, ports, Actions and outputs use verify_phase1's private test harness.
"""

import argparse
from pathlib import Path
import shutil
import tempfile

from verify_phase1 import ROOT, instance, png
from verify_phase2 import rendered


def metadata_flows(client, folder, fixtures):
    for name in ["p3-source.tiff", "capture.heic", "synthetic.dng"]:
        document = client.open(fixtures / name)
        call = lambda tool, **args: client.call(tool, document_id=document, **args)
        before = call("get_metadata")
        assert before["exif"]["present"], (name, before)
        assert not before.get("metadata_not_captured"), (name, before)
        assert before["metadata_import"]["metadata_walked"], (name, before)
        assert (before["ppi_x"], before["ppi_y"]) == (150, 300), (name, before)
        call("set_resolution", ppi_x=300, ppi_y=300)
        call("assign_profile", profile="display_p3")
        exported = folder / (name + ".tiff")
        saved = call("save_copy", path=str(exported), format="tiff")
        assert {"color_profile", "resolution", "exif"}.issubset(saved["wrote"]), saved
        assert "exif" not in saved["dropped"], saved
        reopened = client.open(exported)
        after = client.call("get_metadata", document_id=reopened)
        assert after["exif"]["present"], after
        # The working-space adoption is independently tested in the source
        # probe; exports above carry the actual assigned P3 profile and PPI.
        native = folder / (name + ".rz")
        call("save_copy", path=str(native), format="rz")
        native_id = client.open(native)
        restored = client.call("get_metadata", document_id=native_id)
        for packet in ["exif", "xmp", "iptc"]:
            assert restored[packet] == before[packet], (name, packet, before, restored)
        stripped = call("save_copy", path=str(folder / (name + "-stripped.tiff")),
                        format="tiff", strip_metadata=True)
        assert "exif" in stripped["dropped"] and "resolution" in stripped["wrote"], stripped
    print("PASS: MCP TIFF/HEIC/RAW capture imports, 300-PPI TIFF export, native packets and stripping", flush=True)


def calibrated_flows(client, folder):
    inputs = []
    for number in range(3):
        path = folder / f"calibrated-{number}.png"
        pixels = bytes(value for y in range(10) for x in range(12)
                       for value in (50 + x * 5, 70 + y * 4, 90 + x * 2, 255))
        png(path, 12, 10, pixels)
        document = client.open(path)
        client.call("assign_profile", document_id=document, profile="display_p3")
        inputs.append(document)
    direct, layered, replay = inputs
    operations = [("exposure", {"exposure": 0.6, "offset": 0.01, "gamma": 1.1}),
                  ("white_balance", {"temperature": 5300, "tint": 8})]
    client.call("start_recording", document_id=direct)
    for op, params in operations:
        client.call("apply_filter", document_id=direct, filter=op, params=params)
    record = client.call("stop_recording", document_id=direct, name="Phase Three Calibrated")
    assert "document_v2" in str(record), record
    for op, params in operations:
        response = client.call("add_adjustment_layer", document_id=layered, op=op, params=params)
        assert response["params"]["evaluation"] == "document_v2", response
    assert rendered(client, direct) == rendered(client, layered), "destructive/layer calibration mismatch"
    client.call("run_action", document_id=replay, name="Phase Three Calibrated")
    assert rendered(client, direct) == rendered(client, replay), "recording calibration mismatch"
    client.call("add_layer_mask", document_id=direct)
    failure = client.call("apply_filter", document_id=direct, filter="exposure", target="mask",
                          params={"exposure": 0.5, "evaluation": "document_v2"}, fail=True)
    assert "RGB" in failure, failure
    print("PASS: MCP profile-aware destructive/layer equivalence, explicit Actions semantics and scalar refusal", flush=True)


def psd_flows(client, folder):
    source = ROOT / "core/tests/fixtures/gradient.psd"
    report = client.call("inspect_psd", path=str(source))
    assert report["requires_explicit_choice"] and report["general_limitations"], report
    assert isinstance(report["verified_losses"], list), report
    refused = client.call("open_document", path=str(source), fail=True)
    assert "preflight" in refused, refused
    opened = client.call("open_document", path=str(source), psd_mode="composite",
                         psd_fingerprint=report["source_fingerprint"])
    state = opened["psd_import"]
    assert state["chosen_mode"] == "composite", opened
    layers_path = folder / "layers.psd"
    shutil.copyfile(source, layers_path)
    opened_layers = client.call("open_document", path=str(layers_path), psd_mode="layers",
                                psd_fingerprint=report["source_fingerprint"])
    assert opened_layers["psd_import"]["chosen_mode"] == "layers", opened_layers
    stale_path = folder / "stale.psd"
    shutil.copyfile(source, stale_path)
    error = client.call("open_document", path=str(stale_path), psd_mode="layers",
                        psd_fingerprint="outdated", fail=True)
    assert "changed" in error.lower(), error
    unsupported = folder / "16-bit.psd"
    data = bytearray(source.read_bytes())
    data[22:24] = b"\0\x10"
    unsupported.write_bytes(data)
    limits = client.call("inspect_psd", path=str(unsupported))
    assert not limits["layered_available"] and not limits["stored_composite_available"], limits
    client.call("open_document", path=str(unsupported), psd_mode="composite",
                psd_fingerprint=limits["source_fingerprint"], fail=True)
    print("PASS: MCP PSD report-before-import, explicit modes, stale source refusal and unsupported depth", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", required=True, type=Path)
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    for name in ["p3-source.tiff", "capture.heic", "synthetic.dng"]:
        if not (args.fixtures / name).is_file():
            parser.error(f"Missing {name}; run scripts/color-metadata-probe/verify.py --keep first.")
    folder = Path(tempfile.mkdtemp(prefix="rasterize-phase3-"))
    print(f"Verification scratch: {folder}", flush=True)
    try:
        with instance(folder) as client:
            metadata_flows(client, folder, args.fixtures)
            calibrated_flows(client, folder)
            psd_flows(client, folder)
    except BaseException:
        print(f"Artifacts retained: {folder}", flush=True)
        raise
    if not args.keep:
        shutil.rmtree(folder)


if __name__ == "__main__":
    main()
