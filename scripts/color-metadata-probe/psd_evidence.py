"""Independent byte/profile/PPI audit of the PSD source-probe artifacts."""
import hashlib
import importlib.util
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(files):
    spec = importlib.util.spec_from_file_location(
        "color_probe_native_reader", ROOT / "scripts/live-filter-probe/native.py")
    native = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    profile = (files / "psd-source-profile.icc").read_bytes()
    source = (files / "psd-profile-source.psd").read_bytes()
    assert source[:12] == b"8BPS\0\1\0\0\0\0\0\0"
    assert struct.unpack_from(">HIIHH", source, 12) == (3, 1, 1, 8, 3)
    at = 26
    color_length, = struct.unpack_from(">I", source, at)
    at += 4 + color_length
    resource_length, = struct.unpack_from(">I", source, at)
    at += 4
    end = at + resource_length
    resources = {}
    while at < end:
        assert source[at:at + 4] == b"8BIM"
        identifier, = struct.unpack_from(">H", source, at + 4)
        at += 6
        name_length = source[at] + 1
        at += name_length + name_length % 2
        length, = struct.unpack_from(">I", source, at)
        at += 4
        assert identifier not in resources and at + length <= end
        resources[identifier] = source[at:at + length]
        at += length + length % 2
    assert at == end and set(resources) == {1005, 1039}
    assert resources[1039] == profile and profile[16:20] == b"RGB "
    assert resources[1005] == struct.pack(">IHHIHH", 300 << 16, 1, 1, 240 << 16, 1, 1)
    for name in ("psd-import-report.json", "psd-mcp-inspect.json"):
        report = json.loads((files / name).read_text())
        assert report["resolution"] == {"x": 300, "y": 240}
        assert report["profile"] and report["requires_explicit_choice"]
        assert report["layered_available"] and report["stored_composite_available"]

    results = []
    for origin in ("ui", "mcp", "reused"):
        variants = (("layers", (200, 200, 200, 255)),) if origin == "reused" else (
            ("layers", (200, 200, 200, 255)), ("composite", (10, 20, 30, 255)))
        for mode, rgba in variants:
            name = f"psd-{origin}-{mode}"
            original = files / f"{name}.rz"
            reopened = files / f"{name}.reopened.rz"
            assert original.read_bytes() == reopened.read_bytes()
            for path in (original, reopened):
                document = native.read_native(path)["document"]
                assert (document["depth"], document["width"], document["height"]) == (8, 1, 1)
                assert document["resolution"] == (300, 240)
                assert document["profile"] == profile
                assert len(document["layers"]) == 1
                assert document["layers"][0]["mask"] is None
                pixel = native.png_samples(document["fallbacks"][0])
                assert (pixel["width"], pixel["height"], pixel["depth"]) == (1, 1, 8)
                assert tuple(pixel["samples"]) == rgba
                results.append({"path": path.name, "sha256": digest(path),
                                "profile_sha256": hashlib.sha256(profile).hexdigest(),
                                "resolution": list(document["resolution"]), "rgba": list(rgba)})
    flat_source = native.png_samples((files / "psd-flat-control.png").read_bytes())
    assert flat_source['profile'] == profile
    assert tuple(flat_source['samples']) == (197, 100, 43, 255)
    flat_document = native.read_native(files / "psd-flat-control.rz")['document']
    working = (files / "psd-flat-working-profile.icc").read_bytes()
    # Native byte v10 omits its builtin sRGB ICC slot; an absent profile means
    # that declared builtin. A different present profile cannot pass this control.
    assert flat_document['profile'] is None or flat_document['profile'] == working
    converted = native.png_samples(flat_document['fallbacks'][0])
    assert tuple(converted['samples'])[:3] != (197, 100, 43)
    assert tuple(converted['samples'])[3:] == (255,)
    return {"native_files": results, "source_sha256": digest(files / "psd-profile-source.psd"),
            "profile_sha256": digest(files / "psd-source-profile.icc"),
            "flat_control": {"source_sha256": digest(files / "psd-flat-control.png"),
                             "native_sha256": digest(files / "psd-flat-control.rz"),
                             "working_profile_sha256": digest(files / "psd-flat-working-profile.icc"),
                             "source_rgba": list(flat_source['samples']),
                             "converted_rgba": list(converted['samples'])},
            "independent_source_resources_and_native_metadata_match": True}
