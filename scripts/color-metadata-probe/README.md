# Color and metadata source probe

Run `make rust project`, then `python3 scripts/color-metadata-probe/verify.py`.
The runner compiles the real app sources with a probe entry point, dummy
credentials, temporary fixtures, and a private preferences domain. `--keep`
retains the independent ImageIO reads, TIFF/HEIC/DNG fixtures and logs.

It exercises ImageIO metadata serialization and actual RAW/HEIC document opens,
normalized TIFF/PNG/JPEG exports read independently by ImageIO, and the shared
calibrated adjustment and PSD import UI/MCP paths. The synthetic DNG checks the
decoder/metadata bridge; it does not qualify camera rendering quality.
The `refused profiles` check opens and pastes a YCbCr TIFF (which only the
ImageIO fallback reads) whose embedded profile is over 16 MiB, and requires
sRGB-converted pixels plus the refusal in the import report and the Assign
Profile sentence. The `core open profiles` check has ImageIO write a Gray JPEG,
a CMYK TIFF and a CMYK JPEG, opens each through the core, and requires the
import report's `profile_refusal` (converted, converted, converted) and pixels
matching ColorSync — the TIFF's and the JPEG's rich-dark stripe against
ColorSync with black-point compensation. The ImageIO-written CMYK JPEG is the
qualification of the core's Adobe APP14 inversion rule.

For the bounded PSD check:

```sh
python3 -B scripts/color-metadata-probe/verify.py \
  --checks 'PSD imports' --archive /path/to/libdoctored_core.a --keep
```

It exercises
actual compatibility choices, Cancel, MCP preflight and modal-free Revert using
an independently authored PSD with CoreGraphics Display P3 ICC bytes and distinct
300/240 PPI. Exact metadata and pixels survive both UI/MCP import choices and
native reopen; a separate Python reader checks source resources and ten native
PSD-import saves. An ordinary flat P3 PNG still converts to the sRGB working
space, while reusing that document for PSD clears its previous conversion status.
This is Doctored UI/metadata qualification, not independent PSD-editor
appearance evidence.

The runner physically captures app/core/header/helper/build inputs and the exact
archive before compiling, checks source/copy hashes after execution, and records
the probe, archive, input, profile and artifact hashes in `result.json`. Probe
preferences and Actions/recovery/RAW/filter-preset stores are private. Defaults
still run all checks; `--checks` omits unrelated camera work when only PSD is needed.
Execution requires normal macOS graphics and LaunchServices access; a sandbox
registration failure does not qualify the app's document-opening behavior.
