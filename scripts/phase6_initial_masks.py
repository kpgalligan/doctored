#!/usr/bin/env python3
"""Capture real initial masks using the app's existing Vision selection path.

Requires DR_VERIFY_APP pointing to an isolated built app and the four photos
from phase6_quality_fixtures.py. Output includes exact full-resolution rendered
source/coverage PNGs and per-case tool responses. No model is reimplemented.
"""
import argparse
import base64
import json
from pathlib import Path
from phase5_support import App


def render(app, document, path, **extra):
    result = app.client.rpc("tools/call", {"name": "render", "arguments": {
        "document_id": document, "max_side": 4096, **extra}})
    assert not result.get("isError"), result
    if "channel" in extra:
        descriptions = " ".join(v.get("text", "") for v in result["content"])
        assert extra["channel"] in descriptions, descriptions
    block = next(v for v in result["content"] if v["type"] == "image")
    path.write_bytes(base64.b64decode(block["data"]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    folder = args.directory
    records = []
    with App(folder / "vision-runtime", folder / "vision-recovery", interval=None) as app:
        for name, suffix in [("portrait", "png"), ("fur", "png"), ("glass", "jpg"), ("product", "jpg")]:
            document = app.open(folder / f"{name}.{suffix}")
            info = app.call("get_document", document_id=document)
            render(app, document, folder / f"{name}-source.png")
            response = app.call("select_subject", document_id=document)
            channel = app.call("add_channel", document_id=document, name="Initial subject coverage", **{"from": "selection"})
            render(app, document, folder / f"{name}-initial.png", channel="Initial subject coverage")
            records.append({"case": name, "dimensions": [info["width"], info["height"]],
                            "method": "existing select_subject / macOS Vision foreground instance mask",
                            "response": response, "ground_truth": False})
            print(name, response, flush=True)
    (folder / "initial-masks.json").write_text(json.dumps(records, indent=2) + "\n")


if __name__ == "__main__":
    main()
