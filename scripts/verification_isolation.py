"""The private preference directories every owned test instance is given.

Standard library only, and it imports nothing from `scripts/`: `phase5_support`
imports `verify_phase1`, and both (with every other launcher) read this list.

The agent socket is not among them because it is per INSTANCE, not per folder:
every launcher gives each instance it starts its own `DR_AGENT_SOCKET` from
`mcp_client.scratch_socket()` (a fresh `0700` folder under the temp dir, for
macOS's 103-byte socket path), so no test binds the person's own socket.
"""

import os
from pathlib import Path

# Every private directory the app reads besides Actions and recovery
# (app/CLAUDE.md "Verification isolation"): each preference library plus
# DR_PRINTERS_DIR, the read-only printer-profile scan root. `App` points any
# the caller has not set into its own folder, so no owned instance reads or
# writes the user's real libraries. `HostApp.PRESET_DIRECTORIES` is this list.
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
                      # Edit ▸ Color Settings' keys (#547): `ColorSettingsStore` reads launch
                      # arguments, then `color-settings.plist` here, so an owned instance never
                      # follows (or writes) the person's own Color Settings.
                      "DR_COLOR_SETTINGS_DIR": "color-settings",
                      # Settings ▸ Cloud: the provider key files and `cloud.json`
                      # (consents, the agent switch), so no instance reads the person's keys.
                      "DR_CLOUD_DIR": "cloud",
                      # Window ▸ Styles: the user's style presets.
                      "DR_STYLE_PRESETS_DIR": "style-presets",
                      # A recorded Synchronize RAW Settings Masking step's masks.
                      "DR_RAW_LOCAL_MASKS_DIR": "raw-local-masks"}

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
