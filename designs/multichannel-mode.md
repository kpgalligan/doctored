# Multichannel document mode (ADR)

- **Status:** Accepted — closed; decided 2026-10-22 (owner): not built. [#1](https://github.com/kpgalligan/doctored/issues/1)
  (the colour-mode umbrella) stays open (see [cmyk-mode-status.md](cmyk-mode-status.md)).
- **Issue:** [#1](https://github.com/kpgalligan/doctored/issues/1).
- **Code:** the two refusal sites: `core/src/psd_container.rs` (`MULTICHANNEL_REFUSAL`, the first arm of
  `Container::support_refusal`) and `app/Sources/AgentServer+CmykMode.swift` (`convertMode`'s
  `multichannelRefusal`). Tests: `core/tests/multichannel_psd.rs`; drive gate G8 of `scripts/verify_cmyk_mode.py`.
- **Takes:** none.

## Context

A Multichannel PSD or PSB (colour mode 7) holds 8- or 16-bit ink channels, spot colour channels with no
composite colour model and no layers.

## Decision

Multichannel mode is not built. Why: it is niche for a photo editor; the owner decided 2026-10-22 not to build it,
and spot channels ([spot-channels.md](spot-channels.md)) shipped without it.

## What Doctored does instead

Every place Doctored can meet Multichannel refuses with a sentence naming it, never a phase.

- **A Multichannel PSD or PSB, at any depth.** `inspect_psd` and the open notice report `color_mode`
  "Multichannel", and both choices refuse with `MULTICHANNEL_REFUSAL`: "Multichannel files are not supported". Every open (byte, 16-bit, 32-bit) refuses with it as
  `PSD <path>: Multichannel files are not supported`; the Grayscale and Lab
  opens refuse it by their own admissions ("a Multichannel file is not Grayscale" / "… is not Lab Color").
  The check is `support_refusal`'s first, so even an oversized file names Multichannel.
- **TIFF.** Photoshop writes no Multichannel TIFF, and the layered-TIFF reader admits only RGB, CMYK, Gray and
  Lab (`tiff_layers::locate`), so no TIFF reaches it. DCS 2.0 and Photoshop Raw are not read.
- **Image ▸ Mode.** There is no Multichannel item; the menu lists only the modes Doctored builds. Over MCP,
  `convert_mode mode "multichannel"` answers in-band: "Multichannel documents are not supported."
- **Deleting a colour channel.** Delete Channel acts only on alpha channels: the Channels panel's trash button,
  its row menu and Image ▸ Channels ▸ Delete Channel are disabled on the composite and colour-plane rows
  (`EditorViewController+Channels.swift` validation, `.channel(index)` targets only), and `delete_channel`
  addresses alpha channels only. So no document ever becomes Multichannel.

The refusal sentences quoted above are the code's current wording; they predate spot channels.

## What would reopen it

An owner decision to build it over the shipped spot channels. `DROPPED` (2026-10-30) — reopening it because spot
channels shipped: the owner's decision stands.
