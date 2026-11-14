# Select ▸ Focus Area

Status: decided 2026-11-14 (batch c12-b2, [#17](https://github.com/kpgalligan/doctored/issues/17), which stays open for the Magnetic Lasso's part B). Kernel `core/src/focus_area.rs`, export `dr_selection_focus_area` (`core/src/ffi_focus_area.rs`), app `FocusArea.swift`, MCP `select_focus_area`.

Focus Area selects the part of a photograph that is in focus. The rule is Doctored's own; it is not measured against any other editor's result.

## Decisions

1. **Input.** The selection capture's 8-bit analysis source (`SelectionCapture.source`), as Quick Selection reads it: the composite at 8 bits, its display derivative at 16, the R1 display codes at 32-bit float. The output is byte coverage. No model and no download.
2. **Measure.** Stack Images' two-band sharpness score over Rec. 709 luma (`focus_stack::sharpness`): local detail energy at the fine and the coarse band, summed, 0 where the picture is transparent. One score, so Focus Area and Stack Images can never disagree on what is sharp.
3. **Floor.** Detail energy below `(1/255)²` (one 8-bit code RMS) is no detail. A flat area can be in focus or not and nothing in it says which, so the measure never selects it; the region rule (5) may.
4. **Threshold.** `range` 0–100 places the cut that fraction of the way up `ln(score)`, from `ln(floor)` to the picture's largest; a lower `range` selects more. Auto (no `range`) is Otsu's cut over a 256-bin histogram of the opaque pixels above the floor, reported as `range = 100·(k + 1)/256`. That value is a dyadic fraction, so it maps back onto the same cut and a recorded step replays bit for bit. When Otsu's two classes' mean `ln(score)` differ by less than `ln 4` (a fourfold energy ratio, twice the RMS detail), the picture has one focus and Auto takes range 0. A picture with no pixel above the floor selects nothing, and Auto reports 100.
5. **Regions.** One majority vote over Stack Images' vote box (twice the coarse band's radius: 8 px below a 4000-pixel side): a pixel is selected when more than half of the opaque pixels in its box are, and a tie keeps its own. Then every unselected opaque four-connected region that touches neither the canvas border nor a transparent pixel is filled (Quick Selection's hole fill, uncapped), so a sharp subject's smooth interior belongs to it. Coverage is the source alpha, so transparent pixels stay outside.
6. **Edge.** `soften_edge` (default true) feathers the coverage by the coarse band's radius (`gaussian_plane::feather`) and caps each pixel at its alpha.
7. **Scope.** RGB documents at 8, 16 and 32 bits, from Select ▸ Focus Area… and MCP `select_focus_area` (facts `.commonTool`, `.floatTool`). The colour modes (Grayscale, Duotone, Lab, Indexed, Bitmap, CMYK) refuse it through their closed tool and selector sets until a batch checks each mode's analysis source and runs it there.
8. **Dialog.** Auto (on by default), In-Focus Range (enabled when Auto is off) and Soften Edge. Apply sets the selection in the menu's Replace mode; there is no live preview, the selection is the preview and Undo takes it back. The recorded step carries the resolved `range`.
