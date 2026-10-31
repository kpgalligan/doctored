#!/usr/bin/env python3
"""Qualify Phase 5 history, full-state snapshots and actual crash recovery.

Set DR_VERIFY_APP to the isolated built executable. Every process uses a private
scratch DR_AGENT_SOCKET, private Actions/recovery directories, suppressed startup
recovery UI and a dummy key.
Only processes and fixtures created by this runner are terminated or modified.
"""

import argparse
from pathlib import Path
import shutil
import tempfile

from phase5_history import grouped_action_history, history_flows
from phase5_recovery import crash_and_restart, default_interval_checkpoint, failed_write_and_previous_generation
from phase5_snapshots import snapshot_flows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--suite", choices=["all", "history", "snapshots", "recovery"], default="all")
    arguments = parser.parse_args()
    folder = Path(tempfile.mkdtemp(prefix="rasterize-phase5-"))
    print(f"Verification scratch: {folder}", flush=True)
    try:
        if arguments.suite in ("all", "history"):
            history_flows(folder / "history")
            grouped_action_history(folder / "actions")
        if arguments.suite in ("all", "snapshots"):
            snapshot_flows(folder / "snapshots")
        if arguments.suite in ("all", "recovery"):
            default_interval_checkpoint(folder / "default-interval")
            crash_and_restart(folder / "crash")
            failed_write_and_previous_generation(folder / "faults")
    except BaseException:
        print(f"Failure artifacts retained: {folder}", flush=True)
        raise
    if not arguments.keep:
        shutil.rmtree(folder)


if __name__ == "__main__":
    main()
