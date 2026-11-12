"""The private preference directories every owned test instance is given.

Standard library only, and it imports nothing from `scripts/`: `phase5_support`
imports `verify_phase1`, and both (with every other launcher) read this list.

The agent socket is not among the directories because it is per INSTANCE, not
per folder: every launcher gives each instance it starts its own
`DR_AGENT_SOCKET` from `mcp_client.scratch_socket()` (a fresh `0700` folder
under the temp dir, for macOS's 103-byte socket path), so no test binds the
person's own socket.

`owned_environment(folder, agent_socket)` is the whole set of additions an owned
instance's environment makes to `os.environ`: the background and automation
switches, that socket, the dummy API key, the pinned working limit, private
Actions and recovery folders, and `preset_environment(folder)`. The gate's
`launch` step and `scripts/updater_check.py`'s D9 launch both use it, so the two
cannot drift.
"""

import os
from pathlib import Path

# Every private directory the app reads besides Actions and recovery, which
# `owned_environment` adds (app/CLAUDE.md "Verification isolation"): each
# preference library plus DR_PRINTERS_DIR, the read-only printer-profile scan
# root. `App` points any the caller has not set into its own folder, so no owned
# instance reads or writes the user's real libraries.
# `HostApp.PRESET_DIRECTORIES` is this list.
PRESET_DIRECTORIES = {"DR_RAW_PRESETS_DIR": "raw-presets",
                      "DR_EXPORT_PRESETS_DIR": "export-presets",
                      "DR_METADATA_POLICIES_DIR": "metadata-policies",
                      "DR_DOCUMENT_PRESETS_DIR": "document-presets",
                      "DR_SHORTCUTS_DIR": "shortcuts",
                      "DR_WORKSPACES_DIR": "workspaces",
                      "DR_PROOF_PRESETS_DIR": "proof-presets",
                      "DR_ADJUSTMENT_PRESETS_DIR": "adjustment-presets",
                      "DR_DEVELOP_FILTER_PRESETS_DIR": "develop-filter-presets",
                      "DR_HDR_TONING_PRESETS_DIR": "hdr-toning-presets",
                      "DR_CLIPBOARD_PAYLOADS_DIR": "clipboard-payloads",
                      "DR_PRINTERS_DIR": "printers",
                      "DR_MODELS_DIR": "models",
                      "DR_SKIES_DIR": "skies",
                      # Edit ▸ Color Settings' keys (#1): `ColorSettingsStore` reads launch
                      # arguments, then `color-settings.plist` here, so an owned instance never
                      # follows (or writes) the person's own Color Settings.
                      "DR_COLOR_SETTINGS_DIR": "color-settings",
                      # Settings ▸ Cloud: the provider key files and `cloud.json`
                      # (consents, the agent switch), so no instance reads the person's keys.
                      "DR_CLOUD_DIR": "cloud",
                      # Window ▸ Styles: the user's style presets.
                      "DR_STYLE_PRESETS_DIR": "style-presets",
                      # A recorded Synchronize RAW Settings Masking step's masks.
                      "DR_RAW_LOCAL_MASKS_DIR": "raw-local-masks",
                      # The paint options bar's Preset popup: the user's brush presets.
                      "DR_BRUSH_PRESETS_DIR": "brush-presets",
                      # Edit ▸ Define Pattern…: the user's pattern library.
                      "DR_PATTERNS_DIR": "patterns"}

# Every cloud provider's base URL, pointed at port 9 on loopback (nothing listens there:
# the connection is refused at once), so an owned instance that reaches a provider by
# mistake never leaves the machine. A drive that owns a stub server overrides it.
PROVIDER_BASE_URLS = {
    "DR_OPENAI_BASE_URL": "http://127.0.0.1:9/v1",
    "DR_GEMINI_BASE_URL": "http://127.0.0.1:9/v1beta",
    "DR_FAL_BASE_URL": "http://127.0.0.1:9",
}


def preset_environment(folder: str | os.PathLike) -> dict[str, str]:
    """Every PRESET_DIRECTORIES entry, pointed into `folder`, and PROVIDER_BASE_URLS."""
    environment = {name: str(Path(folder) / leaf) for name, leaf in PRESET_DIRECTORIES.items()}
    environment.update(PROVIDER_BASE_URLS)
    return environment


def owned_environment(folder: str | os.PathLike, agent_socket: str | os.PathLike) -> dict[str, str]:
    """The additions every owned instance's environment makes to `os.environ`.

    `DR_BACKGROUND` and `DR_AUTOMATION_ALERTS` make it a background, automation launch
    (so `AutomationLaunch` holds and the updater is never constructed), `agent_socket` is
    its private scratch socket, the dummy key short-circuits every stored-key read, the
    working limit is the 2 GiB reference, and Actions, recovery and every
    `preset_environment(folder)` entry point into `folder`.
    """
    environment = {"DR_BACKGROUND": "1",
                   "DR_AGENT_SOCKET": str(agent_socket),
                   "ANTHROPIC_API_KEY": "dummy",
                   "DR_AUTOMATION_ALERTS": "record",
                   "DR_WORKING_LIMIT_BYTES": "2147483648",
                   "DR_ACTIONS_DIR": str(Path(folder) / "actions"),
                   "DR_RECOVERY_DIR": str(Path(folder) / "recovery")}
    environment.update(preset_environment(folder))
    return environment
