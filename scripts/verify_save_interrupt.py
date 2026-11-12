#!/usr/bin/env python3
"""A killed save's staged files are swept, and a write that fails part-way keeps the previous file.

Part 1, the sweep. A scratch folder is planted with the hidden staged names a killed Doctored
leaves beside a destination (the app's `.doctored-save-…`, `.doctored-precision-save-…`,
`.doctored-export-…`, `.doctored-cmyk-….tif`, `.dr-pdf-…` and the `.doctored-snapshot-…` package
directory, and the core's `<name>.dr-tmp-<pid>-<seq>`), then a 16-bit document's native copy is
saved into it through
`save_copy` (the staged `CapturedSave.writeSafely` path, whose core write sweeps the folder too).
Gone afterwards: every name whose pid is a dead process (a child this script spawned and reaped)
and an old UUID-only name (mtime two days back). Kept: names carrying the instance's own pid,
this script's (alive) and launchd's (pid 1, alive, which answers EPERM), a fresh UUID-only name,
a name that only starts like one, a directory and a symlink named like staged files (the
symlink's target untouched), a live pid's package, a plain file named like a package, an older
build's `.snapshot-<UUID>` package, and the core names of the instance's own pid and of launchd.

Part 2, the part-way failure. A second instance runs with RLIMIT_FSIZE at 2 MiB and SIGXFSZ
ignored, so any write past 2 MiB fails with EFBIG part-way through the file. For an 8-bit native
copy (the core's whole-buffer write), a 16-bit native copy (the app's staged file around the
core's streamed write) and a 16-bit PNG copy (the staged export): a small copy saves, then a
noise document's copy to the same path must answer in-band with an error, leave the first copy's
bytes unchanged and leave no staged or temporary name in the folder; the instance must still
be running.

Run after `make app-debug`; `DR_VERIFY_APP` names another build. Each instance is an owned,
background one (`owned_environment`: a scratch socket, the dummy key, private folders) and is
terminated by this script; nothing takes focus. Prints one PASS or FAIL line per check and exits
non-zero on a FAIL.

    python3 scripts/verify_save_interrupt.py --output <fresh folder>
"""

import argparse
import contextlib
import hashlib
import os
from pathlib import Path
import resource
import signal
import subprocess
import sys
import time
import uuid

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import app_under_test  # noqa: E402
from mcp_client import Client, remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verification_isolation import owned_environment  # noqa: E402
from verify_phase1 import png  # noqa: E402

LIMIT = 2 * 1024 * 1024
NOISE_SIDE = 1024
FAILURES = []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f" — {detail}" if detail and not ok else ""),
          flush=True)
    if not ok:
        FAILURES.append(name)


@contextlib.contextmanager
def instance(folder, file_size_limit=None):
    """An owned background instance; with `file_size_limit`, every file it writes stops there."""
    executable = app_under_test.default_app_executable()
    app_under_test.require_fresh_app(executable)
    folder.mkdir(parents=True, exist_ok=True)
    socket = scratch_socket()
    environment = dict(os.environ, **owned_environment(folder, socket),
                       DR_EDR_HEADROOM="1", DR_RECOVERY_INTERVAL_SECONDS="600")

    def limited():
        # Ignored, a write past the limit fails with EFBIG instead of killing the process;
        # the disposition survives exec.
        signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
        resource.setrlimit(resource.RLIMIT_FSIZE, (file_size_limit, file_size_limit))

    with (folder / "app.log").open("w") as log:
        app = subprocess.Popen(
            [str(executable), "-ApplePersistenceIgnoreState", "YES",
             "-RecoveryDiscoverySuppressed", "YES", "-ColorWorkingSpace", "srgb"],
            env=environment, stdout=log, stderr=subprocess.STDOUT,
            preexec_fn=limited if file_size_limit else None)
        try:
            client = Client(socket)
            wait_ready(client, app, 30, log.name)
            yield app, client
        finally:
            if app.poll() is None:
                app.terminate()
                try:
                    app.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait(timeout=10)
            remove_scratch(socket)


def dead_pid():
    """The pid of a child this script spawned and reaped: no process answers to it."""
    child = subprocess.Popen(["/usr/bin/true"])
    child.wait()
    return child.pid


def token():
    return str(uuid.uuid4()).upper()


def new_document(client, depth, side=64):
    return client.call("new_document", width=side, height=side, unit="px", depth=depth,
                       background="white")["document"]["id"]


def leftovers(folder):
    return sorted(name for name in os.listdir(folder)
                  if name.startswith((".doctored-", ".dr-pdf-")) or ".dr-tmp-" in name)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sweep(output):
    folder = output / "sweep" / "destination"
    folder.mkdir(parents=True)
    outside = output / "sweep" / "outside-target"
    outside.write_bytes(b"a symlink's target")
    with instance(output / "sweep" / "app") as (app, client):
        dead, own = dead_pid(), app.pid
        removed = [f".doctored-save-{dead}-{token()}",
                   f".doctored-precision-save-{dead}-{token()}",
                   f".doctored-cmyk-{dead}-{token()}.tif",
                   f".dr-pdf-{dead}-{token()}",
                   f".doctored-export-{token()}",
                   f"x.doctored.dr-tmp-{dead}-0",
                   f".doctored-save-{dead}-{token()}.dr-tmp-{dead}-4"]
        kept = [f".doctored-save-{own}-{token()}",
                f".doctored-save-{os.getpid()}-{token()}",
                f".doctored-export-1-{token()}",
                f".doctored-save-{token()}",
                ".doctored-save-notes.txt",
                f"y.png.dr-tmp-{own}-3",
                "z.dr-tmp-1-0"]
        for name in removed + kept:
            (folder / name).write_bytes(b"staged bytes")
        old = time.time() - 2 * 24 * 3600
        os.utime(folder / removed[4], (old, old))
        directory = f".doctored-save-{dead}-{token()}"
        (folder / directory).mkdir()
        link = f".doctored-export-{dead}-{token()}"
        (folder / link).symlink_to(outside)
        # A `.drsnapshot` package is staged as a directory: a dead pid's goes with its contents;
        # a live pid's, a plain file by that name and an older build's `.snapshot-<UUID>` stay.
        package = f".doctored-snapshot-{dead}-{token()}"
        live_package = f".doctored-snapshot-{own}-{token()}"
        legacy_package = f".snapshot-{token()}"
        for name in (package, live_package, legacy_package):
            (folder / name).mkdir()
            (folder / name / "document").write_bytes(b"package bytes")
        os.utime(folder / legacy_package, (old, old))
        plain = f".doctored-snapshot-{dead}-{token()}"
        (folder / plain).write_bytes(b"not a package")
        removed.append(package)
        kept += [directory, link, live_package, legacy_package, plain]

        document = new_document(client, 16)
        reply = client.call("save_copy", document_id=document,
                            path=str(folder / "copy.doctored"))
        check("S0 the 16-bit native copy saves into the planted folder", reply.get("ok") is True,
              reply)
        names = set(os.listdir(folder))
        print(f"     after the save: {sorted(names)}", flush=True)
        for name in removed:
            check(f"S1 swept: {name}", name not in names)
        for name in kept:
            check(f"S2 kept: {name}", name in names)
        check("S3 the symlink's target is untouched",
              outside.read_bytes() == b"a symlink's target")
        check("S4 the copy itself is in place", "copy.doctored" in names)


def failure(output):
    folder = output / "partway" / "destination"
    folder.mkdir(parents=True)
    noise = [output / "partway" / f"noise{depth}.png" for depth in (8, 16)]
    for path in noise:
        png(path, NOISE_SIDE, NOISE_SIDE, os.urandom(NOISE_SIDE * NOISE_SIDE * 4))
    with instance(output / "partway" / "app", file_size_limit=LIMIT) as (app, client):
        noisy8 = client.open(noise[0])
        noisy16 = client.open(noise[1])
        client.call("convert_precision", document_id=noisy16, mode="rgba16", structure="preserve")
        cases = [("8-bit native", 8, noisy8, "doc8.doctored", {}),
                 ("16-bit native", 16, noisy16, "doc16.doctored", {}),
                 ("16-bit PNG", 16, noisy16, "doc16.png", {"format": "png", "bit_depth": 16})]
        for label, depth, noisy, name, extra in cases:
            destination = folder / name
            small = new_document(client, depth)
            first = client.call("save_copy", document_id=small, path=str(destination), **extra)
            check(f"P0 {label}: the small copy saves", first.get("ok") is True, first)
            before = digest(destination)
            refusal = client.call("save_copy", fail=True, document_id=noisy,
                                  path=str(destination), **extra)
            check(f"P1 {label}: the noise copy fails part-way, in-band", bool(refusal), refusal)
            print(f"     {label} refusal: {refusal}", flush=True)
            check(f"P2 {label}: the previous copy's bytes are unchanged",
                  digest(destination) == before)
            check(f"P3 {label}: no staged or temporary name is left", not leftovers(folder),
                  leftovers(folder))
            client.call("close_document", document_id=small, discard_changes=True)
        check("P4 the instance is still running", app.poll() is None)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True)
    output = parser.parse_args().output.expanduser().resolve()
    assert not output.exists() or not any(output.iterdir()), "use a fresh output folder"
    output.mkdir(parents=True, exist_ok=True)
    sweep(output)
    failure(output)
    print("verify_save_interrupt: " + ("PASS" if not FAILURES else f"FAIL {FAILURES}"))
    raise SystemExit(0 if not FAILURES else 1)


if __name__ == "__main__":
    main()
