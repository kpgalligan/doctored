---
title: Feature reference
description: What Doctored does, area by area — menus, tools, file formats, limits, the built-in assistant and agent access.
eyebrow: Reference
---

<!-- Moved from README.md on 2026-10-30. The README sweep keeps it current (root CLAUDE.md, "README"). -->

## Features

The design record behind each area is in the repository's
[`designs/`](https://github.com/kpgalligan/doctored/tree/main/designs) folder.

- **Recovery, history and snapshots**: dirty native and imported images, untitled
  work, selections and session snapshots get native recovery copies in
  `~/Library/Application Support/Doctored/Recovery`. Checkpoints are due after
  30 seconds; a live gesture, Action replay, busy worker or encoding time can
  delay completion. File > Recover Unsaved Work offers abandoned copies after
  restart, opening them as dirty untitled documents. Imported originals remain
  unchanged. Failed writes retain the previous checkpoint and report the error.
  When a checkpoint is refused for lack of working memory, the editor's status
  bar shows "Recovery off" and Recover Unsaved Work names the document and the
  reason. A 45 MP
  8-bit document (8256×5504, a white Background plus a placed photo) is
  protected with one session snapshot and recovers with it: its recovery set
  needs 2.5 GB when reopened. Each session snapshot is written and
  reopened as its own full copy, so two snapshots need 2.84 GB, within a
  16 GB Mac's limit. A 45 MP 8-bit `.drsnapshot` loads into an open 45 MP
  document: it needs 1.88 GB with the document. A recovered snapshot is its
  own copy and shares no memory with the document, so after recovering that
  document (867 MB retained) Save Snapshot and the next recovery checkpoint are
  admitted at the 8 GiB limit a 16 GB Mac gets (measured; the checkpoint plans
  1.22 GB, 4.86× its peak).
  Checkpoints, saves and exports are flushed to disk before they report
  success, so a completed one survives a power loss.
  Window > History lists image and selection edits, supports navigation and
  budgets retained memory instead of stopping at 24 edits. Choosing a state
  while a healing or patch job runs cancels the job instead of navigating. The nearest Undo
  state is retained with a visible warning if one edit alone exceeds the budget.
  Undo, Redo and choosing a History state always proceed:
  each step's display is estimated first, and a step that goes over the
  working limit still runs and shows "Over working limit" in the status bar,
  its tooltip naming the overage and the way out — close other documents,
  lower the history budget, or raise Doctored ▸ Working Memory… (applied
  after a restart); MCP `undo`, `redo` and `restore_history` return the same
  sentence as `memory_warning`.
  Window > Snapshots captures full editing states; Save/Load uses portable
  `.drsnapshot` packages. Ordinary `.doctored` files do not embed the session list;
  recovery copies include it, and normal document close ends the session.
  MCP mirrors the recovery, history and snapshot commands.

- **Working memory**: the working limit, the memory Doctored plans to use for
  images, undo, recovery and edits, scales with the Mac's RAM. The automatic
  limit is `min(RAM/2, max(RAM/3, RAM − 8 GiB))`: 2.67 GB on 8 GB, 4 GB on
  12 GB, 8 GB on 16 GB, 12 GB on 24 GB, 16 GB on 32 GB, 32 GB on 64 GB. It is
  at least a third of RAM and, from 16 GB up, leaves at least 8 GiB for macOS,
  the display and other apps (a third of RAM wins below that). Doctored ▸
  Working Memory… overrides it, from 2 GiB up to three quarters of RAM; the
  new limit takes effect the next time Doctored starts.
  `DR_WORKING_LIMIT_BYTES` overrides both for one run (test instances pin
  2147483648, so refusals in tests are the same on every machine). Help ▸
  Memory Usage and MCP `resource_usage` report the limit and where it came
  from ([ADR](https://github.com/kpgalligan/doctored/blob/main/designs/working-limit-ram-scaled.md)). The figures on this page
  are what a job needs; the assumed minimum is a 16 GB Mac and its 8 GiB limit,
  and work is not designed to fit smaller ones.

- **Background work and memory diagnostics**: final Content-Aware Fill, spot
  healing, the Healing Brush and Patch commits, every 16-bit paint and retouch
  stroke (Brush, Eraser, Dodge/Burn, Clone Stamp and both healing brushes, each
  ghosted on the canvas until its job ends), and final raster transforms
  (Free Transform, Distort, Warp, the 8-bit Crop straighten and filtered arbitrary
  rotation) have cooperative cancellation; save/export and RAW development also run on the
  bounded worker with responsive progress.
  Saves write one captured revision and keep newer edits dirty. Help > Memory
  Usage reports unique retained buffers across documents, undo, jobs and caches,
  plus scratch estimates and unaccounted platform memory. An 8-bit `.doctored` save
  or snapshot admits the core's save plan (the encoded-body bound, the tables
  held beside it, the largest plane's encode transient and fixed overhead),
  which does not depend on cached layer views. MCP offers
  `background: true` for fill/heal (`heal_stroke` at 8 and 16 bits), `job_status`,
  `cancel_job` and `resource_usage`.

- **Software updates** (Doctored ▸ Check for Updates…): a build downloaded from
  the Releases page updates itself through Sparkle, starting with the first
  release after 0.4.2 (0.4.1 and 0.4.2 have no updater, so replace those by
  hand once). On the second launch it asks whether to check automatically;
  when allowed it checks once a day. An update is installed when you choose
  Install Update, or without asking once you tick the alert's option to
  install updates automatically. Each update is the release's notarized DMG,
  and its EdDSA signature is checked against the key built into the app before
  anything is installed. Run the app from `/Applications`: a copy running from
  the disk image cannot update itself. A build made from source has no
  updater, and its menu item stays disabled.
  `defaults write com.kgalligan.Doctored SUEnableAutomaticChecks -bool NO`
  turns the scheduled check off. An instance the MCP helper opens waits until
  you first switch to it before it checks, and no MCP tool starts an update,
  since an install quits the app.

- **Layers**: full layer stack with per-layer opacity, visibility, offsets,
  and the full 27-mode blend set — the separable W3C modes plus
  Dissolve, Vivid/Linear/Pin Light, Hard Mix, Divide, Darker/Lighter Color,
  and the non-separable Hue/Saturation/Color/Luminosity — grouped in the
  panel's menu;
  layers panel with thumbnails, inline rename, drag-reorder, a right-click
  row menu (Rename, Group / Ungroup,
  Lock, Label, Layer Style… with Copy / Paste / Clear Layer Style, Delete Layer),
  and new/group/delete/duplicate/merge-down/flatten; Move tool (V) with
  arrow-key nudges, including exact 16-bit and 32-bit float integer moves;
  with a selection active, a Move drag or nudge moves the SELECTED PIXELS of
  the active layer at 8 and 16 bits: the pixels × the selection float,
  land source-over at the new position (off-canvas landing discarded, the
  layer mask stays put) and leave a transparent hole (there is no Background
  fill), and the selection moves with them — one "Move" undo step, previewed
  on a background worker while the marquee leads. Holding ⌥ at the press
  duplicates instead: no hole, the copy lands source-over and the
  selection moves with the copy — one "Duplicate" step (`move_selected_pixels`
  takes `duplicate`), and so does ⌥ with an arrow key, one "Duplicate" step
  per press; the Move tool shows a copy cursor while ⌥ is
  held. With no selection, ⌥-drag and ⌥+arrow duplicate the selected layers
  and move the copies, which land unlinked directly above their sources and
  become the selection — one "Duplicate Layer" step (`move_layers` takes
  `duplicate`); a position lock on an original, an enclosing group or a
  group's descendant refuses, a link partner's does not. The duplicate drag
  snaps to its source layer, never to its own copy, at 8 and 16 bits alike.
  A drag over no opaque selected pixel refuses on its first tick rather than
  at release, and once the float is measured the drag snaps on its opaque
  bounds; the 8-bit commit's short wait panel appears only if it lasts past
  0.25 s. Only one layer moves: a multi-layer panel selection refuses by name,
  links are ignored, consecutive drags and nudges over one selection move ONE
  float, so pixels it passes over come back (one undo step each, and
  Deselect, a selection or layer change, Save or any other edit commits it), Lock
  Pixels / Lock All refuse while Lock Transparency / Position allow it, and a
  32-bit float document moves the layer's f32 pixels as they are. With a
  LAYER MASK or ALPHA CHANNEL as the edit target, the drag and the nudges
  (⌥ duplicating) move that plane's selected area instead, at 8, 16 and
  32-bit float: the vacated
  area takes the background swatch as gray, the selection moves with it, and
  it is one "Move" or "Duplicate" step that lands exactly as a Free Transform
  translate of the plane would; with the RED, GREEN, BLUE or ALPHA plane as
  the target only that channel moves, at 8, 16 and 32-bit float, its vacated
  area taking the background swatch's 0.30/0.59/0.11 luma rounded to a code
  (Alpha's is transparent); Quick Mask refuses by name;
  Paste as New Layer; PSD imports offer the supported
  layers or saved composite after a compatibility report; the native `.doctored` format saves the full layer stack — masks,
  clipping flags, layer styles, the global light, alpha channels, groups,
  locks and links, and text and adjustment descriptions included —
  losslessly, and older `.doctored` and `.rz` files still load
- **Layer groups** (Layer > New Group, ⌘G; Ungroup ⇧⌘G): a group holds
  layers and nests arbitrarily, with its own name, visibility, opacity,
  blend mode, mask, layer style and clipping. A group is **Pass Through** by
  default — its children composite straight against the layers below it, so
  grouping alone never changes the picture and an adjustment layer inside a
  group still reaches the layers beneath it. Give the group any other blend
  mode or a style and it ISOLATES: its children render on their own and the
  result composites once, which is also what confines an adjustment layer to
  the group. A group's own **mask** and **opacity** never isolate it — they
  gate it per pixel instead, so masking a group of adjustment layers
  restricts their reach rather than switching them off, and a reveal-all
  mask changes nothing. (Clipping a layer to a pass-through group isolates
  it — a clip base has to have its own footprint — and the app says so when
  it happens.) Ungrouping says what it cost: a group's own mask, style,
  opacity, blend mode and clipping cannot be carried by its children, and a
  clipping mask on the group's bottom layer — which clipped to nothing
  inside it — is released rather than silently re-pointed at the layer
  below. A group's disclosure
  triangle is saved with the document and adds no undo step, from the
  panel or over MCP. With a group active, New Layer and New Adjustment Layer
  land the new entry as the group's top child. PSD groups import with their nesting
  and names, folder opacity/visibility/blend mode, and open/closed state.
- **Selecting several layers**: ⇧- and ⌘-click in the panel; move,
  transform, align, distribute, delete, duplicate, group and merge then act
  on the whole set, while the last row touched stays the single active layer
  every tool needs. **Link Layers** ties layers together so they move and
  transform as one whatever is selected; **Align** and **Distribute** work
  on each layer's opaque CONTENT bounds, against the selection or the canvas,
  at 8, 16 and 32-bit float
- **Duplicate to Document** (Layer > Duplicate to Document…): copies
  the selected layers, groups with everything inside them, into another open
  document (its Destination) or into a new
  untitled document at the source's depth (8, 16 or 32-bit float), canvas
  size, colour profile and resolution. Each layer keeps its pixels, offset,
  mask, style, blend settings, label, locks and text, shape or adjustment
  description; alpha channels, guides, layer comps and metadata stay with
  the source, which gets no undo step. A sheet names the new document
- **Layer colour labels and the panel filter** (Layer > Label ▸, or a row's
  Label ▸): mark a layer red, orange, yellow, green, blue, violet or gray —
  Photoshop's own palette and its own `lclr` codes, so a label travels to a
  layered PSD and back. The panel tints the visibility gutter of a marked row,
  and the filter bar above the stack narrows it by name substring, by kind
  (raster, adjustment, text, shape, group, embedded) and by label. The filter
  changes nothing in the document: while it is on, a match inside a CLOSED
  group gets a row of its own so it can be clicked, and clearing the filter
  puts the collapsed stack back. A label is exempt from the layer locks, like
  a rename — it marks a row and changes no pixel — a duplicate copies it, a
  merge keeps the bottom-most merged layer's, and a flatten starts unmarked.
  The native `.doctored` carries labels at every depth; a document with none saves
  exactly the bytes it always did
- **Layer comps** (Window > Layer Comps…): a NAMED recording of per-layer
  visibility, position and appearance — never pixels, the selection or the
  history, which is what a Snapshot holds. Each comp records its own subset:
  the three capture flags decide whether it moves visibility, position
  (raster layers only — a group's offset is derived from what is inside it)
  and appearance (opacity, blend mode and the layer style, Blend If
  included). Applying one is a single undo step, and so are Update, Options…
  (rename, comment, flags), Delete and dragging a comp to a new place in the
  list; ◀ ▶ cycle the list in that order. A comp names its
  layers by identity, so renaming a layer keeps the entry; deleting one makes
  that entry STALE — reported by the name it was recorded under rather than
  skipped — and a layer whose position is locked has its recorded move
  skipped and reported while its visibility and appearance still apply.
  Applying a comp hides every layer it has no record of — a
  layer added, duplicated, placed or pasted since — whatever the comp
  captured, but never an unrecorded group; Update records it, and the window
  (and the MCP reply's `hidden_unrecorded`) names the layers it hid. A
  comp follows the canvas: crops, Canvas Size, Image Size (which scales a
  recorded layer style too), rotations, flips and straightening remap every
  recorded layer position, so applying a comp afterwards puts each layer
  where the operation put it, re-rendered text and shape layers included.
  A comp also follows a layer that grows (paint, bucket fill or a gradient
  past its edge, a retouch with Expand destination or a selection moved past
  its edge at 8 and 16 bits, and the 8-bit Stroke…), so applying it puts the
  content back where it is, and a layer
  that Merge Down, Merge Layers or Merge Visible merges into at 8 and 16 bits:
  a comp saved before the merge restores the merged layer's position and
  leaves its appearance as merged.
  The native `.doctored` carries comps at every depth; a document with none saves
  exactly the bytes it always did
- **Layer opacity** is a continuous drag at every depth: the Layers panel's
  slider previews as it moves, the preview and the release are computed from
  the same captured target, and the whole gesture is ONE Undo step. On a
  16-bit or float document a gesture whose document changed under it (an
  agent edit landing mid-drag) publishes nothing and says so; an 8-bit
  gesture refuses the foreign edit instead, since it holds the document live
- **Layer locks** (Layer > Lock, Lock All ⌘/; a bare `/` on the canvas
  toggles Lock Transparency as one undo step): transparency freezes the
  layer's alpha — every pixel keeps its alpha exactly, so the shape cannot
  change; on a semi-transparent pixel paint mixes in at its own opacity ×
  coverage, not boosted by the kept alpha —
  an eraser cannot punch a hole, and Apply Layer Mask (which would bake the
  coverage into that alpha) is refused; pixels refuses every pixel edit while
  still letting the layer's mask be painted; position refuses moves and
  transforms (a transform is a position edit, so a transparency-locked layer
  still transforms — and so does a whole-document crop or straighten, which
  re-frames the picture rather than moving a layer inside it); all three
  together also freeze the mask. Merging answers to the DESTINATION layer's
  pixel and transparency locks, since a merge replaces its picture. A
  group's locks cover its contents: every layer inside a
  locked group answers to the group's locks as well as its own (a mask
  freezes when the layer or any enclosing group is Lock All). Every refusal
  names the lock that stopped it — and the group, when a group's lock did.
  The Layers panel shows a layer inside a locked group with a dimmed lock
  badge, whose tooltip names the group each inherited lock comes from.
  Locks cross PSD both ways at 8 and 16 bits as Photoshop's `lspf` (Lock All
  as Photoshop's complete lock; a Background's implicit locks and the
  artboard-nesting lock are not carried, a locked folder's lock lands on
  the group, where it covers the folder's contents, and a locked folder
  dissolved beyond the 10-level nesting limit adds its locks to each layer
  it held, which the import report discloses)
- **Workflow commands**: Layer Via Copy (⌘J) and Layer Via Cut (⇧⌘J) lift
  the selection into a new layer — on a group, an adjustment layer, a text /
  shape layer or with nothing selected, ⌘J duplicates instead,
  so a re-editable layer is never silently rasterized; Merge Visible, Stamp
  Visible (⇧⌥⌘E), and Arrange ▸ Bring to Front / Forward / Backward / To
  Back (⇧⌘] ⌘] ⌘[ ⇧⌘[), which move a layer among its siblings only. The Move
  tool's **Auto-Select** activates the layer — or its group — under the
  click at 8 bits, 16 bits and 32-bit float, and its **Show transform
  controls** option (off by default) draws a box and eight handles round the
  moving layers, a press on a handle starting Free Transform
- **Layer masks**: a grayscale coverage mask per layer that hides pixels
  without erasing them — Layer > Mask adds one revealing all, hiding all, or
  built from the current selection, then enables/disables it (a disabled mask
  is kept but ignored, to compare with and without), applies it (bakes the
  coverage into the layer's alpha) or deletes it (the layer comes back
  whole); a masked layer grows a second thumbnail beside its own in the
  layers panel — click either to aim the brush and eraser, which paint the
  mask white to reveal and black to hide. Masks have independent bounds and
  **Link with Content**, **Properties** (density and feather), and **Transform
  Mask** commands. A mask built from the selection (Layer > Mask, or Select
  and Mask's layer-mask output) is canvas-sized at every depth, never cropped
  to the layer, so the layer can later be moved or grown under it. Unlink to
  move content while the mask stays in place; mask-only
  transforms preserve layer pixels. Original coverage stays separate from density
  and feather. Feather uses canvas pixels and keeps its numeric radius through
  geometry. Raw coverage outside the stored rectangle is explicitly 0 or 255;
  feathering, independent transforms and native saves retain that background.
  Older files open with masks that are 0 outside their rectangle, linked,
  full-density and unfeathered. Inverting a layer
  mask inverts the whole mask: with no selection the stored samples and the
  coverage outside them both flip, and with one a raster layer's mask grows to
  the canvas first
- **Paths and vector masks**:
  the **Pen tool** (P, its own rail slot) draws a Bézier path — click for a
  corner point, drag for a smooth one, click the first point to close, Esc or
  Return to end; ⌘-drag moves an anchor or a handle, ⌥-click converts a point,
  a click on a segment adds a point and a click on an anchor deletes it, each
  gesture one undo step. A document keeps one work path and named saved paths
  (up to 256), listed in the **Paths panel**, a tab of the right panel, with
  rename (which saves the work path), duplicate and delete. Make Selection…
  turns a path into a selection (feather, anti-alias, new / add / subtract /
  intersect), Fill Path fills it with the foreground colour, Stroke Path runs
  the Brush along it with the Brush's stored options, and Select ▸ Make Work
  Path… traces the selection into a path within a tolerance of 0.5 to 10 px.
  **Layer ▸ Vector Mask** (Current Path, Reveal All, Delete, Enable, Link)
  masks a layer with a path: it combines with the layer's raster mask,
  re-edits with the Pen and follows a move and, while it has no raster mask
  beside it, a linked Free Transform, the canvas quarter turns and flips and
  Image Size; a warp, a perspective transform, a straighten, an arbitrary
  canvas rotation, a duplicate, a paste or a merge leaves an ordinary raster
  mask with the same picture. Paths follow crop, Canvas Size, Image Size, rotation
  and Perspective Crop, edit without redrawing the picture, work at 8, 16 and
  32 bits and are saved in `.doctored` (a flat save names them among its losses).
  Thirteen MCP tools mirror them (`list_paths`, `set_path`, `rename_path`,
  `duplicate_path`, `delete_path`, `path_to_selection`, `selection_to_path`,
  `fill_path`, `stroke_path`, `get_vector_mask`, `set_vector_mask`,
  `set_vector_mask_properties`, `remove_vector_mask`) and Actions record them.
  A layered PSD, PSB or TIFF carries the paths both ways at every depth, and
  at 8 and 16 bits RGB a vector mask with no raster mask beside it as its path;
  every other export writes the baked mask. Not built: the Curvature, Freeform
  and Magnetic pens, type on a path, path boolean operations, Convert Path to
  Shape, clipping paths, and paths in a flat TIFF
  ([record](https://github.com/kpgalligan/doctored/blob/main/designs/v1-paths.md))
- **Fill layers** (Layer > New Fill Layer ▸ Solid Color… / Gradient… /
  Pattern…): a re-editable layer that is one colour, one gradient or one
  pattern over the whole canvas, at 8, 16 and 32 bits (`add_fill_layer` /
  `edit_fill_layer`, kind `pattern` for a pattern). A pattern fill carries
  the Pattern Overlay's own pattern settings (a preset or a custom tile,
  Scale, Angle, Link with Layer, Snap to Origin) and renders through the same
  pattern sampler; linked, it follows the layer's reference point. Layer ▸
  Layer Content Options… or a double-click re-opens it. It is a description
  like text and shape, saved in `.doctored`, and it always covers the canvas: crop,
  trim, Canvas Size, Reveal All, rotate, flip and Image Size re-render it to
  the new canvas, and a move or transform of one is refused by name. With a
  selection, the menu command adds the layer's mask from it in the same undo
  step. A gradient fill takes Smoothness and Align with Layer, and each colour
  and opacity stop a midpoint (5–95 %), which the shared gradient editor draws
  as a diamond to drag, with a Midpoint field; Gradient
  Overlay and Stroke Gradient take the midpoints too. A Linear, Radial,
  Angle or Reflected gradient fill holds its Smoothness curve between each
  segment's two stops (measured against Photoshop); the Gradient
  tool, Gradient Overlay and Stroke Gradient are not measured there and keep
  the unclamped curve. A PSD export writes a
  solid, gradient or pattern fill as Photoshop's own `SoCo`, `GdFl` or `PtFl`
  record. In a CMYK document a pattern
  fill's tile separates to ink as it renders
- **Selection workflows**: Freehand Lasso closes a dragged outline; the
  **Magnetic Lasso** (L, cycling Polygonal → Freehand → Magnetic) traces a live
  wire along the strongest edge near the pointer as you hover, fastening anchors
  by itself (Width, Contrast and Frequency; `[` / `]` step the
  Width), click to fasten one, ⌥-click for a straight segment, ⌫ to remove the
  last anchor, and double-click, Return or a click on the start to close (⌥ to
  close straight); it works at 8, 16 and 32 bits, and MCP `select_magnetic` drives
  the same session and records the resolved `select_freehand` outline
  (⌥-drag freehand segments, the Caps Lock width cursor, the contrast keys
  and stylus pressure are deferred); **Color
  Range** samples document-profile colors (Localized Color Clusters weights
  each by distance from where it was taken), or selects a hue preset (Reds …
  Magentas), Skin Tones with a Fuzziness, Out Of Gamut (what View ▸ Gamut
  Warning flags) or a tonal range, each with Invert, at 8, 16 and 32 bits and
  on a CMYK document's view (Skin Tones is Doctored's own band, ΔE76 within 18
  of a skin colour and then a soft edge 0.25 × Fuzziness wide, with no
  agreement with Photoshop claimed, and Detect Faces is not built); **Transform
  Selection** moves, scales and rotates coverage independently of layer pixels.
  **Quick Selection** paints with a sized brush: everything under the stroke is
  selected and grows outward through similar colour, stops at edges, fills the
  small gaps a stroke encloses and can refine its edge (Enhance Edge); a plain
  stroke adds to the selection and ⌥ subtracts. **Object Selection** turns a
  dragged rectangle or lasso into the outline of the object inside it (Vision's
  foreground instances, clipped to the drag and refined against the picture's
  edges; MCP `object_select`); with **Object Finder** checked it prompts a
  local SAM 2.1 model with a click or a dragged box instead, a single click
  selecting the whole object and Shift-click adding or ⌥-click excluding a
  point, each one undo step (MCP `object_finder_select`). **Select
  Objects** picks multiple Vision foreground instances by thumbnail, point or
  region; **Select People…** picks individual people the same way, numbered
  left to right, and **All People** uses dedicated aggregate person coverage;
  both refuse an image where no person (body or face) is detected. **Face
  Parts…** selects the face, skin, eyes, irises, eyebrows, lips or mouth of
  one face (click it on the canvas) or every face Vision finds, as smooth
  anti-aliased regions through its landmarks, with a feather (MCP
  `inspect_faces` and `select_face_parts`). **Select ▸ Sky** selects the sky
  from a local sky model (U²-Net) and refuses an image with none, and
  **Select ▸ Person Parts…** selects the hair, facial skin, body skin or
  clothes of each person (MediaPipe's multiclass segmenter), at 8, 16 and 32
  bits (MCP `select_sky`, `select_person_parts`; the models download from
  Settings ▸ Models, see **Depth Blur**). These share
  selection combination modes and one clean Undo step.
- **Select and Mask** (Select menu; MCP `refine_selection`): inspect before/after
  at 100% over checkerboard, white, black, gray or the mask itself. Edge
  Detection re-estimates coverage from the picture's own colours within a Radius
  of the selection's edge, Smart Radius keeps a hard edge where the picture has
  none, and the Refine Edge brush adds or removes areas to refine; then Smooth,
  Feather, Shift and Contrast. Output to the selection, a new/replacement layer
  mask, a New Layer, or a New Layer with Layer Mask (the other layers are hidden,
  not changed); Decontaminate Colors replaces fringe colour with the estimated
  foreground colour on a layer output from a fully opaque composite. All of it
  runs at 8 bits and natively at 16 (65,536 coverage levels; RGBA16 layers and
  u16 masks) and at 32-bit float (all but Decontaminate Colors), and the sheet's preview is an explicit 8-bit display of the
  coverage. It is colour-guided, not learned matting: hair against a busy
  background, whiskers and glass stay approximate and may need manual coverage
  work.
- **Channels** (View > Channels, ⌃⌘C): one of the right panel's six tabs
  (Layers, Channels, Assistant, Info, Navigator and Paths), listing RGB, the composite's Red, Green and Blue
  planes (a CMYK document lists CMYK, Cyan, Magenta, Yellow and Black, to
  view), the active layer's mask while it has one, and every **alpha
  channel** the document carries — named canvas-sized coverage planes that
  are saved selections, never part of the picture. Each row draws its own
  grayscale thumbnail and carries an eye: RGB shows the colour image, a
  single colour plane's eye shows that plane in grayscale, and a channel's
  eye washes it over the picture as a rubylith in its own colour, opacity
  and polarity (Color Indicates: Masked Areas by default — the same side
  Quick Mask washes). Selecting a row *is* the edit target: with Red
  selected the brush, eraser, fill, gradient and every destructive
  Filters/Adjustments item run on that document-depth scalar plane of the active layer
  (previewing in grayscale and committing what they previewed), leaving the
  other planes' samples unchanged; with an alpha channel selected they paint the
  channel — white selects, black deselects — as one undo step each. A selected
  layer mask also supports filters, adjustments, fills and gradients, preserving
  the layer's pixels and description. Unsupported targets and locks refuse the
  operation. Rows
  rename by double-click, and a right-click menu (mirrored under Image >
  Channels) duplicates, deletes, inverts, opens Channel Options… for the
  name and rubylith, or loads the channel as a selection. Selections move
  both ways: Select > Save Selection… writes one into a new or existing
  channel, Select > Load Selection… reads a channel, a layer's transparency,
  a layer's mask or a colour plane back out, and **⌘-clicking** a layer
  thumbnail, a mask thumbnail or a channel row does it in one gesture
  (Shift adds, Option subtracts, Shift+Option intersects — the selection
  tools' own convention). At 8 and 16 bits a load from the composite's
  luminosity follows the Gray working space: Edit ▸
  Color Settings… has a Gray popup (Dot Gain 10 to 30 %, Gray Gamma 1.8 or
  2.2; Dot Gain 20 % by default). Image > Apply Image… blends one source onto the
  current target — onto the layer the three planes blend as one colour, so
  the whole mode set applies; onto a single plane or a channel the four HSL
  modes (Hue, Saturation, Color, Luminosity), which need an RGB triple to
  mean anything, are left out, as they are in Image > Calculations…, whose
  result is always one plane. Calculations blends two source planes into a
  new channel or a selection, and Select >
  Add Luminosity Masks appends the photographer's nine tone masks
  ("Lights 1".."Midtones 3") built from the composite's Rec. 709 luma.
  Opening an iPhone photo (HEIC or a "Most Compatible" JPEG) brings its
  **auxiliary mattes** in as channels — Depth, Portrait Matte, Skin, Hair,
  Teeth, Glasses, Sky — resampled to the canvas and ready to load as a
  selection (Select ▸ Portrait Matte loads one in a click), including an
  imported Live Photo's original still. Native auxiliary
  channels retain declared source coverage precision and orientation metadata.
  A file carrying none adds none. Typed still and RAW imports retain these
  scalar channels; movie frame selection keeps its decoded alpha and does not
  infer still-photo mattes for a different frame. Channels ride along with every
  canvas-geometry op (crop crops them, Canvas Size pads them, rotate/flip
  permute them, Image Size resamples them — a growth that would push the
  channel list past what `.doctored` can store is refused there, with an alert
  naming how many channels that canvas holds, rather than at save time) and
  are saved in `.doctored`.
  **Spot channels** ([#1](https://github.com/kpgalligan/doctored/issues/1),
  [the ADR](https://github.com/kpgalligan/doctored/blob/main/designs/spot-channels.md)): Image ▸ Channels ▸ New Spot
  Channel… adds a channel with an ink (a colour from the colour picker, kept
  as Lab) and a Solidity of 0–100 %, Spot Channel Options… edits them, and the
  channel paints like an alpha channel. RGB, CMYK, Grayscale, Lab and Duotone
  documents take them; Indexed Color and Bitmap refuse by name. The composite
  view overprints each visible spot channel's ink at its coverage (Multiply
  at 0 % Solidity, the plain ink at 100 %), leaving the layers unchanged.
  Merge Spot Channel flattens the document, bakes the ink in and deletes the
  channel as one undo step (a Duotone document refuses); Channel Options'
  Spot Color turns an alpha channel into a spot channel and Spot Channel
  Options' Kind turns it back. Spot channels round-trip through `.doctored`, and an
  RGB, CMYK, Grayscale or Duotone PSD or PSB at 8 or 16 bits opens with its
  spot channels and saves them in the layout Photoshop-authored files showed
  (a 16-bit Duotone document's file is written at 8 bits, the depth
  Photoshop's Duotone has, with one loss line in the export report and the
  document left at 16 bits), and Photoshop 26.10 opens those files with every
  spot channel's name, ink, Solidity and coverage as written; an RGB or CMYK
  TIFF at 8 or 16 bits, flat or layered, opens with its spot channels and a
  TIFF save or layered TIFF export keeps them in Photoshop's layout (and
  Photoshop 26.10 opens those files with them too), while a Lab or 32-bit PSD
  or PSB and a Grayscale, Duotone, Lab or 32-bit TIFF refuse a document
  holding them, by name, and a PDF refuses them: Doctored's PDF cannot write
  spot channels as `/Separation` inks.
  Every other flattened save or export composites the
  overprint and says so; a CMYK export overprints as ink on paper, through
  the document's CMYK profile. The overprint, the merge and the kind change
  are not measured against Photoshop
- **Adjustment layers**: non-destructive color adjustments that live in the
  layer stack and recolor everything below them at composite time, their
  parameters editable forever. Layer > New Adjustment Layer offers
  twenty-two ops — Brightness/Contrast/Saturation (a modern curve that keeps
  black and white fixed, or the earlier linear form under Use Legacy, which
  every existing layer, Action and file keeps), Levels (input black, white
  and gamma plus output black and white, with a channel popup switching
  between the composite RGB stage and Red/Green/Blue stages, each channel
  applied before the composite, and Auto Tone / Auto Color
  buttons that fill the channel stages from what the layer sits over; a
  gamma above 1 takes Color Balance's toe, and at 8 bits each stage rounds
  on an integer path, confirmed on a fresh Photoshop take at 8 and 16 bits,
  [#2](https://github.com/kpgalligan/doctored/issues/2)),
  Curves (an
  interactive editor drawing a natural cubic spline: click the
  curve to add up to 16 control points per channel, drag to move them, with a channel popup switching between the
  master RGB curve and Red/Green/Blue individually; Points | Pencil switches
  every channel to a drawn map you paint with the pencil, with Smooth, and back
  to control points, and a curve drawn with Photoshop's pencil opens as its
  drawn map), Exposure (in linear
  light, with a Gamma Correction that brightens above 1), Vibrance
  (in the document's linear light; raising it lifts the least saturated pixels most, the lift damped on red
  and skin hues, and lowering it mutes every hue evenly,
  with a Saturation slider measured against Photoshop on the document's linear
  values), Hue/Saturation (master plus six editable hue bands, and Colorize;
  the master's rule, measured against Photoshop, is shared by Replace
  Color and Desaturate),
  Color Balance (shadows/midtones/highlights, measured against Photoshop at
  8, 16 and 32 bits),
  Black & White (six hue weights and an optional tint, which at 8 bits takes
  an integer path, within 1 code of a Photoshop take,
  [#2](https://github.com/kpgalligan/doctored/issues/2)), Photo Filter (the
  standard warming and cooling filters, or any colour, at a chosen density;
  at 8 and 16 bits it multiplies in the profile's D50 connection space and
  Preserve Luminosity is a Luminosity blend, measured against Photoshop),
  Channel Mixer (a 3×4 matrix with a monochrome mode, its coefficients
  truncated to 1/1024, measured against Photoshop), Selective Color (nine
  ranges of CMYK nudges, relative or absolute), Shadows/Highlights (whose
  tone estimate is an alpha-weighted large-radius blur of the luma, so local
  contrast survives and a cut-out gets no halo), White Balance (temperature
  and tint, Bradford-adapted to D65), Gradient Map (luma through a multi-stop
  gradient you edit in place — drag a stop, click the ramp to add one, ⌫ to
  remove — with dithering that stops an 8-bit map banding a smooth sky; its
  Method popup offers Classic, with Smoothness and stop
  midpoints, and Perceptual (Oklab; a CMYK document refuses it and keeps
  Classic), each matching Photoshop takes
  at 8 and 16 bits and running at 32 bits too, while Linear and Smooth fit
  no rule tried and are not offered),
  Color Lookup (a .cube LUT, 1D or 3D, parsed in the core and applied with
  tetrahedral interpolation (measured against Photoshop), at a chosen
  strength; a table larger than this
  build stores is resampled down and says what size it came from — or an
  ICC look, `.icc`/`.icm`: an RGB-to-RGB device link applied to the
  document's numbers, or a Lab-to-Lab abstract profile baked through the
  document's profile when it loads and re-baked whenever that profile
  changes — Assign, a keep-parameters Convert or a Don't Convert paste; a
  link whose table is a plain grid of up to 33 nodes is stored at its own
  grid (any other link is sampled to 33³) and an abstract is sampled into a
  33³ table; every
  other profile class is refused by name), Replace Color (hue, saturation and
  lightness shifts weighted by each pixel's distance to one to eight sampled
  colours, full inside half the fuzziness and fading to nothing at it; 8, 16
  and 32 bits), Hue
  Rotate, Posterize (at 8 bits in exact agreement with Photoshop's levels; a
  fresh Photoshop take confirmed it at 8 and 16 bits), Threshold,
  Invert, Grayscale, and Sepia. The
  parameterized ops open live-preview dialogs and re-open any time via
  Layer > Adjustment Options… or a double-click on the layer's row in the
  panel (which badges adjustment layers "◐"). Layer > New Adjustment Layer >
  From Photoshop Preset… opens a Photoshop `.acv` Curves, `.alv` Levels,
  `.ahu` Hue/Saturation, `.eap` Exposure, `.blw` Black & White or `.cha`
  Channel Mixer preset — starting in an installed Photoshop's own Presets
  folder — in that op's dialog, prefilled (a Levels preset with per-channel
  or output levels carries them); a CMYK Curves or Levels preset opens on a
  CMYK document's inks, a CMYK Channel Mixer preset (and a CMYK Curves one
  on an RGB document) is refused by name, and
  `read_photoshop_adjustment_preset` answers the same values over MCP. Curves, Levels
  and the thirteen parameter dialogs carry a Preset popup over a user library
  of adjustment presets (Default, the op's saved presets and Custom, with
  Save, Rename and Delete; `list_adjustment_presets`,
  `save_adjustment_preset`, `rename_adjustment_preset` and
  `delete_adjustment_preset` over MCP). Curves and Hue/Saturation have a
  Targeted Adjustment checkbox: a press on the image samples the adjustment's
  input under the cursor, and a drag moves that curve point or colour range.
  Every adjustment layer is
  created with a layer mask gating where the adjustment applies — built from
  the selection when one exists, else revealing all — and brush and eraser
  strokes on the layer paint that mask automatically. It lands directly
  above the active layer, and `add_adjustment_layer` can put it anywhere
  instead: `position: "top"` at the top of the stack, `position:
  "into_group"` as the topmost child of a group named by `layer`. A new
  layer JOINS a clipping run rather than splitting it — when the entry above
  the slot is clipped, the new one is created clipped too, so no neighbour
  is re-based onto it — and all of it works identically at 8 bits, 16 bits
  and 32-bit float. Because it is just a
  layer, opacity, blend mode and clipping all apply; the thirteen newer ops'
  destructive Image > Adjustments twin runs the identical core op and lands
  on the identical bytes, so the only difference is reversibility. Three
  stated exceptions: Shadows/Highlights is the only op that reads a
  neighbourhood, so the layer reads the backdrop below it and the filter
  reads its own layer — the two agree on the same pixels and are
  deliberately different pictures on different ones; Gradient Map's dither
  is keyed on the pixel's position, and the layer counts that from the
  canvas while the filter counts it from the layer it was handed, so on a
  layer whose offset is not (0, 0) the same jitter falls on different pixels
  (turn `dither` off and they agree exactly at any offset); and the nine
  older adjustments keep the single-purpose filters that predate the shared
  twin, which do their arithmetic in 0-255 rather than 0-1 and so can land
  one step away on a value falling exactly between two codes.
  New RGB Exposure and White Balance use the document's actual supported
  ICC matrix/TRC profile. Their destructive and layer paths share the same
  kernels and store explicit `document_v2` evaluation. Existing descriptions
  and older Actions keep their prior appearance; scalar targets retain
  profile-independent grayscale evaluation. Exposure runs per channel at the
  profile's effective pure gamma (2.2 for sRGB-shaped and sampled curves, a
  pure power's own exponent) with no tone encoder, within about 2 codes of
  Photoshop ([#2](https://github.com/kpgalligan/doctored/issues/2)).
  Calibrated processing refuses unsupported profiles rather than
  substituting sRGB: a LUT-only profile refuses a new or re-edited
  `document_v2` Exposure/White Balance at every depth with the core's
  sentence, and a matrix/TRC one whose tone curves fail the precision check
  refuses White Balance the same way, while Exposure is admitted there
  wherever the curves give it an exponent per channel (ROMM RGB runs at its
  own 1.8 and ITU-709 at 1.883; a degenerate curve still refuses). An existing 8-bit layer under a matrix/TRC profile whose
  curves fail the check keeps rendering; one under a LUT-only profile still
  does not render, as before.
  Adjustment Options… on such a refused layer offers Switch to Legacy sRGB
  Evaluation…, which reopens its settings with the original sRGB processing:
  the look change previews on the canvas and Apply records it as one undo step.
- **Clipping masks** (Layer > Create Clipping Mask, ⌥⌘G): confine a layer
  to the alpha footprint of the first unclipped layer beneath it — the
  base's blend mode and opacity apply to the group as one unit,
  consecutive clipped layers all ride the same base,
  hiding the base hides its group, and reordering simply re-derives the
  groups. The layers panel indents a clipped layer behind a "↳" arrow;
  releasing (the same menu item, retitled) undoes it, pixels untouched
  either way
- **Layer styles** (Layer > Layer Style ▸): an effect stack per
  layer — Drop Shadow, Inner Shadow, Outer Glow, Inner Glow (each filled with
  a colour, or a gradient with Jitter), Stroke (outside
  / inside / center, filled with a color, a gradient — Shape Burst included —
  or a pattern; an inside band replaces the layer under it, as Photoshop
  takes confirmed), Color Overlay, Gradient Overlay, Pattern Overlay (Pattern, Scale,
  Angle, Link with Layer and Snap to Origin; at 8, 16 and 32 bits and on
  CMYK), Bevel & Emboss (with a Texture: Pattern, Scale, Depth, Invert, Link
  with Layer and Snap to Origin) and Satin, each with its own blend mode and
  opacity (a linked pattern's reference point moves with Image Size, Canvas
  Size, Crop, Free Transform, the turns, flips and arbitrary rotation (measured
  against Photoshop), and a quarter turn or flip gives it a turned copy of its
  tile), plus Blending Options (fill opacity, which scales the pixels but not
  the effects, and Blend If's split-slider ramps on this layer and the
  underlying composite, on Gray and on any of the colour channels at once,
  their weights multiplied, its Gray reading 0.30 R + 0.59 G + 0.11 B, as
  a Photoshop take confirmed) — rendered from the layer's shape (alpha × mask) at
  composite time, so they follow every move, mask edit and re-rendered
  text, live-previewed in one sheet with a checklist down the left and a
  pane per effect, copied/pasted/cleared from the same menu, badged "fx" in
  the layers panel (double-click a plain raster layer's row to open the
  sheet), scaled with Free Transform and Image Size, baked by Merge Down and
  Flatten, and saved losslessly in `.doctored`. A document-level global light (angle, altitude) drives every
  effect with Use Global Light on. The Free Transform preview shows the
  layer without its effects until commit. 16-bit documents carry the same
  styles on raster layers, adjustment layers and groups (the effects render from the
  layer's shape quantized to the byte plane under the document's global light,
  saved in `.doctored`; the sheet previews live, at reduced resolution above 4 MP).
  Dither works on the Stroke's gradient and the Gradient Overlay; the
  Contour picker (ten presets or a custom curve, with Range) shapes the
  shadows, the glows, Satin and the bevel's Gloss and Contour; and Noise
  grains the shadows and glows. A layer stacks up to 10 instances each of
  Drop Shadow, Inner Shadow, Stroke, Color Overlay and Gradient Overlay, a
  later instance of a kind drawing over an earlier one. Below 32
  bits a layer's Opacity and its style's Fill composite at an
  8-bit byte; the soft Drop Shadow,
  Inner Shadow and Outer Glow follow the box-pair law measured against
  Photoshop (the Inner Glow's), and the Outer Glow composites over an
  anti-aliased edge in agreement with Photoshop (at 8, 16 and 32 bits and on
  CMYK, confirmed on a fresh Photoshop take at 8 and 16 bits); the Satin's
  matte follows the same
  box-pair law with its contour applied to each shifted copy before the two
  combine, a contoured Inner Shadow reads its contour on the unrounded matte,
  and a Drop Shadow with Layer Knocks Out on lands under the layer's partially
  transparent pixels (each confirmed on a fresh Photoshop
  take at 8 and 16 bits); and an
  anti-aliased Outside stroke at 100 % Fill composites in agreement with
  Photoshop and an Inside stroke on an anti-aliased shape takes the inner
  half of the Center stroke's band (at 8, 16 and 32 bits and on CMYK,
  confirmed on a fresh Photoshop take at 8 and 16 bits), each checked against
  Photoshop takes
  ([#2](https://github.com/kpgalligan/doctored/issues/2)). At 8, 16 and
  32 bits and on CMYK the Inner Glow follows the law measured against
  Photoshop below Choke 100 %, and at Choke 100 % on its Edge and, below Size
  24, its Center; Smooth Inner and Outer Bevel, Emboss, Pillow Emboss and
  Chisel Hard and Chisel Soft Pillow Emboss follow a measured bevel law, and
  a Smooth Outer Bevel lit from 45° or higher cuts its outside shading at a
  measured reach (confirmed on a fresh Photoshop take at 8 and 16 bits); the
  other Chisel bevels keep their own kernel
  ([#2](https://github.com/kpgalligan/doctored/issues/2)). A colour
  picked in Display P3 or ROMM, or between byte codes at 16 bits, keeps its
  value in the style, in `.doctored` and in PSD. Styles and Blend If also work on
  adjustment layers — the effects take the layer's mask as their shape and
  the adjustment applies after the layer's own Below effects, as a Photoshop
  take measured — and on Pass Through groups, which then composite
  isolated, at 8, 16 and 32 bits and on CMYK.
  **Window ▸ Styles** shows style presets as a grid: a click applies one to
  the selected layers and Shift-click adds its effects to theirs, New Style…
  saves the current layer's style, presets rename, delete and sort into
  groups, twelve built-ins ship, and Import Styles… / Export Styles… read and
  write Photoshop `.asl` libraries, the import naming what Doctored does not
  model (MCP `list_style_presets`, `apply_style_preset`, `save_style_preset`,
  `rename_style_preset`, `delete_style_preset`, `import_style_presets` and
  `export_style_presets`; Actions record applying one)
- Open PNG, JPEG, Photoshop (PSD and Large Document PSB, layered), TIFF (including BigTIFF, a
  TIFF with Photoshop layers as its layers, and a multipage TIFF's pages as
  layers, or page 1 alone when it opens as an 8- or 16-bit Grayscale, Lab
  Color, Indexed Color, Bitmap or CMYK document), BMP, GIF, WebP, HEIC/HEIF, AVIF,
  JPEG 2000, JPEG XL, OpenEXR and Radiance `.hdr` (each as a 32-bit float
  document), Apple Live Photos, supported SDR H.264/HEVC/ProRes 4444
  movies, HLG and PQ HEVC movies (as a 32-bit float document), and camera RAW (CR3, CR2, NEF, ARW, DNG, Apple ProRAW,
  ORF, RW2, RAF, SRW, PEF) — EXIF orientation is applied on open, so camera
  photos display upright. A GRAYSCALE PNG, TIFF or JPEG (8 or 16 bits, no
  profile or a Gray ICC) opens as a Grayscale document: its
  codes are kept exactly, an embedded Gray ICC is kept, a PNG `sRGB` chunk is
  sGray and a file with no profile takes the working Gray; a one-channel float
  TIFF with a Gray ICC opens as a 32-bit Grayscale document. A palette PNG,
  a single-image GIF, a palette TIFF or an Indexed Color PSD or PSB opens as an Indexed Color document
  with its palette and every index kept, and a 1-bit black-and-white PNG,
  TIFF or BMP as a Bitmap document (#1). A PDF page or an SVG is RASTERIZED on the way in at a page
  and resolution you choose. HEIC (48 MP 8-bit photos open: a 48 MP AVIF and
  the 36 MP system HEICs need less than 2 GiB, a 48 MP HEIC 2.03 GB; a single
  8-bit still's whole open, its first projection included, is admitted up
  front at 1.25× or more of its measured peak, so a 96 MP (12000×8000) 8-bit
  HEIC, gain-map HEIC or AVIF, which needs 3.77–3.91 GB, opens within a 16 GB
  Mac's 8 GiB limit;
  a plain 8-bit JPEG or PNG's whole open is charged up front the same way
  (34 bytes a pixel plus three times the file, 1.26× or more of its measured
  peak), so a 96 MP one (3.16 GB JPEG, 3.61 GB PNG) opens there too;
  a 10-bit HEIC or AVIF or a 16-bit PNG opened at 16 bits is charged from
  measurements the same way up to 100 MP (12500×8000), the document limit
  at either depth;
  every other 8-bit open — TIFF, WebP, GIF, BMP, a layered or composite PSD,
  `.doctored`, a 16-bit PNG opened at 8 bits, and a file only the platform decoder
  reads — admits its first display after the decode and before the document
  appears (8 bytes a pixel plus the display's own charge, and 4 more for a
  flat file, which may be converted to the working space; 1.36× or more of
  its measured peak),
  though the decode itself is still not planned up front, so a 48 MP TIFF
  needs 2.29 GB on its own and a 45 MP two-layer `.doctored` or PSD 2.14 GB, both
  well within a 16 GB Mac's limit;
  an HDR photo — a gain-map HEIC or AVIF, iPhone photos included, or a PQ or HLG
  HEIC, AVIF, PNG or JPEG XL — opens as a 32-bit float document in linear light,
  1.0 being the SDR base's white or 203 cd/m², unless File ▸ Open HDR Photos as
  HDR is off (`open_document`'s `hdr_photo: "sdr"`), when a gain-map photo opens
  as its SDR base, disclosed, and a PQ or HLG still, which has no SDR
  rendition, is refused by name), AVIF, JPEG XL and JPEG
  2000 open at their actual decoded depth; a 12-bit file opens at 16 bits at
  full scale (white 65535) — an identity-matrix AVIF, which macOS decodes
  left-aligned, is widened by bit replication — except a
  12-bit 4:4:4 HEIC, which is still refused by a sentence naming the cause —
  macOS's decoder returns no pixels for it. A premultiplied-alpha AVIF opens
  with its alpha, its colour divided back to straight on load. A still too
  large for one image at its depth is refused before it is decoded
- Export a copy to PNG, JPEG (with quality control), TIFF, BMP, GIF, WebP,
  HEIC (through the export workspace; the system encoder writes it, and the
  result reports the depth — 8 or 10 bits — and the packets it actually
  wrote), AVIF (written by the system encoder the same way, from an 8- or
  16-bit document), lossless JPEG XL (8-bit sRGB pixels only; anything else
  refuses by name),
  OpenEXR (32-bit or half float), Radiance `.hdr` or a half-float TIFF from a
  32-bit float document,
  an HDR HEIC from a 32-bit float document (the export workspace's HDR encoding,
  `export_image`'s `hdr_encoding`: a colour gain map over an SDR base, or PQ;
  an HDR AVIF is refused by name, since nothing on macOS writes one),
  a single-page PDF at the document's own print size
  (File > Export as PDF…; an 8- or 16-bit image, ZIP or 8-bit JPEG — a 16-bit
  or 32-bit float document can write a 16-bit page), or
  a layered Photoshop document — a PSD, a Large Document PSB or a TIFF with
  layers — at the document's own depth, 8-bit RGB from an
  8-bit document, 16-bit RGB from a 16-bit one and a 32-bit RGB PSD, PSB or
  layered TIFF in Photoshop's own form from a 32-bit float one, and from a Grayscale document a
  Grayscale PSD, PSB or layered TIFF at 8 or 16 bits (Photoshop opened all six
  test files and matched every layer within its declared bound) or a 32-bit
  Grayscale PSD or PSB (File > Export as PSD…;
  the 32-bit RGB PSD, PSB and layered TIFF open in Photoshop, every layer
  sample within the declared bound, and carry a Solid Color fill layer and
  layer styles in Photoshop's own records, which Photoshop opens with the
  fill and Drop Shadow intact; the 32-bit Grayscale files have not yet
  been opened there; at 32 bits a Grayscale layered TIFF, and in the layered
  mode a gradient or pattern fill layer, a style that would bake or an
  adjustment with no Photoshop record, is refused by name);
  failed saves never truncate or delete an existing destination file
- **Export workspace** (File > Export…, ⌥⇧⌘W): output dimensions (width, height,
  long edge or percent, aspect always kept), a destination profile (the
  document's, sRGB, Display P3 or an installed RGB profile it can convert into —
  a matrix/TRC profile whose tone curves fail the precision check is not
  offered unless its bytes are the document's own profile, and every export
  into one — the sheet, Quick Export, Export Layers, contact sheets, batch
  runs and the MCP export tools — is refused by the core before encoding with
  one sentence naming it, except into the document's own bytes under a
  non-absolute intent with BPC off, which exports unchanged (a batch run
  decides this per file as it goes, so a preset into such a profile fails the
  row of each file not already in it); "Document profile" never converts, so
  it always exports, and the sheet dims its rendering intent and black point
  compensation controls (the MCP export tools ignore both there); a document
  tagged with such a profile
  converts and exports OUT of it, its curves read through the parser's
  repairs), optional
  output sharpening, and a preview
  rendered from the ACTUAL encoded bytes with the exact file size beside it —
  the bytes previewed are the bytes written. Naming/destination presets store
  all of it plus a pattern (`{name}`, `{width}`, `{height}`, `{format}`,
  `{date}`, `{preset}`) — a preset naming a profile file that cannot be
  exported into is refused when it is saved, with the same sentence (a
  preset has no document, so the exception for the document's own profile
  does not apply to it) — and
  File > Quick Export (⌥⌘E) writes the default
  preset with no dialog, reporting the path; with no default set it writes a
  PNG named after the document beside its file (in `~/Downloads` for a
  document with no file), and a taken name gets `-2`, `-3`, … instead of a
  refusal. Quick Export and File > Export as
  PDF… encode off the main thread with a progress bar and Cancel (Esc). An
  export never overwrites a file, never creates a folder and never changes the
  master
- **Metadata policies**: a named, bounded library saying what each export
  keeps, packet by packet — EXIF, XMP, IPTC and the container's own print
  resolution are each keep, drop, or the default (which follows "Strip capture
  metadata") — plus an optional author/copyright stamp written into the XMP
  packet as `dc:creator`/`dc:rights`. The export notice names every packet,
  written or not. Orientation is never a choice: the camera's rotation is baked
  into the pixels at open, so every packet written states Orientation 1, and
  dropping the resolution needs EXIF and XMP dropped too, because each carries
  its own copy of the ppi
- **File Info** (File > File Info…, ⌥⇧⌘I): read and edit Author, Copyright
  Notice, Description and Keywords at every depth, as one undo step. They are
  written to the XMP packet (`dc:creator`, `dc:rights`, `dc:description`,
  `dc:subject`) and, where the document carries an EXIF packet, to its Artist,
  Copyright and ImageDescription strings; older IPTC copies of the same fields
  are removed so they cannot contradict it. The fields persist in `.doctored` and
  travel in JPEG, PNG and TIFF exports. MCP `get_file_info` / `set_file_info`
- **Export Layers…** (File > Export Layers…) delivers every selected — or every
  visible, or every — top-level layer or group as its own file through an
  export preset. Each item is rendered on its own, against transparency, so its
  opacity, blend mode, mask and style apply to its own pixels; choose the whole
  canvas or a trim to the item's content. The pattern's `{layer}` and `{index}`
  tokens keep the names apart (one of them is required; `{name}`, the
  document's name, is optional), nothing is overwritten, and the report names
  every item with the path it wrote or the reason it refused. A 32-bit float
  document's layers export too, canvas-framed or trimmed: an exact 32-bit
  TIFF by default, and a trimmed item's samples are copied exactly
- **Contact Sheet…** (File > Contact Sheet…) composes a grid — columns, cell
  size, gutter, optional caption from the layer or file name, background colour
  — from a document's items, from other open documents or from files, into a
  new document or straight to a file through a preset. Cells are 8-bit sRGB and
  are never upscaled; a transparent item shows the background colour
- **Load Files into Stack and Auto-Align Layers**: File ▸ Load
  Files into Stack… builds one new document with each chosen file as a raster
  layer named after its file, bottom-first in name order (MCP
  `load_files_into_stack` keeps the order of its paths). The canvas is the
  largest width and height, and a smaller file sits at the top-left unscaled.
  The document is what File ▸ Open gives the first file — its depth and
  profile — and each later file is converted as Place converts it, with a
  notice naming the ones that were. A camera RAW takes its default
  development, a PSD or layered TIFF its composite and a PDF or SVG its first
  page; a native `.doctored` is refused by name. The whole stack is planned from the
  files' headers and admitted before anything is decoded, so a refusal opens
  nothing. Edit ▸ Auto-Align Layers… registers the selected raster layers to
  a reference — the one position-locked layer, else the lowest selected —
  with an Auto (which behaves as Perspective), Perspective, Collage
  (rotation, uniform scale and shift) or Reposition (shift only) projection,
  and warps each into place as one undo step and one cancellable job, at 8,
  16 and 32 bits and in CMYK; the canvas grows to the union of the aligned
  layers (a union past 100 megapixels is refused and nothing
  changes), and what a warp leaves uncovered is transparent. Vision registers
  first, and the core's own feature matching checks Vision's answer and takes
  over where Vision finds nothing or disagrees, so it follows layers that
  share about 15 % of the frame on a test table of real-photo views panned
  5–40°, frames under 512 px included, less with
  little texture; frames that carry lens distortion need Geometric
  Distortion on (with it off, such a layer is refused and reported rather
  than placed off), a layer already in place is left
  unchanged, and a layer it cannot
  register is left untouched and reported. MCP `auto_align_layers` takes
  `reference`, `projection` (the older `registration` still works alone) and
  `sampler`, and reports `canvas` when it changed. Cylindrical and Spherical
  fit one focal length to the registrations, reproject every layer, the
  reference too, onto the reference camera's cylinder or sphere, and set the
  canvas to the projected layers. The sheet's Lens Correction row adds
  Vignette Removal (MCP `vignette`), which fits one radial falloff to the
  registered overlaps and divides every layer by it before reprojecting
  (Perspective and Auto then switch to Spherical, and Collage and Reposition
  ignore it), and Geometric Distortion (MCP `distortion`),
  which fits one radial lens coefficient for the stack and corrects each
  layer with it on every projection (on Collage and Reposition each layer,
  the reference too, is corrected inside its own frame and placed in the
  same resample; a stack in which no layer moves is refused); Actions record
  both. Layer ▸ Stack Mode puts the Mean, Median, Minimum,
  Maximum, Range, Standard Deviation, Summation, Variance, Entropy, Kurtosis,
  Skewness or Majority of the selected raster layers into one new layer
  above them (a one-shot merge: the source layers stay, hidden), and Layer ▸
  Reduce Noise from Stack aligns the selected layers and takes their Mean or
  Median; both run at 8, 16 and 32 bits as cancellable jobs (Stack Mode and
  Reduce Noise from Stack on CMYK too, per ink), with MCP
  `stack_mode` and `reduce_noise_from_stack`. File ▸ Merge to HDR… (bracketed
  files) and Layer ▸ Merge Layers to HDR… (the selected raster layers of an
  8- or 16-bit document) merge exposures into a new 32-bit float document:
  exposure values from EXIF, else an estimate, else the ones you enter,
  alignment through Auto-Align and optional ghost removal, as one cancellable
  job, with MCP `merge_to_hdr` and `merge_layers_to_hdr`. Edit ▸ Auto-Blend
  Layers… blends the selected raster layers as a Panorama (each gets a mask
  along a least-visible seam, with Seamless tones matching the frames, and
  Content-Aware Fill Transparent Areas, MCP `fill`, stamps a
  "&lt;top frame> (merged)" layer above them and fills its transparent corners
  through Content-Aware Fill in the same undo step) or
  focus-stacks them with Stack Images (each gets a mask revealing where it is
  sharpest, optionally with one seamless merged layer above them),
  with an "Align first" pass through Auto-Align, as one undo step and one
  cancellable job at 8, 16 and 32 bits, with MCP `auto_blend_layers`. File ▸
  Automate ▸ Merge Panorama… builds a panorama in one dialog and one cancellable
  job from those same stages — Load Files into Stack, Auto-Align with the
  chosen layout (Auto, Perspective, Cylindrical, Spherical, Collage or
  Reposition) and its Vignette Removal and Geometric Distortion options,
  Auto-Blend's Panorama when Blend Images Together is on and Content-Aware
  Fill of the transparent border when that is on — into one new document; a
  frame it cannot register is kept, hidden and named, and a refusal names the
  stage it stopped at (MCP `merge_panorama`, recorded into Actions). Image ▸
  Adjustments ▸ HDR Toning… flattens the document, asking first when there is
  structure to lose, and tone maps its composite at native depth as one
  cancellable job and one undo step: Local Adaptation (the default: an
  edge-aware base and detail split of the log luminance with Radius,
  Strength, Smooth Edges, Gamma, Exposure, Detail, Shadow, Highlight,
  Vibrance, Saturation and a Toning Curve drawn over the image's histogram),
  Equalize Histogram, Exposure and Gamma, or Highlight Compression; its
  built-in preset is Default, and user presets are saved from the Preset
  popup, with Save and Delete (MCP `hdr_toning` `preset`; a recorded step keeps the
  preset's values, not its name). A 32-bit
  document becomes 16 bits (or 8), and Image ▸ Mode ▸ 8/16 Bits/Channel on a
  32-bit RGB document opens it at that depth; an 8- or
  16-bit document keeps its depth; CMYK is refused by name. MCP `hdr_toning`
  records and replays; measured against Photoshop, Exposure and Gamma at
  (0, 1) matches it, and the other methods and values differ from it
  ([ADR](https://github.com/kpgalligan/doctored/blob/main/designs/v1-stacks-status.md))
- **Save preservation**: Save and Save As offer formats that preserve the
  document's editing state, including single-layer masks, styles, descriptions,
  channels, guides, layer comps, layer properties and format-specific
  color/metadata losses.
  Use `.doctored` for an editable master; File > Export creates a flat copy without
  clearing the master's dirty state. Native saves and opens share capacity
  preflight, including an 8-bit `.doctored`'s 400-million aggregate layer-pixel
  limit (1.6 GB of decoded pixels). A 16-bit or 32-bit float document the
  working limit admits saves and reopens as a `.doctored` at that limit: its reader
  and writer stream the file instead of holding it whole, and the file and its
  decoded pixels may each reach 8 GiB (2 GiB and 1.6 GB before) — at the
  8 GiB limit eight 100 MP 16-bit layers, a 6.4 GB file, save and
  reopen and a ninth is refused. A refusal
  keeps the previous file and leaves reduction/export available in memory
- **Rendering integrity**: over-budget groups produce an explicit failure.
  The canvas keeps its last valid preview with a failure message; rendered
  output is unavailable for that revision, while supported native saves remain
  available. Resize and Gaussian blur sample premultiplied color to avoid
  dark or contaminated transparent edges, preserving the existing encoded-color policy
- **Colour management**: an ordinary flat byte image's embedded ICC profile is
  read on open and the document is converted into the colour working space (Image > Mode >
  Working Space — sRGB by default, or Display P3; it affects future opens
  only). Native documents and explicit PSD imports retain their source profile
  and decoded samples, including single-layer PSDs and the saved-composite choice.
  An ordinary flat RGB or grayscale image with no profile is assumed sRGB, which is what every reader
  does — so with the working space set to Display P3 an untagged file is
  converted too, and opening one and saving it straight back will not
  reproduce the input bytes; under the default sRGB working space that never
  happens. A platform-decoded or pasted image whose embedded profile a document
  cannot carry is converted from it to sRGB while it is read, and the import
  report says so. By default a CMYK TIFF (chunky or planar, its transparency
  kept) or CMYK (or YCCK) JPEG the core reads, and a CMYK PSD, PSB or layered
  TIFF (its saved composite, or with Import Layers its layers, masks, groups,
  blend modes, layer effects in ink with Fill, knockout, Blend Interior and
  Blend If, and Levels, Curves, Hue/Saturation, Brightness/Contrast, Color
  Balance, Selective Color, Channel Mixer, Photo Filter, Invert, Posterize,
  Threshold and Gradient Map layers), open as a CMYK
  DOCUMENT instead: its own ink
  and embedded profile are kept, an untagged one is assigned the working CMYK
  profile, it is shown through its profile and it saves back as CMYK
  ([#1](https://github.com/kpgalligan/doctored/issues/1),
  [the status](https://github.com/kpgalligan/doctored/blob/main/designs/cmyk-mode-status.md)): TIFF and PSD bit-exact at 8 and
  16 bits, Export as PSD writing layered or flattened PSD, PSB and TIFF
  with those same records (type and shape layers as pixels, named in the
  report; solid, gradient and pattern fill layers as Photoshop's `SoCo`,
  `GdFl` and `PtFl` records), which Photoshop opens layered with every style,
  JPEG through ImageIO, and `.doctored` with every layer.
  Image ▸ Mode ▸ RGB Color… and CMYK Color… convert either way as one undo
  step (relative colorimetric with black point compensation, to the working
  RGB or CMYK profile; MCP `convert_mode`, recorded by Actions), the Channels
  panel shows the C, M, Y and K plates, and `sample_color` reads ink
  percentages and total ink. A CMYK document edits its layer structure —
  new layers, duplicate, delete, reorder, rename, hide, opacity, label, lock,
  Normal and Pass Through groups, clipping masks, the blend modes a CMYK
  document offers (evaluated on 1 − ink) with Fill Opacity, Merge Down, Merge
  Layers, Merge Visible, Stamp Visible and flatten — and layer masks,
  selections and guides; alpha channels with Save and Load Selection, Quick
  Mask, paths and vector masks, Fill and Stroke Path in ink, the Magnetic
  Lasso, Grow, Similar, Modify and Transform Selection and luminosity masks
  work there too, and type, shape and Solid Color/Gradient fill layers stay
  live with their colours stored as ink; the Brush, Eraser,
  Fill, Gradient and Paint Bucket paint in ink in every blend mode offered
  there (the Color Picker's C, M, Y and
  K fields set it exactly, and the Info panel reads CMYK and total ink), and
  Levels and Curves adjust each ink or the composite, destructively or as
  adjustment layers; Hue/Saturation (and Desaturate) acts on the cyan, magenta
  and yellow inks with black kept (Colorize alone sets K),
  destructively or as an adjustment layer. Levels and Curves show each ink's
  histogram and offer Auto (with Auto Options), the black, grey and white
  eyedroppers and ⌥ clipping on ink. Assign and Convert to Profile (CMYK to
  CMYK, or to RGB), 8 ↔ 16 Bits/Channel, File ▸ New in CMYK, Proof Setup
  (Custom with Preserve CMYK Numbers, Working CMYK and its four plates) and
  the export workspace (to sRGB, or keeping CMYK for JPEG and TIFF, with
  resizing, output sharpening, a TIFF depth change and the metadata policy)
  work on a CMYK document; Edit ▸ Color Settings… holds the working spaces, the
  policies applied on open and the conversion options, and an RGB document's
  Info panel, Color Picker and View ▸ Gamut Warning flag colours outside the
  working CMYK. Image Size, Canvas Size, Crop, Perspective Crop, Trim,
  Reveal All, rotation and flips, Free Transform, Warp, Puppet and
  Perspective Warp, Liquify, the Move tool and align/distribute reshape the
  ink. The other adjustments and filters, but for the few named below, run
  on ink too (Auto Tone, Contrast and Color, Equalize, Edit ▸ Fade and Repeat
  Last Filter included; Threshold's black is K 100 % alone), and so do
  Dodge and Burn (with Protect Tones), the
  Sponge, Blur, Sharpen and Smudge, the History Brush, the Art History Brush
  (8-bit CMYK), the Clone Stamp, the healing tools, Patch, Remove (in its
  Content-Aware mode), Content-Aware Fill and Move, Generative Fill and
  Expand (8-bit CMYK) and Layer ▸ Stack Mode (its statistic per ink);
  Exposure, Vibrance, Black & White, Lens Correction, the Develop Filter
  and Auto-Blend, and the RGB-only ones (Red Eye among them), refuse by name
  for now. Layer effects, knockout and Global Light work on ink, their colours
  held as CMYK inks, on raster and adjustment layers and on groups, and Blend
  If works on Gray and the four plates, several at once. One ink plate targeted in the Channels panel
  takes the Brush, Eraser, Fill, Stroke, Gradient, Paint Bucket, filters and
  adjustments on its stored 1 − ink, and the Move tool and ⌘T move its
  selected pixels; Apply Image and Calculations read and write the
  composite, the plates and alpha channels.
  A GRAYSCALE document holds one gray channel at 8, 16 or 32 bits under its
  Gray profile ([#1](https://github.com/kpgalligan/doctored/issues/1),
  [the ADR](https://github.com/kpgalligan/doctored/blob/main/designs/grayscale-mode.md)). Image ▸ Mode ▸ Grayscale
  converts an RGB or CMYK document by luminance (the rule a Photoshop take
  confirmed at 8 and 16 bits) and RGB Color converts back, one undo step each
  (MCP `convert_mode` `gray`, recorded by Actions); it round-trips through
  `.doctored` and saves and exports one-channel PNG, TIFF and JPEG with its Gray
  ICC. It takes new layers, groups, masks, clipping, merges, Fill Opacity and
  knockout in 21 blend modes (12 at 32
  bits); the Channels panel shows one Gray row and the Info panel a K % at 8
  and 16 bits; the Brush, Eraser, Paint Bucket, Fill and Gradient paint a
  colour as its luminance; and Levels, Curves, Brightness/Contrast, Exposure,
  Invert, Posterize, Threshold and Gradient Map run destructively or as
  adjustment layers, Shadows/Highlights and Replace Color destructively, with
  Auto Tone, Auto Contrast and Equalize. Every filter but Liquify runs on gray
  at 8, 16 and 32 bits, with Fade, Repeat Last Filter and Actions; so do Dodge,
  Burn, the Sponge (toward or away from middle gray), Blur, Sharpen, Smudge,
  the History Brush, the Clone Stamp, the healing tools, Patch, Stroke,
  Content-Aware Fill and Move, Remove (8 and 16 bits) and the Art History
  Brush (8 bits), and the Brush, Clone Stamp and Art History Brush Mode
  popups list the Grayscale modes. HDR Toning tones an 8- or 16-bit Gray document at its depth, and a
  32-bit one into 16 or 8 bits. All nine layer
  effects work, each colour stored as its gray; Blend If offers Gray; alpha
  channels take paint, Fill and filters as a mask does; and RGB to Gray
  carries adjustment layers, effects and a Gray Blend If, and type, shape
  and fill layers (re-rendered, their colours greyed). Vibrance,
  Hue/Saturation, Color Balance, Black & White, Photo Filter, Channel Mixer,
  Color Lookup, Selective Color and Auto Color are refused. Geometry,
  transforms, the warps and Liquify, selections and copy and paste work there
  too (a paste across modes always converts, and a copy's pasteboard TIFF is
  one gray channel). Assign and Convert to Profile take Gray profiles (an RGB or
  CMYK one is refused by name), and 8, 16 and 32
  Bits/Channel convert both ways, each one undo step. Proof Setup and Proof
  Colors work at 8 and 16 bits, and Mode ▸ CMYK Color converts a Grayscale
  document directly. Type, shape and fill layers (each colour stored as its
  gray), paths and vector masks work there too, as do layer comps, links
  and snapshots, the Histogram, Apply Image, Calculations, Stack Modes,
  Auto-Align, Auto-Blend, Load Files into Stack, Merge to HDR (a 32-bit
  Grayscale result), Merge Panorama, the model selections, Super Zoom, JPEG
  Artifacts Removal and Generative Fill, Expand and Variations, each colour
  that comes from outside stored as its gray; Neural Colorize and Sky
  Replacement are refused by name. Place Embedded and Linked fold the placed
  image to gray with no retained source, RGB to Gray keeps a Generative
  Layer, and the RAW and image-source tools are refused for good by name.
  An 8- or 16-bit gray PSD, PSB or layered TIFF, or a 32-bit gray PSD
  or PSB, opens as a Grayscale document with its layers, masks, groups,
  effects, Blend If and Gray profile, and Export as PSD writes one back
  at the document's depth (a layered 32-bit TIFF is refused); a 32-bit
  document's TIFF is one float channel.
  A LAB COLOR document holds L\*a\*b\* (D50) at 8 or 16 bits; there is no
  32-bit Lab
  ([#1](https://github.com/kpgalligan/doctored/issues/1),
  [the ADR](https://github.com/kpgalligan/doctored/blob/main/designs/lab-mode.md)). File > New's Color Mode **Lab
  Color** makes one; Image ▸ Mode ▸ Lab Color converts an RGB, Grayscale or
  CMYK document, and RGB Color, CMYK Color (through 16-bit ProPhoto RGB),
  Grayscale (on L\* alone) and 8/16 Bits/Channel convert from it, one undo
  step each (MCP `new_document` `color_mode` `lab` and `convert_mode`,
  recorded by Actions). Photoshop takes measured the RGB to Lab encoding
  within 0.76 of a code of Photoshop's at 8 bits and 0.02 at 16, and
  confirmed Lab to Grayscale. The canvas, `render`, print and thumbnails show
  it through the core's own Display P3 conversion; the Channels panel shows
  Lightness, a and b, and the Info panel, the samplers and `sample_color`
  answer in L\*a\*b\*. It round-trips through `.doctored`; a Lab TIFF, PSD, PSB or
  layered TIFF opens with its layers, MCP `save_copy` writes a CIELab TIFF and
  Export as PSD a Lab PSD, PSB or layered TIFF as Photoshop writes
  them, and Export…, Export Layers, PDF and contact sheets convert to RGB.
  The rest of the RGB surface works on the codes: layers, masks, alpha
  channels and 19 blend modes, each within one
  8-bit code of a Photoshop take; painting, with an Eyedropper colour kept as
  an exact Lab ink; Levels, Curves and the tonal adjustments on the codes,
  and most colour adjustments through 16-bit ProPhoto RGB; the filters,
  retouch tools and layer effects, and Blend If on Lightness, a and b
  (not measured); geometry, warps, selections (Color Range's Sampled Colors by the
  per-channel rule a take measured), copy and paste across modes, type,
  shape and fill layers, layer comps, Proof Setup, Convert to Profile, the
  Histogram, Apply Image, stacks, Auto-Align, Merge Panorama, Place and the model
  selections; and the neural and generative tools read the Display P3 view
  and land their results as codes by the Mode rule. A Lab document has no
  profile of its own, so Assign Profile is refused, and Auto-Blend Layers,
  Sky Replacement, 32 Bits/Channel, some adjustments and filters and the
  other eight blend modes are refused; the Magic Wand and Paint Bucket compare Lab
  codes by the rule a Photoshop take measured, as do Select Similar and Grow
  at 16 bits, which at 8 bits refuse by name pending their issue
  ([#5](https://github.com/kpgalligan/doctored/issues/5)); and every
  other tool (the RAW and image-source tools among them) is refused for good
  by name.
  An INDEXED COLOR document holds a palette of 2 to 256 colours, an
  optional transparent index and one index a pixel, at 8 bits in one layer
  ([#1](https://github.com/kpgalligan/doctored/issues/1),
  [the ADR](https://github.com/kpgalligan/doctored/blob/main/designs/indexed-mode.md)). A palette PNG, a
  single-image GIF, a palette TIFF or an Indexed Color PSD or PSB opens as
  one with every index kept, and
  Image ▸ Mode ▸ Indexed Color… converts an 8-bit RGB or Grayscale document
  (a layered one after flattening) with the Exact, Web, Uniform, System
  (Mac OS), System (Windows), Adaptive (median cut), Custom and Previous
  palettes, Colors, Forced,
  Transparency and Matte, and Dither None, Diffusion (Floyd–Steinberg),
  Pattern or Noise (the last two Doctored's declared rules), one undo step
  (MCP `convert_mode` `indexed`, recorded by Actions). RGB
  Color converts back exactly, and Grayscale, CMYK Color and Lab Color
  convert from it directly. Image ▸ Mode ▸ Color Table… edits the palette,
  every pixel keeping its index, and loads and saves Adobe `.act` files (MCP
  `get_color_table` and `set_color_table`). It round-trips through `.doctored`,
  and PNG, GIF and TIFF saves and exports in the document's profile keep the
  palette, the transparent index and every index exactly, and so does File ▸
  Export as PSD…, its PSD or PSB written in Photoshop's own form and read back by
  Photoshop with the same palette, transparent index and indices; the Info panel
  shows the index under the cursor. The Brush, Fill, Stroke and the Paint
  Bucket paint it in every blend mode: a
  changed pixel takes the palette entry nearest the ink (in a mode other than
  Normal, the mode's full-strength colour over the pixel) when its coverage is
  at least 131/255 and otherwise keeps its index, Doctored's disclosed rule,
  not measured against Photoshop, whose own edge in Indexed Color differs
  between two identical strokes. The Eraser, Clear and Cut leave the transparent index, or with
  none the entry nearest the background swatch (white over MCP, except crop's
  `background_color`), and so does
  new area from Canvas Size, a rotation, a move or a transform. Copy and Copy
  Merged copy the palette's colours, and Paste and Paste Special land in the
  one layer, a colour equal to an entry taking that entry. Image Size, Canvas
  Size, Trim, Image Rotation, the flips, Crop, Perspective Crop, Free
  Transform, Transform Again, the selected pixels' transform, Warp and the
  Move tool keep every index exactly where pixels only move (whole pixels or
  Nearest Neighbor) and otherwise re-index each changed pixel to its nearest
  entry. The layer row renames, labels, locks, hides and offsets the one
  layer. Assign Profile keeps every index and the table's codes; Convert to
  Profile converts the colour table and keeps every index. Select ▸ Color
  Range, Similar and Grow sample the palette's colours, and Modify, Transform
  Selection, Quick Mask and Save and Load Selection work. New layers,
  Duplicate Layer, adjustments (HDR Toning among them), filters,
  Content-Aware and Generative Fill and Select Subject are refused, as are
  Auto Tone, Auto
  Contrast, Auto Color, Equalize, Puppet Warp, a layer mask and a layer's
  opacity and blend mode; Align, Distribute, Auto-Align, Auto-Blend, Delete
  Layer and Flatten Image refuse because the document holds one layer.
  Forced Custom works, `render` shows the Index and alpha channels, and every
  other tool works or is refused for good by name. The Local and Master Perceptual and
  Selective palettes are not offered; they refuse by name and point to Adaptive.
  A Photoshop take kept Pattern (99.2 % of
  Photoshop's pixels on a Web ramp, 62.9 % on System (Mac OS)) and Noise as
  declared; Photoshop refused every Noise encoding the take tried, so Noise
  is unmeasured.
  A DUOTONE document is an 8- or 16-bit Grayscale document printed with 1 to
  4 inks, Monotone to Quadtone
  ([#1](https://github.com/kpgalligan/doctored/issues/1),
  [the ADR](https://github.com/kpgalligan/doctored/blob/main/designs/duotone-mode.md)). Image ▸ Mode ▸ Duotone…
  converts a Grayscale document only, and later edits the
  inks: each a name, a D50 Lab colour and a 13-point curve, with Overprint
  Colors and a Preview, one undo step (MCP `convert_mode` `duotone`,
  `get_duotone`, `set_duotone`). Every Grayscale edit works on the gray. The
  canvas and the thumbnails show the gray through the inks, and the Channels
  row is named after the type. It round-trips through `.doctored`, a Duotone PSD or
  PSB opens with its inks and Export as PSD writes one, and Duotone
  Options loads and saves `.ado` files; no Pantone or other licensed ink
  library ships. Image ▸ Mode ▸ RGB, CMYK, Lab and (8-bit) Indexed Color
  convert through the inks; Grayscale renders the inks into the gray,
  keeping the document's own Gray profile; and Bitmap… (from
  8 bits) is that Grayscale followed by the Bitmap conversion, one undo step
  each. Every export renders RGB through the inks, and any other save
  refuses, naming Export as PSD. The ink model is measured against
  Photoshop (within 0.6 of a code on two- and three-ink documents), so is
  the Grayscale conversion (within one code), and Photoshop opens Doctored's
  Duotone PSDs with their inks and curves; Bitmap from Duotone is composed
  and unmeasured. In Duotone, Assign and Convert to
  Profile, Proof Setup and Proof Colors (which can still be turned off),
  Content-Aware Fill, Puppet and Perspective Warp, Select ▸ Select Subject and Select ▸ Sky,
  the Neural Filters, Develop Filter, Lens Correction, Tilt-Shift and 16
  and 32 Bits/Channel refuse, and Merge to HDR runs as on Grayscale.
  A BITMAP document holds one bit a pixel, black or white, in one layer
  ([#1](https://github.com/kpgalligan/doctored/issues/1),
  [the ADR](https://github.com/kpgalligan/doctored/blob/main/designs/bitmap-mode.md)). Image ▸ Mode ▸ Bitmap… converts an
  8-bit Grayscale document (a layered one after flattening;
  16- and 32-bit Grayscale convert to 8 Bits/Channel first, colour documents
  to Grayscale first, and an 8-bit Duotone document through its Grayscale
  conversion), by 50% Threshold, Pattern Dither, Diffusion Dither, Halftone
  Screen (frequency, angle and six spot shapes) or Custom Pattern, at its own
  or a new Output resolution; Grayscale converts back with a Size Ratio, one
  undo step each (MCP `convert_mode` `bitmap` and `gray`, recorded by
  Actions). It round-trips through `.doctored`; a 1-bit PNG, TIFF or BMP and a
  Bitmap PSD or PSB open as one, and Save As (or MCP `save_copy`), Export and
  Export as PSD write them back at 1 bit (JPEG through 8-bit Grayscale).
  Measured against Photoshop 2025: 50% Threshold matches it on every pixel,
  and Pattern Dither's threshold table, fitted to Photoshop's, matches it on
  99.2 % of a test ramp; Diffusion Dither is Sierra Lite and does not match
  (Photoshop's changes from session to session; they agree on about 70 % of
  the pixels), and Halftone Screen is Doctored's own screen, not measured
  (its conversion hung both Photoshop sessions that tried it). On a Bitmap
  document the Brush, Eraser, Fill, Stroke, Paint Bucket and Clear paint in
  black and white in Normal, Darken or Lighten mode, each pixel landing by
  a 50% rule, so edges are hard; the
  Gradient is thresholded the same way, and the Clone Stamp, History Brush
  and Liquify work. Crop, Canvas Size, the quarter turns and the flips move
  the plane bit for bit; Image Size, the Crop tool's straighten, Perspective
  Crop, Move and the selected pixels' moves resample the gray and threshold
  it; Straighten Layer is the straightened crop; Cut (leaving white) and
  Copy, the selections with Quick Mask, Quick Selection and the Magnetic
  Lasso, and the layer row's name, label and lock work, and Info reads K.
  Free Transform, arbitrary rotation, Trim, the profile commands, Color
  Range, the Magic Wand, the filters, new layers, masks and styles, the
  operations that need a second layer, and the alpha channels with Save
  Selection refuse for good by name; Lens Blur is not built there.
  MULTICHANNEL mode is not built, by decision
  ([the ADR](https://github.com/kpgalligan/doctored/blob/main/designs/multichannel-mode.md)): it waits on spot channels, which
  Doctored keeps in `.doctored` and in 8- and 16-bit PSD and PSB files (not
  Lab), so a Multichannel PSD or PSB and `convert_mode`
  `multichannel` refuse by name, and Delete Channel deletes only alpha
  channels.
  Opened explicitly as 8-bit RGB, the core's own open converts a
  grayscale file with a
  Gray profile (which otherwise opens as a Grayscale document), and a CMYK TIFF or CMYK
  (or YCCK) JPEG with a CMYK profile — each is converted from that profile to
  sRGB as it is read, CMYK at relative colorimetric with black point
  compensation, finding each profile's black point the way Little CMS does — and
  any other embedded profile it cannot use is named in the import report. An
  untagged CMYK TIFF or JPEG opened that way is converted the same way from the working CMYK
  profile — U.S. Web Coated (SWOP) v2 when Adobe's profiles are installed, else
  macOS's Generic CMYK — and the import report names it. A
  Gray, CMYK or Lab file the core does not read itself (a CMYK TIFF past its
  size limits, a Lab TIFF with an associated alpha) is opened by
  the platform decoder instead, which converts it to sRGB as it reads it, and
  File > New from Clipboard does the same with a Gray, CMYK or Lab paste —
  including a Finder file copy, whose own file names the profile; the
  import report says so either way — in the core's own words, naming the
  embedded profile, when that profile is one the core would have converted
  from, and otherwise as a plain "converted to sRGB" sentence naming the colour
  model. Over MCP, placing an image names it in the
  reply's `disclosures`,
  and a contact sheet built from files lists it per
  file in `profile_refusals`; an assisted-annotation case file that carries an
  embedded profile is refused. The canvas,
  the layer and channel thumbnails and every preview are tagged with the
  document's own profile, so a Display P3 photo finally looks right on a
  wide-gamut display. **Assign Profile…** relabels the
  document — the pixel numbers do not change, so the picture changes
  appearance, unless the new profile describes the space the document was
  already in — while **Convert to Profile…** transforms every layer's pixels
  so each layer keeps its appearance; both live under Image > Mode.
  **Convert to Profile…** also takes a rendering INTENT (perceptual, relative
  colorimetric, saturation, absolute colorimetric), black point compensation,
  and an explicit choice of what happens to the adjustment layers — keep
  their parameters (the default, and what Doctored has always done) or
  preserve the appearance, which keeps the light-linear adjustments and bakes
  each of the others into one disclosed raster layer in its place without
  flattening anything else. The sheet's check of what a conversion would do
  is a cancellable wait at 8 and 16 bits: Esc ends it and Apply stays
  disabled until a setting changes. The destination may be either built-in (sRGB or Display P3), any
  profile on disk, or any RGB profile installed under the ColorSync, Adobe and
  printer-driver (`/Library/Printers`) folders. **Image ▸ Evaluation…** sits beside them and answers the other
  question about the document's numbers: whether the projection composites
  them **as stored** (the default, and what every document has always done)
  or in **linear light** — decoding each layer through the document's own
  profile, compositing at f32 in linear light and re-encoding once at the
  output. It changes what every blend, opacity and mask means, so the sheet
  previews the result on the canvas and measures how far it sits from the
  picture behind it. Linear light evaluates every blend mode, opacity, masks,
  groups, clipping, adjustment layers (a guided Shadows/Highlights included),
  layer styles and Blend If — whose ranges, like adjustment kernels, still read
  display levels — and Image Size, Free Transform, Warp and a RAW layer's
  re-render resample in linear light. Painting, fills, filters, Liquify and the
  mesh warps still edit the stored numbers
  ([#35](https://github.com/kpgalligan/doctored/issues/35)). An encoded document is
  byte-for-byte what it always was.
  **Image ▸ Tone Map…** is the same kind of question one step further out.
  Opening a genuine floating-point TIFF, a 32-bit RGB PSD or PSB, an OpenEXR
  file or a Radiance `.hdr` file gives you a **32-bit float document**, and
  so do File ▸ Merge to HDR…, a camera RAW developed at 32-bit float
  (linear), File ▸ New… at
  32 bits and Image ▸ Mode ▸ 32 Bits/Channel… on an 8- or 16-bit document:
  linear light with an extended range, where a specular highlight really is
  8.0 and a wide-gamut colour outside the document's primaries really is
  negative, and both survive editing, saving and reopening exactly. An OpenEXR
  file opens its first flat part that holds R, G and B, on a canvas the size
  of its display window, with associated alpha unpremultiplied (other parts
  are named in a warning, and a file holding a deep part is refused by name);
  a Radiance file opens opaque. Stated primaries that match Rec. 709 or
  Display P3 take that built-in profile, absent ones are read as sRGB with a
  warning, and any other set is converted once, in linear light, to sRGB
  primaries with a warning. Promoting an
  ordinary document invents no HDR information: each code becomes its own
  linear light through the profile's tone curve (white is exactly 1.0), every
  layer, description, RAW binding and adjustment comes along, converting back
  returns every code, and a blend mode or adjustment float cannot hold, a warp
  still in effect or a profile with no matrix/curve model blocks it by name. The
  mode composites in Normal,
  Multiply, Darken, Lighten and Difference, in nine further modes —
  Dissolve, Darker Color, Linear Dodge (Add), Lighter Color, Subtract, Hue,
  Saturation, Color and Luminosity (measured against Photoshop: Linear Dodge
  and Subtract unclamped, the colour modes with no clip into 0…1) — and in Linear
  Burn, Linear Light, Pin Light and Divide, unclamped (Divide with a 1/65504
  floor); the nine modes that invert about white (Screen, Overlay, Soft
  Light, Hard Light, Color Dodge, Color Burn, Vivid Light, Hard Mix and
  Exclusion) are refused by name, since above 1.0 they have no meaning. It
  carries layer opacity, masks,
  channels, clipping and groups, and takes every adjustment layer but three:
  Exposure and White Balance, which are already defined in linear light;
  Levels, Brightness/Contrast/Saturation, Black & White (its Tint too),
  Channel Mixer, Photo Filter, Color Balance and Curves, a pencil-drawn map
  included, each evaluated on the linear values (measured against Photoshop
  at 32 bits); Hue Rotate,
  Sepia and Grayscale in closed form; Threshold, Posterize and Gradient Map
  through the display encoding of each value held to 0…1; Vibrance, Selective
  Color, Replace Color and every Hue/Saturation setting scaled so a value
  above white keeps its light; and every Color Lookup table.
  Invert, Shadows/Highlights and the legacy White Balance are refused by name,
  since each is defined on 0…1. Image ▸ Adjustments applies the same set
  destructively there too (Brightness/Contrast in either form), with the same
  result as the layer — and it takes the
  canvas and layer geometry — Canvas Size, crop and straighten, rotation, the
  flips, Image Size, Free Transform, Distort, Warp and Transform Mask — and Edit ▸
  Fill… and Stroke… on a layer, its mask or an alpha channel, the colour
  filling as linear light in any of those blend modes. Load Selection
  from the Red, Green, Blue or luminosity plane or from a layer's transparency
  works there too, clamping each scene-linear value to 0…1,
  whatever the tone map — the luminosity plane being the document profile's Y
  row over the unclamped values with only the sum clamped, as a Photoshop take
  confirmed. The Magic
  Wand, Grow, Similar, Color Range and Quick Selection compare on the display
  encoding of each value held to 0…1, so a value above white compares as
  white, and select with full 16-bit coverage; Select and Mask's edge stage
  (its Radius, Smart Radius and Refine Edge brush, and Quick Selection's
  Enhance Edge) reads that same encoding, its New Layer outputs keep every bit
  of the float composite under the matte, and only its Decontaminate Colors is
  refused by name. The Magnetic Lasso traces the composite clipped to white at
  exposure 0. Vision's
  selections — Select Subject, Select Objects, Select People…, All People,
  Face Parts, the Subject tool and the Object Selection tool (whose edge
  refinement is off there unless MCP `object_select` passes `refine: true`) —
  work on the document's tone-mapped picture at view exposure 0, so the
  window's exposure never changes what is selected. A filtered
  resample there works on premultiplied extended-range samples and clamps
  each layer's RGB to that layer's own visible range, so a Bicubic or Lanczos
  overshoot never leaves it; an output alpha below 2⁻²⁴ becomes transparent.
  A float warp takes the same resample and is applied at once: it keeps no
  re-editable field, so each Apply warps the current pixels again and Undo
  adjusts or removes it.
  Layer styles — all nine effects in those blend modes, fill opacity, knockout
  and Blend Interior Effects as Group — Blend If and the global light work
  there too: an effect's colour composites as its linear light, Blend If on
  the stored values (measured against Photoshop at 32 bits). A placed image
  (Place Embedded or Linked) lands there as an ordinary layer: its pixels —
  8- or 16-bit, or 32-bit float for a floating-point TIFF — are converted
  to the document's profile first (one in a LUT profile through its table,
  unclamped) and
  rendered into linear light, with no depth question asked. A Live Photo still or
  movie frame places there the same way; a camera RAW file places as a RAW
  layer that develops in 32-bit float and re-develops, and Redevelop RAW, RAW presets, Synchronize, Migrate
  RAW Decoder and RAW locals work on it there.
  Text and shape layers work there too: their colour numbers are linear
  light and may lie above white (the Color Picker, the Artwork Colors sheet and the Shape tool's
  swatches gain an Intensity row in stops there, and the eyedropper picks a
  colour above white into the foreground), and a layer
  pasted in or converted out has its numbers rewritten so it looks the same,
  clipped at white when it leaves 32 bits.
  The Brush, Eraser and Gradient paint there too, in linear light with no
  clamp on colour (a foreground above white paints as its own value), on a
  layer, its mask or an alpha channel; the Paint
  Bucket fills the region the Magic Wand's comparison finds, in linear light;
  Dodge and Burn shape each value through the display encoding (one above
  white scales by the curve's gain at white) on a layer, its mask or an alpha
  channel; and the Sponge works on a layer's linear values unclamped.
  The Clone Stamp, Healing Brush, Spot Healing Brush, Patch, Remove (in its
  Content-Aware mode), Content-Aware Fill, Content-Aware Move, Stroke Path, the Blur, Sharpen and
  Smudge brushes and the History Brush work on a float layer's pixels as well,
  keeping values above white.
  Eleven filters run on a float layer — Gaussian Blur, Box Blur, Motion Blur,
  Radial Blur, Lens Blur, Tilt-Shift, Offset, Lens Correction, Liquify,
  Unsharp Mask and High Pass — on the linear values with no clamp to 0…1 (a
  blur spreads the energy of a pixel far above white, and Unsharp Mask and
  High Pass keep values below zero), confined by the selection, as cancellable
  jobs with their previews and Repeat Last Filter, and over MCP
  (`apply_filter`, `liquify`). Gaussian Blur there is the box-pass blur
  (below) from radius 7 up, and below 7, as Unsharp Mask and High Pass at every radius, the
  windowed Gaussian. Add Noise (relative
  to the light) and Pixelate (the cell's mean light) run on the linear values
  too, and the filters whose settings are display values — Surface Blur, Dust
  & Scratches, Reduce Noise, Add Grain, Clouds, Difference Clouds, Emboss,
  Edge Detect, the legacy Sharpen, Presence, Develop Filter and Remove Red
  Eye — run on the layer's display encoding (the profile's curve to white, a
  logarithm above it) and are decoded back, so a value above white keeps its
  light. Dehaze (in Presence and the Develop Filter) runs on that encoding
  too, measuring the haze on values clamped to white so a highlight cannot
  drag it and no value is pushed above white; the Develop Filter's
  Highlights and Shadows leave a pixel at or above white unchanged, and its
  Vibrance and Saturation work in linear light as the Vibrance adjustment does.
  In an RGB float document every float filter but Liquify, Depth Blur and the
  Develop Filter also runs on a layer mask, an alpha channel or one Red,
  Green, Blue or Alpha plane — a colour plane on its unclamped linear values,
  a mask or channel as coverage held to 0…1 — from the Filters menu, over MCP
  (`apply_filter` with a target) and in Actions.
  Apply Layer Mask, Layer Via Copy and Layer Via Cut (alpha scaled by the
  coverage, every RGB value kept exactly), Assign Profile (the linear samples
  relabelled bit for bit), Convert to Profile (each layer's linear RGB mapped
  between two matrix/TRC profiles' primaries with no clamp; absolute intent,
  black point compensation and preserve-appearance refuse by name),
  Calculations, Add Luminosity Masks and Contact Sheet run there too, as do
  paths and vector masks, Load Files into Stack, Auto-Align Layers, Stack
  Mode, Reduce Noise from Stack, Auto-Blend Layers, Puppet Warp, Perspective
  Warp and Depth Blur; Apply Image stays refused, and Auto Tone, Auto Contrast
  and Auto Color refuse at 32 bits.
  Everything else is refused by name. The tone map is how those numbers become an
  ordinary picture: **Clip to white**, or **Reinhard** with a white point that
  maps exactly to white and keeps the highlights above it distinguishable. It
  applies on screen and to every 8-bit or 16-bit export and to both
  conversions, while a **32-bit float export** — a TIFF, an OpenEXR file or a
  Radiance `.hdr` file — writes the samples as they are, exactly at 32-bit
  float (a half-float TIFF or OpenEXR and Radiance's RGBE round them) — or,
  with a size, resampled the same way Image Size resamples
  them, clamped to the composite's own visible range. The readouts change with it: linear components with stops instead
  of codes, and a histogram binned in stops from −12 to +6 EV that counts
  what is above white and below zero rather than what is clipped; Levels and
  Curves instead plot the 0…1 axis their handles act on, and their After plot,
  clipping wedges and ⌥ clipping display work there, counting a value at or
  past 0 or 1. The Tone Map
  sheet previews its candidate map live on the canvas, and **View ▸ 32-bit
  Exposure…** (with Reset 32-bit Exposure) sets a view-only exposure of −20 to
  +20 stops per window, applied before the tone map: it is view state — no
  undo step, nothing saved, nothing an export carries, and the readouts never
  see it. MCP `get_view`, `set_view` and `open_second_view` carry `exposure`,
  and `render` takes its own `exposure` argument and never reads the window's.
  **View ▸ HDR Preview** (per window, on by default) shows a float
  document's values above 1.0 brighter than white on an EDR (XDR) screen,
  with the view exposure applied and the screen's current headroom followed;
  off, or on an SDR screen, the tone-mapped view is unchanged, and the
  readouts, `render`, thumbnails and saves read the document's values only.
  `get_view` and `set_view` carry `hdr_preview`.
  **View ▸ Proof Setup**, **Proof Colors** (⌘Y) and **Gamut
  Warning** (⇧⌘Y) simulate another medium on the canvas — a coated press
  profile, say — without touching the master, any render or any export.
  A proof condition can be saved as a **proof preset** (Proof Setup ▸
  Custom… ▸ Save…) and chosen again from the Proof Setup submenu; one preset
  can be the default a window with no setup proofs through on ⌘Y. A preset
  keeps the profile's path and checksum, so a profile that changed or moved
  is refused by name rather than proofed through silently. At
  16 bits the canvas proofs the 8-bit display codes it already shows (MCP
  `render_proof exact:true` proofs the full 16-bit composite instead, on the
  background worker); a 32-bit float document has no soft proof. The
  profile is embedded on export (PNG, JPEG, TIFF and WebP can carry one), and
  supported EXIF / XMP / IPTC capture packets from JPEG, PNG and TIFF are
  written into JPEG/TIFF, and EXIF and XMP into PNG (a PNG has nowhere to put
  an 8BIM run). A JPEG's ExtendedXMP — the extra XMP segments Photoshop,
  Lightroom and Pixel cameras write when a packet outgrows one segment — is
  merged back into one packet on open, and an XMP packet too large for one
  JPEG segment is split the same way on export instead of being dropped. Flat exports
  leave out a Pixel photo's uncropped original and depth map (Google's
  `GImage:`/`GDepth:` XMP); the `.doctored` keeps them. RAW and HEIC imports reconstruct standard ImageIO capture
  fields and its XMP view. Exports reset orientation to 1 — the camera rotation is
  already baked into the pixels, so a preserved 6 would double-rotate it in
  every viewer. The reset covers **both** copies a file carries: the EXIF tag
  and the XMP packet's `tiff:Orientation`, which XMP's own reconciliation
  rules make the authority for Bridge and Adobe Camera Raw. The resolution is
  written into both as well, and an EXIF block that cannot be brought to the
  document's is dropped — and reported dropped — rather than left
  contradicting the density beside it. A file that states its resolution twice is read from its EXIF
  tags, which the Exif specification makes the authority and which is what
  ImageIO and ImageMagick answer for the same file, with the container's own
  density (a JFIF header, a PNG `pHYs`) as the fallback. A non-sRGB export
  also gets EXIF's ColorSpace tag set to "Uncalibrated", the value that means
  "read the embedded profile" — a camera's "sRGB" left beside a Display P3
  profile is how a wide-gamut export comes out oversaturated elsewhere. The Export panel's **Embed colour
  profile** (on) and **Strip metadata** (off) checkboxes govern both, and a
  format that cannot carry something says so rather than dropping it
  silently — unless nothing is actually lost, as when an sRGB document is
  written to a format with no profile slot and every reader assumes sRGB
  anyway. **Image resolution** in pixels per inch travels with the
  document (Image Size's Resolution field, with Resample off changing only
  the print size, never a pixel), and **File > Print** (⌘P) prints the
  flattened composite at that resolution, fitted to the page (at 16 and
  32 bits, the display projection the canvas shows); **Page
  Setup…** is ⇧⌘P. The colour management is Doctored's own: ICC v2/v4 RGB
  matrix/TRC profiles are parsed and transformed by the core, and the four
  built-ins it writes (sRGB IEC61966-2.1, Display P3, Adobe RGB (1998) and
  ProPhoto RGB) are real ICC blobs any other application reads
- Smooth zoom (pinch, ⌘+/⌘-, fit ⌘0, actual size ⌘1 or ⌥⌘0) and pan — plus a Zoom tool
  (Z: click steps in, ⌥-click out, drag a marquee to fill the window with
  it, or turn on scrubby zoom and drag left/right) and a Hand tool (H) that
  drags the view
- **Guides and rulers, the grid, and snapping** — View > Rulers (⌘R) puts rulers along the top and left edges, drawn in pixels,
  inches, centimetres, millimetres, points or percent (View > Ruler Units —
  the inch and metric scales come from the document's own ppi), with the
  ticks rescaling as you zoom and the pointer marked on both strips. Drag out
  of the corner box to move the ruler's zero point, double-click it to reset.
  Drag out of a ruler to place a **guide**; with the Move tool — or ⌘ held
  with any other tool, so painting along a guide stays painting — drag a
  guide to move it (the cursor changes as you pass over one), and drag it
  back into its ruler, or press ⌫ while it is grabbed, to delete it; View > New Guide… places one by
  typing a position in the current unit, with Show Guides (⌘;), Lock Guides
  (⌥⌘;), Clear Guides, and a Guide Color preference. Guides are *document*
  state: they persist in `.doctored`, undo and redo as their own steps, and ride
  along with crop, Canvas Size, Image Size, rotate and flip exactly as the
  pixels do — a crop drops the ones it cuts away. View > Show Grid (⌘') draws
  a grid at the spacing and subdivision count you choose (View > Grid
  Spacing / Grid Subdivisions, authored in the ruler's unit), and the Pixel
  Grid option on the Zoom/Hand tool's bar (Auto / On / Off) draws the
  one-pixel lattice once the zoom is high enough for it to read as a grid
  rather than a wash. **Snap** (⇧⌘;) pulls every drag in the app — moving a
  layer, each Free Transform handle and the whole box, the crop box, the
  shape tools, the three marquees, and a guide itself — onto
  whichever of Guides, Grid, Layers, Document Bounds and Selection (View >
  Snap To, each independently toggleable, plus All and None) is nearest
  within **8 screen points**, so the pull is the same distance on screen at
  any zoom — one canvas pixel at 800 %, thirty-two at 25 %; hold ⌃ to suspend
  it, and let go to have it back without releasing the mouse. Moving a layer
  also shows **Smart Guides**:
  alignment lines to the nearby layers' edges and centres (a group's bounds
  are its whole subtree's) and equal-spacing bars, in the guide colour's
  accent
- Rotate 90°/180°, flip horizontal/vertical, and **Image > Rotate Arbitrary…**
  — turn the whole picture by any angle at all, with the canvas GROWING to hold
  the turned image (the turned box's width and height
  rounded up, with the old canvas centred and its offset floored). Type an angle, pick Clockwise or Counterclockwise and a
  resampling filter, and the sheet shows the new canvas size before you commit;
  the corners the picture no longer covers come out transparent. Every layer,
  every mask and every saved alpha channel turn with it, the guides and the
  ruler origin shift with the picture (one that leaves the new canvas is
  dropped). Text and shape layers regenerate from editable descriptions.
  An exact quarter turn is LOSSLESS and
  takes the quarter-turn path instead, so those layers stay editable and the
  print resolution's axes swap; any other angle leaves the resolution alone,
  and a full turn changes nothing and adds no undo step. Filtered rotation
  works at 8, 16 and 32 bits (a Bitmap document takes only the quarter turns),
  and MCP `rotate` (any finite `degrees` plus a `sampler`) and a recorded
  Action step do the same. After a recorded arbitrary rotation, later Action
  steps in `relative` or `anchor` geometry mode are remapped against the
  EXPANDED canvas, exactly as after any canvas-changing step — but the recorded
  space is no longer an axis-aligned copy of the original picture, so on a
  different-size target those rectangles do not follow the rotated content
- **Free Transform** (Layer > Free Transform, ⌘T): rotate, scale and move the
  active layer or selected group/set in one session — drag the eight handles to scale (Shift keeps
  the proportions, Option grows about the pivot), drag just outside a corner to
  rotate (Shift snaps to 15°), drag inside to move, arrow keys nudge; Return or
  a double-click commits, Escape cancels. The options bar shows editable
  Angle / Scale X / Scale Y and W / H — the layer's own scaled pixel size,
  bound to the matrix both ways, so typing a width sets the scale (keeping
  a mirrored layer mirrored) — plus the resampling filter
  (nearest, bilinear, bicubic by default, or Lanczos). The drag is a live
  preview and the whole session commits as **one undo step** — the layer is
  resampled exactly once, in premultiplied alpha so rotated edges stay clean
  instead of fringing dark. A linked mask transforms with its layer, and whole-
  pixel moves and mirrors copy pixels losslessly. **⌘-drag a corner** to pull
  that corner alone — distort/perspective: the box becomes an arbitrary convex
  quad (drags that would fold it simply stop), the live preview warps with it,
  and the commit resamples through a true projective homography, mask riding
  along when linked; rotating, scaling and moving carry a warped box rigidly, and a box
  whose corners are pulled back to a parallelogram commits as the exact affine
  it is, lossless fast paths included. Text and shape layers do not rasterize
  for an affine transform: the session's matrix composes
  into the layer's description and the layer re-renders through it — glyphs
  and paths drawn under the matrix, so a rotated headline keeps crisp vector
  edges and rotating it back is lossless — with its mask and layer style
  riding along. Only the ⌘-corner perspective drag still asks to rasterize. A text
  layer whose font is not installed refuses preservation rather than silently
  substituting a face. With a selection active, ⌘T transforms the SELECTED
  PIXELS of the active layer instead, at 8 and 16 bits: the box is the
  float's opaque bounds (the pixels with alpha above
  zero under the selection, measured in a short cancellable wait; none
  refuses), the pixels × the selection resample once as premultiplied color through the
  box's affine or perspective map, land source-over (off-canvas landing
  discarded, the layer mask stays put) and leave a transparent hole (there is
  no Background fill), and the selection is transformed with the same map to
  become the new selection — one "Transform Selected Pixels" undo step,
  previewed on a background worker; the commit computes the transformed
  selection in the same background job as the pixels, behind one
  cancellable wait (at 16 bits that job also prepares the display, and
  both depths plan over the selection rather than the whole layer). One layer only, the active one: a
  multi-layer panel selection refuses by name, links are ignored, Lock Pixels /
  Lock All refuse while Lock Transparency / Position allow it, and a 32-bit float document transforms
  the layer's f32 pixels, each landed RGB clamped to the floated pixels' own
  visible range.
  MCP `transform_selected_pixels` is the twin. Layer ▸ Free Transform a
  Copy (⌥⌘T) is the same session over a copy: no hole, the original
  stays, one "Transform Copy" step (`transform_selected_pixels` takes
  `duplicate`). With a LAYER MASK or ALPHA CHANNEL as the edit target, ⌘T and
  ⌥⌘T with a selection transform that plane's selected area instead, at 8,
  16 and 32-bit float: the box is the
  selection's bounds (every selected plane pixel floats), the coverage floats
  as premultiplied (value × coverage, coverage) through the same resample and
  sampler, so a feathered edge carries through and Nearest stays hard, and
  lands back as `lerp(plane, value, coverage)`; the vacated area takes the
  BACKGROUND swatch as coverage gray, and ⌥⌘T leaves none. Off-canvas
  landing is discarded; a
  channel stays canvas-sized, a layer or adjustment mask's stored rect grows
  to hold what lands (its other properties unchanged), and a group's mask is
  clipped to its stored rect. A mask edit refuses only under Lock All, a
  channel has no lock, the layer's pixels and description stay, and Quick
  Mask refuses; a colour plane (Red, Green, Blue or Alpha) transforms that
  one channel of the layer's selected pixels, its vacated area as the Move's;
  ⌘T with no selection still transforms
  a whole mask as before. `transform_selected_pixels` and `transform_again`
  take `target` (`"mask"` or `"channel:<name>"`) and `background` (a hex,
  default `#000000`), and both record them. With no selection it duplicates the selected layers and
  opens Free Transform on the copies, over the original, which stays in
  place; Return is one "Transform Copy" step with the copies selected, Esc
  or an unchanged box leaves no copy, the box snaps to its source, and
  `transform_layers` takes `duplicate` (the reply adds `copies`). A position
  lock on an original's subtree refuses; a text, shape or
  artwork copy takes ⌘T's own rules — at 8 bits one copy under an affine
  stays editable with no prompt, a true perspective or a set asks before
  rasterizing, and at 16 bits the copies keep their artwork under an affine;
  at 16 bits it takes one entry. Layer ▸
  Transform Again (⇧⌘T) repeats the last committed Free Transform
  verbatim — the same canvas-space matrix and pivot, so a 15° turn repeated
  becomes 30° about the same point — on ⌘T's target as one "Transform Again"
  step, and Transform Again with Copy (⌥⇧⌘T) repeats it on a copy — over a
  selection on the targeted mask or channel when that is the edit target
  (the memory stores no target, and MCP names it); a true
  perspective is remembered as its corners and repeats as the same
  canvas-space perspective, rasterizing editable artwork after one prompt.
  The memory is one for the application and never saved, undo
  never changes it, and it repeats only in a document whose canvas is the
  size it was committed on, so a Crop or Canvas Size makes it unavailable
  until the size is back, and MCP
  `transform_again` is the twin. Every 16-bit
  Move, ⌘T, ⌥⌘T and Crop commit and every 8-bit layer transform shows its wait
  panel only after 0.25 s. At 16 bits, raster, group and multi-layer affine and perspective
  handles use one native set request and one commit. Linked masks follow;
  independently placed masks keep their own position. RAW masters
  remain bound through both map types. True perspective of editable text or
  shapes requires explicit rasterization, while a parallelogram stays editable.
  A lone 16-bit text or shape layer keeps its description under an affine or
  parallelogram ⌘T too. A RAW layer
  in a ⌘T or ⇧⌘T set is never asked to rasterize, at either depth. A true
  perspective over a set asks once: at 16 bits for the whole set, at 8 bits
  over its text and shape layers. A confirmed
  lone true perspective records `distort_layer` with `rasterize_artwork`, so
  its replay is not refused.
  MCP `transform_layers` and recorded Actions use the same set geometry.
  A 16-bit ⌘T, Distort, Transform Again or set transform of a 24 MP
  (6000×4000) layer commits at the default Catmull-Rom sampler: the commit is
  a one-shot job bounded by the destructive-filter budget, and its Cancel
  stops the resample within a fraction of a second. The live preview keeps
  the interactive budget, so at that size it renders at reduced resolution
  and says so ("Preview current (reduced resolution; the commit renders at
  full size)") while Return commits at full size. A 24 MP 16-bit layer
  transforms and turns, and the result saves as `.doctored` and reopens;
  a RAW-backed one transforms too. A reopen decodes each stored 16-bit
  plane in row bands straight into tiles, so a
  two-layer 24 MP 16-bit document — a background plus a placed photo
  transformed by 17° — saves and reopens, and a single-layer 41 MP
  (7842×5228) placed photo with ordinary metadata transforms and turns by 17°.
  Image ▸ Rotate Arbitrary… of a 24 MP 16-bit File ▸ New
  background plus a placed 24 MP photo is admitted too, and its result
  saves and reopens. A second turn of that result and its
  `.doctored` save are admitted at the 8 GiB limit a 16 GB Mac gets (measured), the
  saved file is within its own plan's encoded bound, and reopening that result
  is admitted: the core's reopen is covered 1.25×, and the 16-bit display that
  follows it is charged with a margin, 1.5× the reopen's measured peak.
  Float Free Transform, Free Transform a Copy and Transform Again work on
  layers with no selection; with a selection they work on a targeted mask or
  alpha channel and on the layer's pixels at float.
- **Warp** (Layer > Warp…): a 2–16 by 2–16 editable displacement lattice
  over one raster or one mask. Set, Remove Warp and Bake Warp are available at
  8 and 16 bits through the numeric sheet, `get_warp`/`warp_layer` and Actions.
  A pixel warp carries its linked mask, including a disabled one; an unlinked
  mask stays put, and a mask-only warp works on raster, group and adjustment
  masks. Identity changes nothing, integer translation keeps original sample
  buffers, and bilinear or bicubic sampling retains the pre-warp source for
  later re-edit. Native `.doctored` saves preserve the field and its original codes;
  flat saves and PSD export disclose that they keep only rendered pixels.
  Bake the field before another geometry or paint edit on its layer. RAW
  layers and editable artwork need explicit rasterization before a
  pixel warp. In a 32-bit float document a warp resamples premultiplied
  extended-range samples, clamped to the layer's own visible range, and is
  applied at once: there is no field to reopen, remove or bake (`get_warp`
  answers null), so each Apply warps the current pixels again and Undo
  adjusts or removes it.
- **Puppet Warp** (Edit ▸ Puppet Warp): an on-canvas
  session that lays a triangle mesh over the active layer's opaque pixels, or
  over the selection's pixels, which float and leave a hole where they were.
  Click adds a pin, dragging one deforms the layer live, Option-click removes
  one and Shift-click selects several; the rest of the mesh follows as rigidly
  as it can (as-rigid-as-possible). The options bar holds Mode (Rigid, Normal
  or Distort), Density, Expansion, Show Mesh, Pin Depth (which part draws on
  top where the mesh overlaps itself), Rotate (Auto, or Fixed by Option-dragging
  near a pin) and Remove All Pins. Return commits one "Puppet Warp" undo step
  and Esc cancels; a text or shape layer asks to rasterize first.
- **Perspective Warp** (Edit ▸ Perspective Warp): in Layout mode
  you drag out quads over the picture's planes, a building face or a table
  top, and a quad dropped near another's edge snaps to it and shares that edge;
  in Warp mode you drag their corner pins. Each plane is resampled through its
  own homography and joined planes stay continuous along their shared edges,
  while the rest of the layer follows as rigidly as it can. Shift-click
  straightens one edge exactly, and Straighten Vertical, Level Horizontal and
  Straighten Both do every edge; Remove Warp resets the pins, Return commits
  one "Perspective Warp" undo step and Esc cancels. With a selection only its
  pixels float and warp. Both warps resample once (Catmull-Rom), run at 8, 16
  and 32 bits, are recorded by Actions (the pins or the resolved quads, so a
  replay gives the same result) and are MCP `puppet_warp` and
  `perspective_warp`.
- **Crop** (Image > Crop): the canvas shrinks to the selection's
  bounds and layers keep their pixels outside it, ready to be revealed
  again. Available only while the selection covers less than the whole
  image — anything else would be a no-op. The Crop tool's box can reach
  past the canvas instead, and MCP `crop` takes a window past any edge with
  a `fill` (see **Crop tool**)
- Image Size (scale with filter choice, up to 100 MP). Its shared Dimensions
  selector accepts Pixels or Percent; percentages always use the pixel size
  captured when the sheet opened, not the pending size, and each axis rounds
  to the nearest integer pixel with halfway values away from zero. The result
  row shows the resolved pixels (and achieved percentages after rounding),
  while Actions record those resolved width/height values so replay is exact,
  and Canvas Size with a 3×3 anchor selector — grow or
  trim the canvas without scaling (a centred anchor floors an odd offset);
  layers keep their pixels and can be
  revealed again later. Image > Trim… crops inward past transparent pixels or
  a border matching the top-left or bottom-right pixel, edge by edge, and
  Image > Reveal All grows the canvas until it holds every layer's pixels;
  neither resamples anything (`trim`, `reveal_canvas`). Canvas Size works on an 8-bit, a 16-bit and a 32-bit
  float document alike: it reads no sample, so every layer, mask,
  saved channel and guide simply moves by the anchor's offset. Applying it at
  the size the canvas already is changes NOTHING and says so: the
  sheet simply closes, the `canvas_size` tool answers `changed: false` with a
  note, and neither adds an undo step, dirties the document or records an
  Action step — at 8 bits, at
  16 bits and at 32-bit float alike, as for an exact rotate or flip whose
  stored result is identical. That is what lets a recorded "normalize the
  canvas" step replay on a document that is already the right size and a
  batch keep going over a folder of mixed sizes. A 32-bit float document also
  accepts the other EXACT canvas ops — a zero-angle crop, the quarter turns and
  the flips, which copy samples rather than combining them. Image Size,
  straightening, arbitrary canvas rotation and Free Transform resample at float
  too, clamping each layer's RGB to its own visible range. A 24 MP (6000×4000)
  16-bit photo straightens and rotates by any angle with the default
  Catmull-Rom sampler: the filtered turn is a one-shot job, so its pixel work
  is bounded by the destructive-filter budget rather than the interactive one
- Image > Adjustments: the destructive twins of every adjustment layer
  above, Curves included (the same curve editor, recorded as the
  `apply_filter` curves step), with live in-context preview on the active
  layer — Levels… is ⌘L (the adjustment layer's dialog, channel stages and
  output levels included, running the same core op and recorded as the flat
  `apply_filter` levels step); Curves… is ⌘M, Hue/Saturation… ⌘U, Color
  Balance… ⌘B and Black & White… ⌥⇧⌘B — and Auto Tone,
  Auto Contrast (⇧⌘L, ⌥⇧⌘L) and Auto Color (⇧⌘B), which derive Levels
  parameters from the image's own histogram (clipping 0.1 % at each end by
  default, the white point's clip counted jointly across the channels) and
  apply them through that same levels math.
  Desaturate (⇧⌘U) is Hue/Saturation with the saturation taken all the way
  down, and records as that step.
  A Levels adjustment layer's dialog (new and Options…) has Auto Tone and
  Auto Color too:
  the same math, measured over the BACKDROP below the layer rather than the
  composite that already carries the correction. The whole visible backdrop
  counts even when the layer is clipped or sits in an isolated group
  (measured against Photoshop 26.10.0), and a live selection does not narrow
  an existing layer's Auto; only the layer's mask does — the selection a new
  layer takes as its mask, or an existing layer's own mask. The result replaces the Red,
  Green and Blue stages (the RGB stage resets), and Apply commits it as one
  undo step. A layer with nothing visible below it beeps and says so; the
  buttons are disabled at 32-bit float. Over MCP, `add_adjustment_layer` and
  `edit_adjustment_layer` take `auto` {mode `tone` or `color`, optional
  `clip`} instead of `params`; the call applies and records the resolved
  params, so a replayed Action reproduces the numbers.
  ⌥-dragging Levels Black or White (the dialog and the adjustment-layer pane)
  or a Curves end point shows a clipping display on the canvas —
  clipped highlights lit on black, clipped shadows dropped on white, exact
  0/65535 at 16 bits — and every Levels and Curves sheet draws a live "After"
  histogram of the adjusted composite under its input plot (both hidden
  for a mask or channel target and while a channel view replaces the
  composite; at 32-bit float the display lights a value at or above 1.0 and
  drops one at or below 0.0). View ▸ Show
  Clipping (⌥⌘J) draws Develop's clipping markers over the canvas — blue at 0, red at the maximum, magenta for both, exact at
  16 bits — and follows a dialog's preview; it is an app-wide view preference
  with no MCP tool, and is off in 32-bit float documents. Destructive
  filters and adjustments write through the active selection's coverage on
  layer pixels, masks, color planes and saved channels in both depths. Neighborhood filters sample the full target; automatic tonal analysis
  defaults to the whole target and can explicitly weight the selection.
  Sheet previews and commits share captured inputs and refuse stale edits
- Filters: Gaussian blur, sharpen, **Unsharp Mask**, **High Pass**, Pixelate,
  Add Noise, Edge Detect, Emboss, **Box Blur**, **Surface Blur**, **Add
  Grain**, **Radial Blur**, **Clouds**, **Difference Clouds** and **Offset**,
  and **Motion Blur**, **Lens Blur** and **Tilt-Shift**, grouped
  into Filters submenus (Blur, Noise, Other, Pixelate, Render,
  Sharpen, Stylize), with **Liquify**, **Lens Correction** and **Presence**
  in the Filters menu itself. Box Blur is the plain mean of a square window and filters alpha as
  Gaussian Blur does; Surface Blur smooths each channel across neighbours
  within its threshold (in 8-bit levels at both depths) and leaves edges of
  2.5× the threshold alone; Add Grain adds positional, luminance-weighted grain
  (Amount, Size, Roughness) that never touches black or white; RAW Develop's
  Grain uses the same kernel. All three run
  at 8 and 16 bits, on selections, masks, colour planes and channels, record
  into Actions and repeat with ⌃F. Radial Blur… spins (Amount in degrees) or
  zooms (Amount × 0.5% of the radius) about a centre you click on the image,
  at Draft (grainy, the same grain every time), Good or Best; Filters ▸ Render
  ▸ Clouds and Difference Clouds mix the foreground and background swatches
  with a fresh seed and no dialog (⌥ for high contrast; ⌃F renders new clouds,
  a recorded Action replays the same ones); Offset… moves the picture right and
  down with Wrap Around, Repeat Edge Pixels, or the vacated area left
  transparent on a layer and filled with the background colour on a mask
  (white), colour plane or channel. With a selection, Offset shifts and wraps
  within the selection's bounding box and Radial Blur's centre is placed in
  it. These four run at 8 and 16 bits on the same targets, record
  into Actions and repeat with ⌃F; `apply_filter` takes them. A 24 MP layer
  takes Clouds, Offset and Radial Blur at Draft. Radial Blur at
  Good (about 21 s at 8 bits and 43 s at 16 for Amount 10 at 24 MP, measured)
  and Best, and Surface Blur at the top threshold (255), have no size limit:
  their Apply runs as a cancellable job, so they are admitted by working
  memory alone. Filters ▸ Blur ▸ Motion Blur… (an angle and a distance of 1
  to 2000 px) is the equal-weight mean along a line; Lens Blur… (radius 1 to
  100, brightness, threshold) is a disc mean that weights highlights above
  the threshold up; Tilt-Shift… is the same disc with a radius that grows
  with the distance from a focus line (centre, angle, focus, transition).
  Each has a preview, progress and Cancel, runs at 8 and 16 bits and on masks
  and channels, and `apply_filter` takes `motion_blur`, `lens_blur` and
  `tilt_shift`. Filters ▸ Liquify… (⇧⌘X) opens a modal workspace on the
  active layer: Forward Warp, Pucker, Bloat, Twirl in both directions,
  Reconstruct, and Freeze / Thaw, which paint a mask that protects pixels. The
  brush has Size, Strength and Density, ⌥ reverses the tool and holding still
  keeps applying; Undo Stroke and Restore All work inside the workspace, Show
  Mesh and Show Freeze draw overlays, a selection confines the result, Cancel
  discards and Apply commits one undo step as a cancellable job, at 8, 16 and
  32 bits. The warp is a displacement grid of at most 2048 nodes on the long
  side, and a large layer previews on a reduced proxy with the same grid and
  strokes as the commit. MCP `liquify` takes an explicit stroke list (tool,
  points in canvas pixels, size, strength, density) of up to 2048 strokes and
  131,072 points, and Actions record it; `apply_filter` refuses liquify.
  Face-Aware Liquify, saved meshes, Liquify on a mask or channel and stylus
  pressure are not built. Filters ▸ Lens Correction… corrects Distortion,
  Red/Cyan and Blue/Yellow fringe, Vignette (amount and midpoint) and vertical
  and horizontal perspective with an angle and a scale, with a live preview
  and a Show Grid overlay; what the corrected picture does not cover is
  transparent. Its sliders are set by hand, and its Lens Profile popup adds a
  bundled Lensfun profile (Lensfun database v0.3.4, CC BY-SA 3.0): Auto, the
  match for the document's EXIF camera and lens, or a lens chosen from the
  camera's mount, whose distortion, fringe and vignetting are corrected after
  the sliders in the same resample. It runs at 8, 16 and 32 bits as a
  cancellable job; `apply_filter` takes `lens_correction` with `profile`
  (`auto`, `none` or a lens id from `list_lens_profiles`) and Actions record it. Filters ▸ Presence… adds Texture,
  Clarity and Dehaze, each −100 to 100 with 0 changing nothing: Texture and
  Clarity scale a fine and a coarser band of detail over an edge-aware base,
  Clarity weighted toward the midtones, so a strong edge gets no halo and the
  ends of the range do not clip; Dehaze estimates the haze's colour and its
  veil and removes it, and a negative amount adds haze. It has a live preview,
  runs at 8 and 16 bits on the active layer or plane as a cancellable job,
  confined by the selection, and `apply_filter` takes `presence` with
  `texture`, `clarity` and `dehaze`; a 32-bit float document runs all three,
  and the Develop RAW window has the same three sliders. Filters ▸ Develop
  Filter… (⇧⌘A) applies Develop's global controls — Temperature (a
  relative shift from 6504 K, since an ordinary layer has no as-shot white),
  Tint, Exposure, Contrast, Highlights, Shadows, Whites, Blacks, Texture,
  Clarity, Dehaze, Vibrance and Saturation — to an 8- or 16-bit layer as one
  destructive filter with a live preview, selection confinement, a cancellable
  job and Repeat Last Filter; it has its own presets and reads Develop presets'
  shared controls, `apply_filter` takes `develop_filter`, and a 32-bit float
  document runs every control. Its **Masks** section adds up to 16 local
  adjustments — Radial, Linear, Brush, Range, Subject, People and Sky — each
  with Temperature, Tint, Exposure, Contrast, Highlights, Shadows, Whites,
  Blacks, Texture, Clarity, Dehaze, Saturation, Sharpness (−100…100) and
  Noise Reduction (0…100) plus density, feather and
  invert, at 8 and 16 bits (`apply_filter` `develop_filter` `masks`; a replay
  reruns the Subject, People and Sky models, and a 32-bit layer refuses
  masks by name).
  On a canvas
  above 4 MP, at 8 and 16 bits, the Gaussian Blur, Box
  Blur, Surface Blur, Unsharp Mask, High Pass, Reduce Noise and Dust &
  Scratches sheets
  preview a downscaled proxy first and the exact full-resolution result after
  it, and a slider change cancels the exact render it supersedes; in a 32-bit
  float document all seven preview the full-resolution result only. Apply
  always runs the full-resolution filter. At 16 bits a 24 MP layer's Surface Blur and
  Gaussian Blur sheets preview, proxy then exact, and Apply (previews measured
  at Surface Blur r5–r10 and σ2, Apply at r5 T15 and σ2).
  Reduce Noise keeps its window statistics in row rings rather than whole planes, with
  its output unchanged, so a 24 MP layer takes it at 8 and 16 bits: the sheets
  preview and the 16-bit Apply lands (measured at radius 1–2), and
  `apply_filter` takes it too. Reduce Noise's and Dust & Scratches' work
  charges are measured; Dust & Scratches takes any radius, admitted by
  working memory. The seven
  8-bit filters that run through one core call (Box Blur, Surface Blur, Add
  Grain, Unsharp Mask, High Pass, Dust & Scratches and Reduce Noise) are
  planned by the core and admitted against the working limit before they
  allocate: the sheet's full-resolution preview, Apply, `apply_filter`, and
  the replayed Actions and ⌃F that go through it. One that does not fit is
  refused with a sentence (on the sheet's status line, or Apply's alert)
  instead of the preview silently clearing; outputs are unchanged. Box Blur
  holds one channel's sum at a time (24 bytes a pixel), and its plan is
  that stage's own, 24 bytes a pixel plus a quarter. The commit and redraw
  after any 8-bit edit (the result, the written layer and the first
  projection) are charged as their own stage, by the filters' Apply and by
  every 8-bit publication, at 1.25× or more of the measured peak. A 45 MP
  filter commit (Box Blur, Invert, any of them) therefore needs 2.31 GB
  beside the photo and is admitted at a 16 GB Mac's 8 GiB limit.
  Dust & Scratches at radius 2 and Surface Blur at radius 5, threshold 15 run
  there too, admitted by memory alone, and the other four take their
  defaults there.
  Apply and `apply_filter` for those seven run in the background with a progress bar and
  Cancel (Esc), and the app stays responsive while they run; they wait for
  the worker and are refused, with the sheet left open, while another
  background job holds it. The 8-bit
  Gaussian Blur is planned and run the same way, as a cancellable job with
  progress, and every 8-bit filter sheet's Apply does its layer write and
  first redraw on that job rather than on the main thread.
  From radius 7 up, Gaussian Blur at every depth is four
  extended box passes at σ = 0.99 × radius, within one 8-bit level of
  Photoshop 26.10.0's own renders at every measured radius from 9 to 250; its cost does
  not grow with the radius either. From σ4 to 7 (and for the legacy Sharpen's
  blur) both depths run a recursive Gaussian whose cost does not grow with the
  radius, so a 24 MP 16-bit layer takes every sigma on the slider, up to 64.
  The 8-bit Gaussian Blur takes sigma up to 1000; `apply_filter` refuses more by name
  rather than blur less than asked. Saved Actions
  and recordings that replay `blur` take the same paths, and one whose 8-bit
  `blur` step asks for more than σ1000 stops with that refusal. Unsharp Mask has independent radius, amount
  and threshold; High Pass takes a radius and returns the detail around neutral
  grey. Both write colour only, so alpha
  stays exact, and both honour the active selection like every other
  destructive filter. Native 16-bit destructive tone and spatial filters preserve
  exact samples outside coverage. Unsharp Mask, High Pass and export output
  sharpening run the recursive Gaussian from radius 4 up at 8 and 16 bits (a
  32-bit float layer runs the windowed Gaussian at every radius).
  Levels, Threshold and Curves offer normalized/native-code fine entry; new
  Actions retain those full numeric values.
- **Files from earlier builds**: Doctored was called Rasterize. It opens and
  edits Rasterize's `.rz` documents (Save on one asks for a `.doctored` name
  first) and its `.rzsnapshot`, `.rzaction`, `.rzmask` and `.rzcb` files,
  while new files take the `.doctored`, `.drsnapshot`, `.draction`, `.drmask`
  and `.drcb` names; a Rasterize build cannot open a `.doctored` file. The
  first ordinary launch moves `~/Library/Application Support/Rasterize` to
  `…/Doctored` when the new folder holds nothing yet, and copies Rasterize's
  preferences once, leaving the old ones in place. A `.rz` saved with a smart object, a live
  filter stack or a placed image that kept its original opens with that
  layer as ordinary pixels, exactly as it last looked; those features are
  not in this version
  ([record](https://github.com/kpgalligan/doctored/blob/main/designs/convert-to-smart-object-status.md)).
- **Smart Sharpen** (Filters ▸ Sharpen ▸ Smart Sharpen…): Amount, Radius,
  Reduce Noise, Remove (Gaussian Blur, Lens Blur, or Motion Blur with an
  Angle) and the Shadows and Highlights fades, at 8, 16 and 32 bits, with a
  live preview, confined by the selection, recorded by Actions, repeated by
  ⌃F, and over MCP as `apply_filter` `smart_sharpen`. Lens Blur removal holds
  each colour within two levels of its neighbourhood's range, so an edge
  sharpens without the halo Unsharp Mask draws. It is Doctored's own
  algorithm, not measured against Photoshop.
  The export sheet and `export_image` offer it as an output-sharpening method
  beside Unsharp Mask, and export presets keep it.
- **Depth Blur** (Filters ▸ Neural Filters ▸ Depth Blur…): a local depth
  model, Depth Anything V2 Small running through Core ML, estimates the layer's
  depth once, and pixels away from the focus are blurred with Lens Blur's disc
  at a radius that grows with their distance from the focal plane. The focus is
  the subject Vision finds (Focus Subject), a point you click, or a focal
  distance, with a focal range kept sharp; Haze, Warmth, Brightness,
  Saturation and Grain finish it, and Depth Map Only shows the map itself. It
  writes to the current layer or a new one, confined by the selection, at 8,
  16 and 32 bits (in linear light) as a cancellable job with a preview;
  Actions record the settings, so a replay runs the model again, and MCP has
  `depth_blur`. Nothing is downloaded until you ask: **Settings ▸ Models**
  lists the models, shows each one's licence before its first download, and
  downloads it resumably from the `kpgalligan/doctored-models` releases,
  checked against a pinned SHA-256, into
  `~/Library/Application Support/Doctored/Models` (a model a Rasterize build
  installed there stays ready where it is); MCP `list_models` reports
  them read-only. Further neural features follow the plan in
  [`designs/neural-models.md`](https://github.com/kpgalligan/doctored/blob/main/designs/neural-models.md).
- **Super Zoom**, **JPEG Artifacts Removal** and **Colorize** (Filters ▸
  Neural Filters)
  run local Core ML models from Settings ▸ Models tile by tile, stitched so a
  tiled run equals a one-tile run, at 8 and 16 bits (32 bits and CMYK refuse
  by name, and a missing model is refused with its download offered). Super
  Zoom enlarges a layer ×2 or ×4 into a new document with Real-ESRGAN
  (Enhance Image Details; off, a classical resample), optionally removing
  JPEG artifacts, reducing noise and sharpening, and refuses a result past
  100 megapixels; JPEG Artifacts Removal (Low, Medium or High; FBCNN)
  rewrites the layer or writes a new one; Colorize colours a layer from its
  luminance with DDColor, keeping its lightness exactly, with Saturation,
  three colour-balance sliders and Color Artifact Reduction, to the layer, a
  new layer or a new document. Actions record them, and MCP has
  `super_zoom`, `jpeg_artifacts_removal` and `neural_colorize`.
- **Sky Replacement** (Edit ▸ Sky Replacement…): the sky
  model's mask swaps in a sky from a downloadable CC0 library (Settings ▸
  Models) or one of your own, with its controls — Shift Edge, Fade
  Edge, Brightness, Temperature, Scale, Flip, the sky dragged in the preview,
  Lighting Mode, Lighting and Color Adjustment and Edge Lighting — over a live
  preview, as one undo step, at 8 and 16 bits (32 bits and CMYK refuse by
  name). In its Output To popup,
  New Layers, the default, puts a Pass Through "Sky Replacement Group" above
  the source holding "Foreground Color" and "Foreground Lighting" Curves
  layers and the masked "Sky" layer, each part editable and kept in `.doctored` and
  PSD at both depths; Duplicate Layer writes one flattened "Sky Replacement"
  layer instead. Decontaminate Edge, on by default, takes the old
  sky's colour out of the horizon's edge pixels so a dark sky shows no light
  halo; with New Layers it adds a "Foreground Edge" layer, masked to the
  edge, at the bottom of the group. Actions record it, and MCP has
  `sky_replacement` (with `output` and `decontaminate`) and `list_skies`.
- **Generative Fill** (Edit ▸ Generative Fill… and Edit ▸ Generative
  Expand…) sends
  the selection, or the transparent area Canvas Size added, to OpenAI, Gemini
  or fal with your own key (Settings ▸ Cloud; a consent card shows each
  provider's terms at first use). The result lands on a new masked
  "generated" layer above the anchor, at 8 and 16 bits and on 8-bit CMYK
  (32 bits and 16-bit CMYK refuse by name); pixels outside the selection do
  not change. Each run keeps three variations on the layer, saved in `.doctored`
  and switched with Layer ▸ Generative Variations… (a provider that returns
  fewer is reported with its reason: an alert, and the MCP reply's
  `variations_received` and `short_reason`); PSD export writes the
  chosen one. It is never recorded into an Action, so a replay never bills a
  key. MCP has `generative_fill`, `generative_expand`,
  `generative_variations`, `select_generative_variation` and
  `generate_variations`; the ones that call a provider are refused unless
  Settings ▸ Cloud's agent switch (off by default) is on.
- A **Navigator panel** (View > Navigator, ⌃⌘N) — the whole composite as a
  thumbnail with a proxy rectangle over what the canvas well is showing: drag
  inside the rectangle to pan, click outside it to centre there, and a log-scaled
  zoom slider, a percentage field and Fit beside it. It is VIEW state and
  nothing else: no pixel changes, no undo step, no dirty flag, nothing a Save a
  Copy would carry. The right panel's edge is **draggable** between 240 and
  560 pt, and that width is an app-wide preference every open window follows —
  and one a saved workspace carries.
- **View > Rotate View…** turns the VIEW to any angle — an angle field, a
  slider and Reset, an ⌥-drag with the Hand tool, or the trackpad's two-finger
  rotate — so you can paint along a slanted edge without cocking your wrist.
  Positive is clockwise, and **no pixel moves**: the render is byte-identical
  before and after, nothing is added to the history and the document is not
  marked edited. Every tool keeps working in document space through the turn;
  the grid, the guides and the marquee rotate with the picture, and the ruler
  strips hide while a view is turned (the preference itself is untouched, so
  **View > Reset View Rotation** (⌃⌥⌘R) brings them straight back).
- **Window > New View for “&lt;document>”** opens a second window on the same
  document — a 100 % detail beside a fit overview. One picture, one history,
  one undo stack, and an edit in either window appears in both immediately;
  each window keeps its own zoom, scroll and view rotation, while the layer
  selection, the paint target and the channel eyes are document state and
  follow in both. **Window > Sync Pan & Zoom** makes a zoom or a pan in one
  window happen in the other (rotation stays each window's own). Closing a
  second view neither closes nor dirties the document; closing the document
  closes every view with one save prompt.
- **Window > Workspaces** — save the window frame, the right panel's
  visibility, tab and width, the rulers/grid/snap toggles and the current tool
  under a name, and put them back from the same menu. Deliberately NOT saved:
  the document's zoom and scroll position, which belong to the picture rather
  than to the layout. A frame saved on a larger or second display comes back
  reachable, and a tool the document's bit depth refuses is skipped rather than
  beeped. A file in the workspace folder that is not a readable layout appears
  under Delete Workspace ▸ **Delete Broken File** with its reason. With no
  workspace involved, a new editor window opens at the last editor window's
  frame — stepped off an open window at the same origin and fitted onto a
  connected display — with the right panel's last visibility and tab; like
  the other view preferences it is app-wide and has no MCP tool.
- **Doctored > Settings… (⌘, or ⌘K)** opens one window with
  Shortcuts, Models (see **Depth Blur**), MCP (see **AI agent access**) and
  Cloud tabs; Cloud holds the provider keys, the consents and the agent
  switch **Generative Fill** uses. Shortcuts lists every customizable menu command with its
  key equivalent, searchable, and lets you record a new chord for any of them.
  A shortcut must contain ⌘ or be a function key — a menu key equivalent is
  resolved ahead of whatever you are typing in — and a chord another command
  already holds is refused naming that command ("⌘J is already Layer ▸ Layer Via Copy.").
  Per-row Reset and Reset All restore the shortcuts this build ships
  ([`designs/v1-shortcuts-fade.md`](https://github.com/kpgalligan/doctored/blob/main/designs/v1-shortcuts-fade.md)): the defaults are a snapshot taken at launch
  and the file on disk holds only your own changes, so a shortcut you set
  survives an update — where a new default would take a chord you gave another
  command, that default is left off. macOS's own items (Quit, Hide, Close, Minimize, Undo/Redo) and
  the menus that rebuild themselves are not offered. Edits apply immediately, to
  the menus and to the command palette's displayed shortcuts alike, and survive
  a relaunch.
- An **Info panel** (F8) with the document's histogram — per-channel or
  luminosity, with a clipping wedge at each end — over a readout that
  follows the cursor and freezes with its last value when the cursor leaves:
  position, RGB, HSB, Lab (computed through the DOCUMENT's own profile
  against the D50 white, so the same bytes read differently in an sRGB and a
  Display P3 document), the selection's bounds and area, and the document's
  size, resolution and profile. The same plot is drawn behind the Levels and
  Curves controls — showing, for an adjustment layer, the backdrop below it
  rather than the already-corrected composite. Composite, Active layer and Below
  active layer sources share a Weight by selection switch for the histogram and
  RGB readout. Fractional coverage contributes its exact weight; full-scope
  clipping counts stay separate from the 256 display bins and any plot sampling.
  Native scalar tone sheets measure the captured raw mask, channel or RGBA plane,
  including zero values and off-canvas storage; selection weights map through
  the plane's own placement. A read that fails shows "—" and the footnote
  names why, and so does the histogram ("Histogram unavailable"). The Levels
  and Curves sheets' histograms do the same, with the reason as the tooltip.
  Those sheets add a second plot, "After", the whole adjusted composite each
  preview render makes (stride 1 at 8 bits, exact u16 endpoints at 16), while
  the input plot keeps the tones the handles move.
  At 32-bit float the histogram counts in stops, with end markers for below
  zero and above white, and the readout shows linear components with stops;
  Composite, Active layer, Below active layer and Weight by selection all apply
  there as at 8 and 16 bits.
- Selections beyond the rectangle: ellipse marquee (M, shared with the
  rectangle marquee), polygonal lasso (L — click vertices,
  double-click/Return/click-the-start closes, ⌫ removes the last vertex,
  Escape cancels), and a magic wand (W) with tolerance + contiguous options
  (a pixel joins when its R, G, B and alpha, and its R − G, G − B and R − B,
  each lie within the tolerance of the seed's, a rule the
  Paint Bucket shares; its Sample Size, Point or a 3×3 … 101×101 Average,
  sets the seed's colour)
  that samples the flattened composite or, with its Sample popup on Current
  layer, the active layer alone; the ellipse marquee and both lassos take
  Anti-alias, and the selection tools' bar has a Save selection button;
  Select > Select Subject hands the
  composite to macOS's Vision segmentation — the model behind Preview's
  Copy Subject — and turns the people, animals or objects it finds into a
  selection with no seed point, no tolerance and a genuinely soft edge,
  while the Subject tool (W) does it one subject at a time: press and the
  subject under the pointer is outlined, drag between subjects to change
  which, release to select it; a marquee, freehand-lasso or Quick
  Selection drag may start on the pasteboard around the image and is
  clipped to the image, while a click there does nothing;
  combine selections with Shift (add),
  Option (subtract), or Shift+Option (intersect), invert (⇧⌘I), soften
  them with Select > Modify > Feather…, reshape them with Expand…,
  Contract…, Border…, and Smooth… — true Euclidean-distance morphology at
  the selection's 50% contour, so a moved edge comes back freshly
  anti-aliased instead of jagged — grow them by colour with Select > Grow
  (connected pixels) and Select > Similar (the whole image), which add every
  pixel whose colour lies in the selected pixels' range widened by the Magic
  Wand tolerance, or paint them directly in Quick Mask
  mode (Q — the selection becomes a red rubylith overlay the brush adds
  to and the eraser removes from, and Filters ▸ Blur ▸ Gaussian Blur… and the Gradient
  tool work on it too; toggling back out converts the buffer
  into the selection, and an empty one deselects), and the marching ants
  march (pausing when the window is not key, under Reduce Motion or Low
  Power, and above 20,000 path elements) and trace the true selection
  contour — disjoint pieces and holes each get their own
  dashed loop; selections confine brush, eraser, fill, gradient, filters and
  destructive adjustments, dim
  the outside, and define Crop. Delete (Edit > Clear, ⌫) clears the
  selected region of the active layer to transparency in one undo step
  (it stands down while a polygonal or magnetic lasso is open or a marquee,
  freehand-lasso or Quick Selection drag is in flight, so ⌫ never erases pixels
  mid-gesture),
  and does it proportionally where coverage is partial — a feathered
  selection leaves a soft-edged hole, not a hard one (text layers ask to
  rasterize first, as any destructive edit does)
- Edit > Fill… (⇧F5, or ⇧⌫) fills the selection — or the whole target when
  nothing is selected — with the foreground or background colour, a picked
  colour, black, 50% gray or white, through any layer blend mode at an
  opacity, in one undo step; Preserve Transparency keeps the layer's alpha as
  it is, exactly as Lock Transparency does, and on a semi-transparent pixel the
  colour mixes in at the fill's own opacity × coverage. It fills the target the Channels
  panel names: a layer mask or an alpha channel takes the colour's gray, and a
  colour plane 0.30 R + 0.59 G + 0.11 B of it, not weighted by the
  pixel's transparency (the four HSL
  modes need colour and are refused there). Edit >
  Fill with Foreground Color (⌥⌫) and Fill with Background Color (⌘⌫) are the
  one-keystroke Normal 100% fills, and ⇧⌥⌫ / ⇧⌘⌫ keep transparency; like Clear,
  every ⌫ chord stands down while text is being edited, a guide is grabbed or a
  selection gesture is in flight. On a text or shape layer ⌥⌫ / ⌘⌫ (and ⇧ with
  them) recolour the artwork instead — every character of the text, a shape's
  fill or a line's stroke — and it stays editable, one "Fill" undo step;
  Fill… and `fill_selection` still rasterize. `fill_selection` is the MCP twin, and a
  recorded fill replays the colour it used. In Quick Mask, Fill… and the four
  chords fill the whole Quick Mask with the colour's gray (white adds to the
  selection, black removes it) at 8, 16 and 32 bits, one undo step that leaves
  the file clean, and `fill_selection` does the same there. An agent's
  `fill_selection` recorded in Quick Mask saves `target: "quick_mask"`, like the
  menu's, so its replay outside Quick Mask refuses by name rather than filling
  the active layer. Outside Quick
  Mask, Fill… and the four chords work in 16-bit documents too, on the same
  targets, with the blend mode, opacity and Preserve Transparency; there a
  trimmed layer grows to the pixels the fill changes, as the 16-bit brush does
  (not under Preserve Transparency or Lock Transparency), and at 8 bits
  Fill…, ⌥⌫, ⌘⌫ and `fill_selection` grow a trimmed layer to its union with
  the canvas when the fill reaches past it, with the same two exceptions.
  In 32-bit float documents Fill… works on a layer, its
  mask or an alpha channel: the colour fills as linear light (diffuse white is
  1.0, so a full Normal fill shows the colour you chose under Clip to white,
  and a foreground above white fills as its own value),
  and a layer takes any of the float document's blend modes. With the Red,
  Green, Blue or Alpha channel targeted it writes one value into that channel,
  0.30 R + 0.59 G + 0.11 B of the colour, whatever the
  pixel's transparency, in Normal, Dissolve, Darken, Multiply, Lighten, Linear
  Dodge (Add), Difference or Subtract.
  Edit > Stroke…
  strokes the selection's edge with one colour, Inside, Center or Outside, 1 to
  250 px wide, through a blend mode at an opacity, in one undo step — the same
  band as Layer Style > Stroke, filled exactly like Fill…. The canvas edge
  counts as an edge, so Select All then Stroke Inside draws a frame. With
  nothing selected it strokes the true edge of the active layer's own pixels,
  including any part of the layer beyond the canvas, and grows the layer to
  hold the stroke in the same step — past the canvas too, unless
  Preserve Transparency or Lock Transparency keeps its alpha; a mask,
  colour-plane or alpha-channel target still needs a selection. It strokes the
  same targets as Fill…, and a selection's band, which stays within the
  canvas, grows a trimmed or offset layer to cover the canvas in the same step,
  so Outside and Center land at full width (Preserve Transparency and Lock
  Transparency keep the layer as it is). `stroke_selection` is the MCP twin.
  Like Fill… it works at 8, 16 and 32 bits (at 16 and 32 bits a selection's
  band grows a trimmed layer only to the pixels it changes), and in a 32-bit
  float document it follows Fill…'s rules: the float blend modes on a layer,
  and a colour channel's one value and modes.
- Selection changes, including combine/modify/load/deselect and every Quick Mask
  stroke, have undo/redo without marking the file dirty. Select > Reselect
  restores the previous selection. Canvas geometry changes (crop, canvas size,
  rotate, flip, image size, straighten) carry the selection with the canvas as
  part of the edit, and Perspective Crop clears it; undo restores the matching
  selection. Actions
  replay Quick Mask strokes from their exact coverage operations. Native selections
  preserve odd u16 codes through channel transfer, modification and history;
  Feather, Smooth, Expand, Contract, Border and Refine evaluate over the selection's
  own extent plus the edit's reach, so a 250-px feather of a small selection
  finishes on a large 16-bit canvas at full resolution;
  `get_selection` reports exact `coverage16` alongside legacy byte readouts. MCP exposes
  the same history, target policy and coverage inspection
- **Crop tool** (C): an interactive crop box over the whole canvas — drag
  the eight handles (aspect presets: Original, Free, 1:1, 4:3, 3:2, 16:9,
  plus editable W/H), move it from inside, draw a fresh one from outside,
  with a rule-of-thirds grid and a Straighten angle that live-rotates the
  image behind the fixed box; Return or a double-click commits (straighten
  rotates every layer about the box's center, then crops) as one undo
  step, Escape resets. Cropping only moves the canvas window — layer
  pixels outside it are kept and can be revealed again — unless Delete
  cropped pixels is on, which trims each plain raster layer to the new canvas
  (MCP `crop delete_cropped:true`). The box also reaches past the canvas on
  any side, turned or not, at 8 and 16 bits, at float and on CMYK, and the
  canvas grows to it. The options
  bar's Fill popup chooses what fills the new area: Background (Default)
  pads the bottom layer with the background colour only when it is a plain,
  fully opaque raster and Delete cropped pixels is on, and every other layer
  gets transparency; Generative Expand (see **Generative
  Fill**) and Content-Aware Fill fill it as one undo step with the crop, and
  Content-Aware forces Delete cropped pixels on. MCP `crop` takes a signed
  window, `background_color` and `fill` (`background`, `generative_expand`
  or `content_aware`, with `prompt`, `provider` and `variations` for
  Expand). The tool works on a
  16-bit document too: real handle drags and the Straighten control preview
  the captured native result, and Return commits the same native crop or
  straighten operation in one step.
  A 32-bit float document takes it too. Straighten (the slider, MCP `crop`
  `angle`) is ONE operation equal to rotating every layer, every mask and
  every saved channel and then cropping, at 8, 16 and 32 bits; at float it
  clamps each layer's RGB to its own visible range. An `angle` that is present
  but not a readable number (say `"abc"`) is refused with "angle must be
  between -45 and 45 degrees", and a recorded crop writes `angle` as a JSON
  number, only when it is non-zero. On a 24 MP 16-bit document the live
  preview keeps the interactive budget, so it shows a reduced-resolution
  preview, labelled as one, while Return still straightens at full size.
  While straightening, the box always re-fits inside the rotated image, so
  the corners never show transparent wedges: it keeps its aspect, shrinks
  about the handle you hold (or its centre), and returns to the box you drew
  when the angle goes back. Auto, beside
  the Straighten field, sets the angle from Vision's horizon (a clear
  horizon tilted by about 1° to 10°), and Draw line turns the next drag
  into a line along something that should be level or vertical — the angle
  that levels it is applied on release.
- **Perspective Crop** (C, sharing the Crop tool's slot): place the four
  corners of a convex quadrilateral — a sign, a painting, a page photographed
  at an angle — and Return crops the document to it squared up, resampling
  every layer, mask and channel, at 8, 16 and 32 bits. Text and shape layers
  are rasterized after a prompt, a RAW layer must be rasterized first, and the
  selection and guides are cleared. MCP `perspective_crop` is the twin.
- Fill tool (G, shared with Gradient): bucket flood fill on the active layer with tolerance,
  contiguous and opacity options, Contents (foreground or background), any
  blend mode and Sample all layers, and a
  Gradient tool (G): drag to paint a Linear, Radial, Angle, Reflected or
  Diamond gradient from the foreground to the background color (Reverse
  swaps them, Opacity fades both), or through 2 to 32 stops, each with its
  own opacity and colour and opacity midpoints, set in the bar's Edit… stop
  editor with the gradient's Smoothness (0–100 %, default 100 %), at 8, 16 and
  32 bits and on CMYK ink (MCP `gradient` takes `stops`, with `midpoint` and
  `opacity_midpoint` per stop, `smoothness` and `shape`), and through any
  blend mode from the bar's Mode popup on a layer's pixels (`blend_mode` over
  MCP; a mask, colour plane, channel or Quick Mask target takes Normal only),
  both selection-aware. Linear, Angle, Reflected and Radial gradients follow the
  Classic ramp and geometry measured against Photoshop at 8, 16 and 32 bits
  and in CMYK, in the tool, Gradient Fill layers, Gradient Overlay and Stroke
  Gradient alike, and a Shape Burst stroke follows its measured contour-distance law;
  Diamond keeps its own
- **Clone Stamp** (S): ⌥-click sets the source, then strokes stamp the
  composite from that fixed offset through round dabs — the classic
  aligned clone, live-previewed through the projection like every stroke,
  `[`/`]` resize, selections confine it. At 8, 16 and 32 bits, Sample chooses Current
  Layer, Current and Below, or All Layers; Ignore Adjustments prevents sampling
  adjustment entries above the target. Output can create a separate Retouch layer.
  Source Scale, Rotation and flips use a fractional native source transform;
  Source Overlay previews that same captured source under the pointer. Expand
  Destination explicitly adds transparent raster storage up to the canvas while
  preserving existing pixels and mask placement
- **Dodge / Burn** (O): brush-local tonal retouch — dodge brightens, burn
  darkens, banded to shadows / midtones / highlights with an exposure
  setting, applied by a Rust core op through the stroke's own coverage so
  soft edges fade the effect out, at 8, 16 and 32 bits, and at 16 and 32
  bits on a mask or alpha channel too (MCP `dodge_burn` takes a `target`)
- **Blur, Sharpen, Smudge, Sponge and History Brush**: five more
  retouch brushes with size, hardness and Strength in the options bar (Sponge
  adds a Desaturate / Saturate Mode, the History Brush uses Opacity), at 8 and
  16 bits, and on a 32-bit float layer too, in linear light
  keeping values above white. R cycles Blur, Sharpen and Smudge, O cycles Dodge / Burn and
  Sponge, and Y is the History Brush. Blur mixes each covered pixel toward a
  Gaussian of the layer, Sharpen toward an unsharp mask and Sponge toward or
  away from its luma (with Vibrance, on by default, toward the Vibrance
  adjustment's target), each by coverage × strength and never writing alpha —
  except that Blur and Sharpen with Sample All Layers read the picture of
  every visible layer, those above it included, and mix the layer toward it,
  alpha included;
  Smudge drags colour along the stroke in steps of at most one pixel, so the
  result does not depend on Spacing and leaves no gaps. The History Brush
  paints a layer back toward what it held in a chosen history state — a
  History panel row's "Set History Brush Source", the oldest retained state
  by default — and refuses a state that lacks the layer or has another
  canvas size or bit depth. At 16 and 32 bits each stroke commits as one
  cancellable job; a 32-bit float document refuses the Sponge on a mask or
  channel by name. MCP: `blur_stroke`,
  `sharpen_stroke`, `smudge_stroke`, `sponge_stroke` and
  `history_brush_stroke`
- **Art History Brush** (Y, sharing the History Brush's slot): each click
  paints a set of stylized strokes from the History Brush's source state,
  each stroke in the source's colour where it starts, heading along the
  source's luma contours, and starting only where the source differs from
  the current pixels by at least the Tolerance. The options are Mode (seven
  modes), Opacity, the ten Styles, Area and Tolerance. It works at
  8-bit RGB and on 8-bit CMYK ink; other depths refuse by name. Actions
  record it, and MCP has
  `art_history_brush_stroke`
- **Healing Brush** (J): ⌥-click sets the source, then a stroke clones its
  TEXTURE while keeping the destination's LIGHT — the sampled pixels under
  the whole footprint are Poisson-blended into the layer once, at mouse-up,
  as one undo step, so a stroke has no ridges where its dabs overlapped.
  The drag previews the raw clone, which stays on the canvas while the
  blend runs on the background worker; the healed result replaces it when
  it lands. A progress panel with Cancel appears only if the blend takes
  longer than a quarter second; Esc anywhere in the window outside a text
  field — or in the document's History, Snapshots or Layer Comps window, or
  the Actions window — and ⌘Z/⇧⌘Z cancel it
  too (the Undo and Redo items read "Cancel Healing Brush" while it runs,
  and no step pops), and on a text or
  shape layer the rasterize question is asked at release,
  before the blend runs. While a healing job runs, a new healing stroke
  anywhere, any 16-bit stroke, and any stroke on that document are refused
  with a beep. Aligned, the three Sample modes, Ignore Adjustments,
  source-transform controls and the tip options. With Aligned on
  the source marker follows the pointer, because what persists between
  strokes is the OFFSET and so the point being sampled moves with the brush;
  with it off the marker stays on the point you ⌥-clicked and every stroke
  re-aims from there, which is what the Clone Stamp always does. Hardness
  shapes the footprint rather than fading the heal — the blend is already
  seamless at the edge, so there is nothing to feather — and Opacity is the
  heal's strength. Size stops at 3 px for both healing brushes, and `[` will
  not step below it: the blend keeps the destination wherever the footprint
  touches un-healed pixels, so a footprint two pixels across is all edge and
  would heal nothing at all. Replace mode is deliberately not offered: in
  this build it would be exactly the Clone Stamp
- **Spot Healing Brush** (J): the same stroke with no source point. The
  source is generated, by the options bar's Type: Content-Aware (the
  default) inpaints the footprint by PatchMatch from a ring of valid pixels
  around it, Proximity Match copies the nearby patch whose surroundings best
  match the stroke's, and Create Texture tiles a texture built from the
  pixels under the stroke; each is then Poisson-blended in, and the
  footprint ghosts on the canvas while you drag. Hardness shapes the
  footprint here exactly as it does for the Healing Brush: a soft tip heals
  a smaller area, never a fainter one. It is the slowest BRUSH (the
  Healing Brush, the Patch tool and Content-Aware Fill also run on the
  worker): the whole footprint is inpainted and blended at mouse-up, which for a
  big brush and a long drag is seconds (2.4 s for a 200 px brush
  dragged 800 px across a 2000 × 1500 photo, 4.4 s for a 1500 px drag on a
  5000 × 4000 one). The final result runs on a background worker with
  progress and Cancel; cancellation stops the Rust computation and leaves
  the layer unchanged
- **Patch tool** (J): outline a polygon or adopt the current selection, then
  drag it. Source heals the outlined flaw using texture at the dragged position;
  Destination moves the outlined good texture onto a flaw. Current, Current and
  Below, and All Layers sampling are available at 8, 16 and 32 bits, with
  ignore-adjustment controls. The gesture captures its source and options when
  placed, and the drag and commit use that same capture. Offsets retain their
  established whole-pixel rounding and output remains in the current layer.
  Every depth commits on the shared cancellable healing worker; while another
  job holds the worker a commit is refused with a beep and the region stays
  placed for another drag. A successful patch is one Undo step, and polygon
  or adopted-selection Actions replay the same operation. Outline a clean rim
  around a flaw so the illumination solve has useful boundary pixels.
- **Red Eye** (J): drag a rectangle over an eye and flash red inside it is
  neutralised and darkened — scored by red DOMINANCE rather than a naive
  R > G test (which every skin tone passes), so a specular catchlight comes
  out untouched. Pupil Size rejects a red region wider than that fraction
  of the rectangle's shorter side, and defaults to 100 % because its job is
  to spare a red shirt caught by a sloppy rectangle, not to require a small
  pupil; a red region that reaches all four sides of the rectangle is
  refused whatever Pupil Size says, because a rectangle dropped inside
  something red has no pupil in it to find — that rule is what gives the
  default teeth, since a red region is clipped to the rectangle and on a
  square one can never measure bigger than it.
  Filters > Remove Red Eye does the whole picture automatically
  through Vision's face landmarks; with no face in the frame it says so and
  changes nothing — the tool's rectangle is then the way to do it
- **Content-Aware Fill** (Edit > Content-Aware Fill…): fills the selection
  using surrounding texture in 8-bit, 16-bit or 32-bit float documents. Include or exclude
  source areas with brushes and rectangles; the selection stays the destination.
  Choose Current, Current and Below, or All Layers sampling, optional ignored
  adjustments, a sampling ring and a reproducible seed. Prepare Final Result
  computes the full result with progress and Cancel. Compare Before and After
  at Fit or 100%, pan for detail, then Apply publishes those exact reviewed
  pixels as one Undo step. Changing the source area or settings invalidates the
  candidate, and document/selection changes refuse a stale result.
  Output can replace the layer or create a new Content-Aware Fill layer while
  preserving the original and its editable description. New output over a
  translucent backdrop can increase opacity; gated groups refuse new-layer
  output. Soft selection coverage is applied once and every zero-coverage pixel
  remains unchanged. A selection over 1 MP is filled with its search at a
  reduced scale (half to 4 MP selected, a quarter to 16 MP), so large structure
  is matched and finer texture is copied without being matched to the
  selection's edge. The shared limits are 4 MP for the selection plus its
  sampling ring (16 MP for a selection over 1 MP, 64 MP over 4 MP), 32 MP for
  one part's window and 80 MP of component windows; working-memory admission can
  refuse earlier. Source recipes support 512 operations and 4096 brush points.
- **Remove tool** (J): paint over something and, on mouse-up, the stroke is
  filled, with the stroke as the hole. Its Mode is Model, the default, which
  fills from a local MI-GAN inpainting model downloaded from Settings ▸
  Models, at 8 and 16 bits (32 bits and CMYK refuse it by name and point at
  Content-Aware), or Content-Aware, the same content-aware synthesis as
  Content-Aware Fill. It has a brush size, Sample all layers, and output to the
  current layer or a new layer. With "Remove after each stroke" off, finished
  strokes are held (at most 64) until Apply (Return) fills them as one undo
  step or Cancel (Escape) drops them. MCP `remove_stroke` takes `mode`,
  `use_selection` and `strokes`.
- **Content-Aware Move** (J): outline a region and drag it. Move fills the
  place it left and heals its pixels in at the new position, as one job and
  one undo step; Extend is the heal alone. MCP `content_aware_move`. Both
  tools (the Remove tool in its Content-Aware mode) run as cancellable jobs
  at 8, 16 and 32 bits, and a hole past 1 MP is searched at a reduced scale as Content-Aware
  Fill's is; the Spot Healing Brush has no 1 MP cap either.
- **Shape layers** (U — repeated presses cycle Rectangle, Ellipse, Line):
  drag out a shape (Shift constrains squares, circles and 45° lines) and
  it lands as its own parametric layer — fill, stroke, weight and corner
  radius from the options bar, plus the stroke's corner style (miter, round,
  bevel), dash (dashed or dotted) and alignment (inside, center, outside;
  `stroke_join` / `stroke_dash` / `stroke_align` over MCP), the description stored in the layer's meta
  like text, so Move keeps it honest and the `.doctored` format round-trips it.
  Double-click a shape layer in the panel to reopen it: drag the eight
  handles or the interior to re-box it, restyle it from the options bar,
  Return (or a click away) re-renders it as one undo step, Escape cancels.
  A transformed shape reopens with its handles on the rotated box and
  re-boxes in its own space — a rotated rectangle grows along its own axes
- Eyedropper (I): picks the color under the cursor into the shared paint
  color, sampled from the flattened composite — what you actually see — or,
  with Sample from on Current layer, from the active layer alone, at every
  depth; point, 3×3 or 5×5 mean sampling, an optional sampling ring, a
  monospaced hex readout in the options bar, a Recent popup of the last eight
  picks and an optional copy-to-clipboard on pick; a drag keeps
  sampling, and Option-click borrows the eyedropper mid-tool from brush,
  fill, and gradient; a read that fails beeps on the press (never an alert)
- Color Sampler (I, sharing the Eyedropper's slot; bare I cycles the
  two): click pins up to ten readouts per document at
  the current Sample Size, drag a marker to move it, ⌥-click it or drag it
  off the canvas to delete it, and ⇧-click with the Eyedropper adds one;
  View ▸ Show Color Samplers (placing one turns it back on) and Clear Color
  Samplers, and Clear All in the options bar. The Info panel's Color Samplers section reads each one live
  from the composite at 8, 16 and 32 bits (RGB, HSB, Lab or 16-bit codes),
  and while a tone sheet previews it shows before → after on the panel and
  the canvas markers. The list is saved with the document (`.doctored`, and a
  PSD's colour-sampler resource at 8 and 16 bits, which Photoshop reads at
  the same pixels); a change dirties the document but adds no undo step.
  Crop, Canvas Size, Image Size, the rotations, flips, arbitrary rotation and
  Straighten move the samplers with the content (one that lands off the
  canvas is dropped); ordinary undo and redo keep the
  current list, and undoing a geometry edit brings back the list from
  before it
- Ruler (I, sharing the same slot): a drag measures a distance and an angle,
  read out in the options bar, and Straighten Layer rotates the selected
  layers by the measured angle as one undo step (MCP `straighten_layers`, at
  8, 16 and 32 bits)
- Brush and eraser with the full tip option set, shared with the clone
  stamp and dodge/burn (per-tool size, opacity/exposure, hardness, flow,
  spacing, angle, roundness, smoothing, pressure size and airbrush, plus
  built-in tip presets: hardness below 100% stamps soft airbrushed dabs;
  flow deposits per dab so a stroke builds up where it crosses itself,
  with opacity still capping the stroke once; spacing sets the dab
  rhythm — 150%+ reads as a dotted line; angle and roundness squash the
  tip into a calligraphy nib; smoothing steadies the hand with a
  pulled-string leash that catches up at mouse-up; pressure size tracks
  a tablet pen's pressure; native Pressure Opacity also scales each dab; airbrush keeps depositing while the pointer
  hovers; and the brush and clone stamp composite their paint through
  the full layer blend-mode set; shared color; `[`/`]` resize, up to
  5,000 px for the brush, eraser, clone stamp and dodge/burn, `{`/`}`
  step hardness through 0/25/50/75/100%, `,`/`.` step through the built-in
  presets (wrapping) and Shift+`,`/Shift+`.` jump to the first/last; the
  pointer becomes an outline of the tip's full footprint, scaled by size,
  angle, roundness and zoom (the crosshair returns below 6 screen points
  and while ⌥ samples a pixel); 1 px
  pixel-snapped mode; fast drags render through a smoothing spline, so
  a flick lands as a curve instead of a chain of straight chords;
  ⇧-click paints a straight line from where the last stroke ended
  (brush, eraser, clone, dodge/burn and both healing brushes);
  strokes confine to an active selection)
  and on-canvas text (font/weight/size/color and left/center/right
  alignment, ⌘Return commits, Escape cancels) — tools switch via the left
  tool rail, Tools menu, or M/L/W/C/V/B/E/S/Y/O/R/J/G/U/P/T/I/Z/H — a
  shared tool key cycles its tools in rail order, first returning to the
  one last used. Tool keys still switch after
  an options-bar edit or preset pick. Related tools share one
  rail slot: the nine selection tools, Crop and Perspective Crop, the five
  paint tools (brush, eraser, clone, history brush, art history brush), the six healing tools
  (spot healing, remove, healing, patch, content-aware move, red eye),
  the five toning and focus tools (dodge, sponge, blur, sharpen, smudge),
  the three shapes, the eyedropper with the color sampler and ruler, and
  zoom + hand,
  each slot showing whichever member is current with a corner triangle
  that drops a menu of the rest (with their keys). A group remembers the
  member last used
- **The redesigned chrome**: a 48pt icon rail down the left with the
  foreground/background swatches at its foot — a swap glyph in the pair's
  top-right corner and a default-colours glyph in its bottom-left; X swaps
  the colours and D resets them to black/white (white/black while a layer
  mask is the edit target), and like the tool letters they are canvas
  keys, so a text field keeps them; clicking either swatch opens a
  Color Picker sheet (a saturation/brightness field and
  hue strip, the new/current pair, and HSB, RGB, Lab and hex fields — the
  Lab is the core's D50 Lab through sRGB, the Info panel's numbers on an
  sRGB document — with a note when a Lab entry is outside sRGB and was
  clamped, and System Colors… for the macOS colour panel, which also
  edits alpha) — and a fixed-height options
  bar under the title bar that never resizes the canvas — each tool
  declares its options in priority order and whatever doesn't fit at the
  current window width folds into a `More` popover instead of wrapping.
  Numeric fields carry a chevron menu of quick-pick presets (a 1–64 px
  spread for sizes, tens for percentages, doublings for radii) alongside
  typed entry. Options persist per tool across documents and launches
- **Screen keys**: with the canvas holding key focus, Tab hides or shows
  the tool rail, the options bar and the side panel together (View ▸
  Hide/Show Panels is the menu path), Shift+Tab hides or shows only the
  side panel, and F toggles macOS full screen on the editor window. A
  focused text field or panel control keeps Tab for itself, and Ctrl+Tab
  moves key focus off the canvas. The state belongs to each window and
  is not saved; restoring a workspace brings the rail and options bar
  back. Grey/black surround and edge-reveal screen modes are
  declined for now
- **Re-editable text layers**: committing text adds its own layer that
  remembers the string, font, size, color, alignment and typography —
  weight, italic, tracking, leading, baseline shift, underline,
  strikethrough — it was rendered from, plus the width it wraps at:
  paragraph text remembers its box, point text (from the agent's
  `wrap_width: 0`) never wraps, and a layer saved before widths were
  stored is given one the first time something rewrites it, remembered
  from then on: the on-canvas editor re-wraps it from its origin to the
  right edge (at most 600 px) as it always did; a Free Transform, a
  document rotate/flip or Image Size recovers the width from the layer's
  own raster, so its existing line breaks are kept; and the agent's
  `edit_text_layer` keeps its canvas-edge default (no 600 px cap). Click
  it again with the text tool to reopen the editor pre-filled, with
  everything restored into the options bar, and the layers panel badges
  it with a "T". Double-clicking the layer's row in the panel reopens it
  the same way from anywhere: it switches to the text tool and selects
  the whole string, so typing replaces it. A rotated or scaled layer
  opens in place, turned and scaled like its glyphs (a mirrored layer, or
  one entirely off the canvas, opens upright at its block's origin); the
  commit re-renders it through its transform. A destructive edit
  (filter, adjustment, fill, gradient, brush, eraser) asks "Rasterize text
  layer?" first and drops the description on confirm, keeping the pixels.
  The native `.doctored` format stores the description alongside the pixels, so
  text stays editable across save and open in both document depths.
  Descriptions preserve continuous sRGB RGBA and attributed colors;
  Layer > Artwork Colors provides precise default or character-range entry.
  Older descriptions remain readable. A reader that supports the native
  container can display saved pixels for an unknown description. Tall or dense text and shape
  outlines rasterize by an active-edge sweep bounded by the ink's rows, so
  dense outlines finish at full resolution,
  and the work charged for a row is linear in its length, so a
  1,000-character line of prose at 24 px renders
- **Live Photo and movie frames**: open a JPEG/HEIC still and movie sharing
  one basename, or a supported SDR H.264/HEVC/ProRes 4444 movie (an HLG or
  PQ HEVC movie opens as a 32-bit float document and does not place). Pairing uses the
  filename; it does not verify an Apple asset ID. A pair opens as its
  original full-resolution still, a bare movie as its first frame. Layer >
  Place Live Photo adds one to an existing document. Either way the frame
  lands as an ordinary raster layer, converted into the document's profile;
  the source files are not kept in the document.
- **Camera RAW**: open a CR3, CR2, NEF, ARW, DNG, Apple ProRAW, ORF, RW2,
  RAF, SRW or PEF in the Develop window before the pixels enter a document.
  Exposure, white balance, tone curve, shadow lift, local contrast, sharpness,
  detail, luminance/colour noise reduction, lens correction and supported
  highlight recovery start at the file's reported settings, or an `.xmp` sidecar's
  exposure. Unsupported controls are disabled; Reset restores the captured starting
  values and Cancel opens nothing. **Enable Profile Corrections** applies the
  system decoder's lens profile or, when the decoder has none, the bundled
  Lensfun profile matched to the file's camera and lens (never both), and
  starts on for a new development when either applies; the window shows the
  lens the file's EXIF names and which profile corrects it, and
  `open_document` reports `lens_profile` with its `source`.
  Fit and **100% native detail** share a full 16-bit development. Pan through the
  native source, compare Before/After or Split, and inspect a whole-image RGB
  histogram with exact 0/65535 endpoint counts. Clipping markers show black,
  white or both endpoints, including native pixels skipped by the Fit colour preview.
  Choose normalized SDR **sRGB, Display P3, Adobe RGB (1998) or ProPhoto RGB**
  output (ProPhoto is best in a 16-bit document); the exact camera file,
  effective recipe and decoder identity remain embedded alongside the genuine
  u16 developed master in native saves, snapshots and recovery. Beside 16-bit
  and 8-bit, the Document depth row offers **32-bit float (linear)**: it develops into a 32-bit
  float document whose RAW layer keeps scene-linear highlights above 1.0 in the
  chosen space's primaries (an f32 master); exposure, white balance, the tone
  curve, Shadows, detail, noise, lens and highlight controls act as at 16 bits
  (the curve keeps values above white), and Grain applies on the display
  encoding (a pixel at or above white keeps its value; Doctored's own rule,
  not measured against Adobe Camera Raw), while the local tone map,
  Texture/Clarity/Dehaze and local tone controls are disabled there; on a
  screen with headroom the Fit and 100 % previews show values above white, with
  Clipping marking samples at or below 0.0 and at or above white, and the
  histogram reads the float samples in stops, above white included. A RAW placed into a float document develops
  float, Redevelop RAW there can switch either way, and a float RAW layer never
  narrows into an 8- or 16-bit document.
  **Layer > Redevelop RAW…** changes development beneath existing retouch layers,
  preserving source identity, accumulated affine, masks and other layer properties
  through one Undo. Every RAW decode is charged 528 MiB plus 82 bytes per pixel plus
  three times the file: at least 1.25× the peak of every measured file, real 24–61 MP
  camera RAWs included, and above 61 MP at least the largest file's whole cost per
  pixel, so a 45 MP RAW opens on a 16 GB Mac.
  Each develop renders through its own Core Image context, released afterwards.
  Core Image frees it asynchronously: right after a develop up to about 0.78 GB
  can still be resident (0.31 GB above what the document retains one second
  later in the app), settling to at most 26 MB in the probe and about 74 MB in
  the app within six seconds.
  **Layer ▸ Redevelop RAW…** first restores the embedded camera file, charged 22 MiB
  plus 2 bytes per pixel plus three times the file. Local development — the Develop window's
  preview, `edit_raw_development` and an enhanced `redevelop_raw` — is charged the
  decode figure or 522 MiB plus 73 bytes per pixel, 13 more per local and 16 more
  with authored lens geometry, grain or vignette, plus three times the file,
  whichever is larger: 1.25× or more of every measured peak on real 24–61 MP
  camera files, up to four camera-variant locals (tone-only locals and sixteen
  masks measured within it at 24 MP). At the 8 GiB limit a
  16 GB Mac gets, a 45 MP RAW restores, previews and Applies with four locals
  (its preview is charged 6.39 GB) and a 61 MP one with one; the preview ceiling
  is about 70 / 68 / 51 MP for 0 / 1 / 4 locals. A RAW's file and masks share
  1 GiB, so sixteen full-size masks fit up to about 32 MP (eight on a 61 MP
  file), and a local past that is refused up front with the figure. A float
  development opens to about 78 MP there. Camera-relative baseline compensation and supported moiré
  reduction add explicit camera controls; manual vignette operates in the chosen
  source profile. The **Local Masks** tab creates up to sixteen source-coordinate
  radial, linear or brushed masks, or masks Vision generates — **New Subject**
  and **New People**, as Select Subject and All People read the global
  development, stored as native 16-bit coverage and recomputed only on request
  (Recompute), or **New Sky** from the sky model Select ▸ Sky uses (Settings
  ▸ Models offers it when it is absent) — and refines any of them, or covers
  the whole frame, with a
  **Luminance Range** or **Colour Range** (a Lab colour and Delta E, picked by
  the Sample Colour tool), read from the base development so a local's own
  exposure never moves its selection. A local's **tone** controls — Exposure,
  Contrast, Highlights, Shadows, Whites, Blacks, Saturation, Clarity,
  Texture, Sharpness and Noise Reduction — run in the core on the 16-bit
  development inside its mask (a float development refuses Sharpness and
  Noise Reduction by name); a local
  that changes white balance or camera exposure (which recovers clipped
  highlights) develops from the original RAW, at most four of those. Locals
  blend in stack order beneath
  the manual vignette and existing layer geometry/retouch. An explicit, supplied
  radial-distortion/transverse-CA profile can then map that native 16-bit master;
  this is not an automatic lens database. Local masks remain in canonical
  before-optics coordinates; the Local Masks tab shows that editing view, while
  Global shows the final optical result. With nonneutral optics, review Global
  before Apply. **Texture, Clarity and Dehaze** (−100 to 100, Filters ▸
  Presence…'s kernels) act on the finished development after locals, vignette
  and explicit optics, live in the preview. **Grain** (Amount, Size, Roughness —
  the Add Grain kernel) is the
  last development stage, after locals, vignette, explicit optics and Presence, so the
  Local Masks editing view stays grain-free. Its seed comes from the embedded
  camera file's SHA-256, so redevelopment, reopen, presets and sync reproduce
  identical grain. Density, feather,
  inversion, ordering and enablement remain editable. Apply uses the exact reviewed
  result; Cancel preserves the document. Local settings can be copied while each
  target keeps its own mask. **RAW Presets…** stores selected transferable controls, and
  **Synchronize RAW Settings…** applies selected settings to chosen RAW layers
  with one Undo per changed document. A library file that is not a readable
  preset is LISTED with the reason instead of blocking the library: repair it
  in place — the name is trimmed and each control kept only if it still reads,
  with every dropped control named — or delete that one file. Presets are
  written at the current schema version and older ones are still read; a file
  from a newer version is reported and never rewritten, and a save whose file
  name is already taken is refused rather than overwriting it. Presets do not carry another camera's hidden
  defaults; as-shot white balance and baseline compensation use each target's
  captured camera settings, and global synchronization preserves target-local masks.
  With **Masking** checked, a sync replaces each target's local adjustments
  with the source's, in order: drawn masks are resampled to the target's size,
  a Colour Range keeps its stored colour, and Subject, People and Sky are
  detected again on the target, one that finds nothing being skipped and named
  (MCP `sync_raw_settings` `locals`).
  Opening saved work never invokes a RAW decoder. **Migrate RAW Decoder…**
  explicitly compares the saved rendering with an available decoder's prepared
  result; keeping the saved rendering changes nothing, and acceptance applies
  the reviewed pixels. EXIF orientation and registered integer crop/pad changes
  remap native masks exactly; a changed legacy extent without registration needs
  an explicit reviewed inverse map. Filtered affine maps retain native 16-bit
  coverage. Migration preserves the original camera bytes and registers existing
  vignette and optical coordinates against the old source. Unsupported migrations
  refuse without changing saved work.
  Agent tools mirror development, native detail/histogram inspection, presets,
  synchronization and migration. `edit_raw_development`, `get_raw_locals`,
  `get_raw_local_mask` and `copy_raw_local_settings` expose the same local workflow,
  range, subject and people masks and tone included, and `raw.output_encoding`
  (`sdr16` or `linear_float`) chooses the float development on `open_document`,
  `place_image`, `redevelop_raw` and `edit_raw_development`;
  recorded Actions retain exact source/recipe/mask guards. Omitted `redevelop_raw` fields keep saved values;
  `as_shot_white_balance:true` restores captured as-shot balance. Import defaults
  to 16-bit; explicit `precision:rgba8` converts only the layer fallback and retains
  the u16 master and original camera bytes. Byte documents reconstruct supported
  ImageIO capture fields. Typed RAW and still imports reconstruct the platform
  reader's capture packets and the file's PPI into the document, so an export
  carries them; the exact original camera metadata stays inside the RAW layer's
  embedded file.
  Original/mask resources remain limited to 256 MiB each, originals to 512 MiB
  and originals with masks to 1 GiB together, subject to the existing memory budget. Each local mask is at most
  64 MP; a pending edit is bounded to 256 operations and 4,096 brush dabs.
  Camera variants run sequentially with at most five cached developments / 256 MiB.
  Cancellation is checked between camera calls and during native work; each
  individual platform capture/render call remains noncancellable. Broader camera,
  lens-database calibration, broader camera coverage, scale and HDR policies have
  named later assignments.
- **Native layer blending:** the Layers panel supports Normal, Multiply, Screen,
  Overlay, Soft Light, Hard Light, Darken, Lighten, Difference, Exclusion,
  Color Dodge, Color Burn, Linear Dodge (Add), Linear Burn, Subtract, Vivid Light,
  Linear Light, Pin Light, Divide, Hue, Saturation, Color, Luminosity, Dissolve,
  Darker Color, Lighter Color and Hard Mix on 16-bit raster layers, adjustment layers and isolated groups, with Pass Through available for
  groups. Masks and opacity retain native coverage and f32 intermediates until
  the final u16 projection; stored pixels remain exact. The existing
  `set_layer_properties` tool and recorded Actions share the controls, captured
  publication and Undo. Native saves preserve the modes through reopen,
  snapshots and recovery. A blended adjustment layer's adjusted colour is
  pushed through the mode as the source against the backdrop, then mixed back by
  opacity × mask coverage, exactly as the 8-bit compositor does.
  Blend If on a 16-bit raster layer, adjustment layer or group (Layer > Layer Style >
  Blending Options…, `set_layer_style` with a Blend If-only style) evaluates
  its ramps on 16-bit codes — each 0–255 knee at code 257 × knee — exactly
  where the byte compositor weights a layer's alpha;
  a Pass Through group carrying Blend If composites isolated.
  Layer styles — fill opacity and all nine effects — on a 16-bit raster layer
  or group render from the shape quantized to the byte plane through
  the byte renderers (a promoted document renders within 129 codes of the
  8-bit compositor) and scale with geometry;
  a styled Pass Through group composites isolated too.
  **Knockout** (None / Shallow / Deep) and **Blend Interior Effects as Group**
  are the two remaining blending options, they sit in the Layer Style sheet's
  Blending Options pane, and they work at 8, 16 and 32 bits: a knockout makes
  the layer's own shape punch a hole through a backdrop instead of compositing
  onto it. An isolated group's buffer stops BOTH modes (the hole then shows
  what lies beneath the group), a clipped layer punches to its clip base
  alone, Shallow inside a Pass Through group punches to what that group's
  contents started from, and Deep punches to the Background layer — the bottom
  layer when it is a plain, visible, unmasked, unstyled, Normal, opacity-1,
  canvas-sized layer at (0, 0), unless the PSD it came from made it an
  ordinary layer — or to transparency when there is none.
  "Blend Interior Effects as Group" makes the layer's own blend mode apply to
  Inner Glow, Satin and the two Overlays. Layer ▸ Flatten Image, Stamp
  Visible and Merge Visible bake a knockout EXACTLY, because all three render
  the projection; **Merge Down and Merge Layers merge it against its REAL
  backdrop** — captured from the document's own projection — so the hole is
  filled with that backdrop's pixels and the picture is kept wherever it is
  opaque. Where no backdrop exists by structure (inside an isolated group,
  inside a merged pass-through group, clipped to a base outside the merge, or
  punching to the root of a document with no qualifying Background layer)
  the hole still bakes as transparency and the `merge_down` reply says so.
  **Both keys cross a PSD in both directions and at every depth**, as
  Photoshop's own `knko` and `infx` records on the layer's style — on rasters
  and on group records — whenever the style itself is written rather than
  rasterized into pixels, and only when the key is not at its default; a
  rasterized style writes neither and the export report says so per layer.
  An 8- or 16-bit RGB PSD's bottom layer keeps what the file says it is —
  Photoshop's Background layer, or an ordinary layer a Deep knockout punches
  through to transparency (`get_document` reports it) — through `.doctored`,
  and the export writes such a Background, or an unmarked opaque bottom a
  written knockout reaches, as Photoshop's Background layer. A bottom that
  cannot be one (transparent pixels, a mask) and the Gray, Lab, CMYK and
  32-bit files keep one disclosure sentence in the report: Photoshop punches
  such a knockout through to transparency where Doctored shows that bottom
  layer. Blend Interior Effects as Group applies on a
  clipping base that has visible clipped layers too: the
  grouped interior effects recolour the base and its clipped layers together,
  which then meet the backdrop once with the layer's blend mode.
- **Actions** (File > Automate ▸): an Action is a recorded sequence of the
  same MCP tool calls an agent makes. Start Recording, work — from the menus,
  the panels, the options bar or the canvas, a brush stroke included — then
  Stop Recording and name it, and it replays on any document. Played from the
  Actions window or File ▸ Automate ▸ Play Last it is one undo step per
  step that changed something; MCP `run_action` and
  Batch replay it as ONE undo step. A step, or a run, that changed nothing
  leaves no undo step, so a run whose steps all failed leaves no phantom entry
  behind. A replay long enough to notice puts up a step counter with a Stop
  button; stopping keeps what has already been applied. When
  a step set to stop on error cannot run during Play (the Actions window or
  File ▸ Automate ▸ Play Last), it asks **Stop** or **Continue** ("The step
  “Paste” could not run.", or "The step names layer “X”, which this document
  does not have.", with the reason beneath; Continue is the default): Stop
  keeps the steps already applied, Continue skips the step and plays on. A
  command that runs and fails (a Move or Transform on a selection that holds
  no pixels) offers no Continue: the play stops with "… stopped: the
  selection holds no pixels.". Select Inverse with nothing selected is not
  a failure. Batch, MCP `run_action` and ⌃F never ask. Every user-visible edit
  already has an MCP twin, and that parity is
  what makes recording possible at all: each of the app's edit entry points
  states the call it is, so a new feature cannot quietly stop being
  recordable — it does not compile until it says what it records. A command
  with no twin (Quick Mask, a multi-layer Free Transform, a clone or
  heal stroke with no latched source, playing another action) lands as a
  VISIBLE placeholder naming it, never as a silent hole, and replaying that
  step reports the gap instead of skipping it. A MOVE is not one of
  them, at either size: it records the relative `move_layers` step, naming the
  one layer that was dragged or the current selection — or, with a selection
  active, `move_selected_pixels` naming the layer; a ⌘T over a selection
  records `transform_selected_pixels`. Neither are a
  multi-entry Reorder Layers, a ⇧-click Auto-Select, or a lock or clipping-mask
  write over a multi-selection, all four of which have real tools, and a channel
  whose name will not read is recorded by its index rather than as a
  placeholder. A recording is bounded — 1,000 steps or 200,000 recorded stroke points,
  which is a few megabytes of JSON — and reaching either appends one more
  placeholder saying so rather than growing a file nothing can open;
  deleting steps in the window gives the budget back. Steps that mean
  "the active layer", "the layer called Sky" or "the current layer selection"
  are recorded symbolically and resolve against the document being played. A
  layer or channel is named by its DISPLAY NAME **and** by a stable id, so a
  step still aims at the very layer that was clicked after it has been renamed
  or reordered — while the name is what carries the same step to another
  document, or to each of a batch's files (a layer's id is saved in the
  `.doctored` and survives a reopen; a channel's id is not saved, so it holds
  only while the document stays open).
  Canvas coordinates stay absolute unless you say otherwise: a step's Geometry
  can be set — in the Actions window's row menu — to **relative**, which scales
  it with the canvas, or to one of nine **anchors**, which keeps its distance
  from that corner or edge, so "20 px in from the bottom-right" stays 20 px in
  on any size. An absolute step on a differently sized document still reports
  the difference rather than guessing at it, and Select All is a
  canvas-relative step rather than a literal rectangle so it batches correctly.
  A Quick Mask stroke refuses a Geometry by name. Select
  Objects' point or region remaps like any other coordinate, and the matrix of
  a Free Transform or a mask's Transform is carried through the same remap —
  its translation scales or shifts with the canvas and is never rounded to
  whole pixels — so a Free Transform's matrix and its corner-pin form land in
  the same place for every recording made in the app (a hand-written integer
  corner rounds to the nearest pixel, as everywhere else). A Select Objects
  coordinate written as a numeric string is refused by name when it replays
  under relative or anchor geometry.
  An action can also declare **parameters** — a name, a type and an optional
  default — that its steps reference; every way of playing one asks for them
  first, and a value that is missing or of the wrong type refuses the whole run
  before anything is applied. Actions are
  plain JSON in `~/Library/Application Support/Doctored/actions`, one file
  per action, editable by hand; a file that will not parse is listed with the
  exact reason rather than dropped. An action that uses none of the newer
  constructs is still written in the older schema, so hand-edited files and
  older builds keep reading it. **Package…** writes an action and the files it
  uses (a placed image, a profile) into a `.draction` bundle that can be handed
  to somebody else; **Import…** installs one, files and all, and the paths in
  the copy you keep are never touched. `DR_ACTIONS_DIR` points that folder
  somewhere else, which is what the test instances use. Agents get the whole
  surface — `start_recording` / `stop_recording` record a person's session,
  `save_action` writes one from a sentence, `run_action` plays it — with
  three exceptions, all refused by step index when the action is read. An
  action may not contain the Actions tools themselves: it does not run or
  edit actions. It may not contain `save_copy`: an action says what to DO to
  a picture, never where to write it — a step's path is the absolute one it
  was written with, so in a batch every file would be written over that same
  one, outside the output folder — and Batch is what chooses the folder, the
  format and the name. And it may not contain `undo` or `redo`: `run_action`
  and Batch play an action as one undo entry, so an undo inside it would throw
  the document's redo history away and report success. A tool call from an
  agent or the assistant that arrives while an action is playing is refused in
  band and can simply be retried, rather than landing in the middle of the run
  (and, in a `run_action` or Batch run, inside its single undo entry, to be
  reverted with it). The built-in assistant knows the symbol vocabulary, so
  "make me an action that
  warms and sharpens" is a request it can fill. Filters > Repeat Last Filter
  (⌃F) re-runs the last destructive filter or adjustment with the same
  parameters, on the same machinery; unlike a replay inside an action, it
  asks before rasterizing a text or shape layer, exactly as the
  Filters menu itself does. Edit > Fade… (⇧⌘F) opens right after a filter or
  destructive adjustment, Auto Tone/Contrast/Color, Liquify, a fill, a
  gradient or a paint or retouch stroke: Opacity and any blend mode the
  document's depth admits, with a live preview, blend the result with the
  state before it, and the fade replaces the step it fades as one undo step
  named "Fade &lt;step>", at 8, 16 and 32 bits, recorded by Actions and over MCP
  as `fade` {opacity, mode}. Any other change, an undo or a new selection makes
  it unavailable, and it does not follow Patch, Red Eye, Remove,
  Content-Aware Move or Fill, Apply Image, or an edit of a mask, channel or
  Quick Mask ([`designs/v1-shortcuts-fade.md`](https://github.com/kpgalligan/doctored/blob/main/designs/v1-shortcuts-fade.md)).
  Tools > Command Palette… (⌘F) fuzzy-searches every menu command in the
  app by name or by its
  whole path, shows each one's own shortcut, and runs the top match on
  Return; the list is a walk of the real menus rather than a second
  catalogue, so it can never fall behind them, and it offers only what is
  enabled right now — which is why it still lists File > Open… with no
  document open. A command run from the palette is recorded like any other
- **Batch** (File > Automate ▸ Batch…) runs one action over every image in a
  folder — its top level only, not sub-folders — and writes each result into
  an output folder that must not be the input folder, in a chosen format,
  named by a `{name}`/`{n}` template whose extension always comes from the
  format. Camera RAWs are developed with the action's own `raw` settings,
  edited in the Batch window's RAW develop row and saved back to the action,
  because no dialog can be shown per file. Stop on error says both halves: on, a
  failed step halts the run and the file it was working on is not written;
  off, that file is written as the action left it and every failed step is
  listed against it. Nothing is written outside the output folder, a name a
  file already written has claimed is refused rather than overwritten, and an
  existing file is left alone unless Overwrite is on. The report says what
  happened to every file — including what each format carried and dropped —
  and copies to the clipboard as text. An agent can run the same thing without
  the window — `run_batch` starts it, `batch_status` polls the per-file report
  and `cancel_batch` stops it at the next file boundary. One batch runs at a
  time, and while it does every other tool call is refused in band with a
  sentence saying so, because the run owns the documents it has open.
  **Deliver with:** an export preset instead, and the destination folder, the
  naming pattern and every delivery option come from the preset (the output
  folder, name template and Overwrite controls then do not apply); the run
  writes through Quick Export, which never overwrites, and a metadata policy
  may be chosen for the run. A preset pattern's `{index}`, like `{n}`, is the
  file's position in that run, so re-running over a folder that has gained or
  lost files renumbers the outputs rather than matching the earlier run's
  names. A file the preset would write that already exists is skipped with
  its reason — never overwritten, and never a halt, even with Stop on error on.
  The Output row then reads the preset's own folder, dimmed, and says
  "(missing)" when that folder does not exist; such a run is refused before it
  starts, in the window and in `run_batch`.
- Opening an image focuses its canvas, so image commands act on the selected
  layer immediately. Ordinary **Paste** (⌘V) creates and selects a new layer
  above the current layer; a deliberately focused text field receives text
  paste instead. Paste centres the copy on the selection's bounds when a
  selection is active (not in Quick Mask), with no mask, and otherwise on
  the visible part of the canvas —
  never at its source position, however much of it is in view; a pixel
  paste made with a selection deselects it in the same undo step
  (Reselect brings it back). MCP `paste` does the same by default
  (`placement: "auto"`, read from the window `view` names — the document's
  first without it — and the canvas centre with no window), reports where
  it landed as `placement` and `offset`, and takes `placement: "centered"`
  for the canvas centre or `"at"` with `x`/`y`; a recorded Paste stores no
  position and lands by the same rule on the document it replays on. Copy,
  Copy Merged and Cut also put a private typed payload
  on the clipboard beside the ordinary TIFF/PNG rendering other apps read:
  at 16 bits and 32-bit float it round-trips samples, alpha, profile,
  offset, selection coverage, PPI and (float) the tone map bit for bit, and
  at 8 bits it carries the copied bytes with their offset, so **Paste in
  Place** (⇧⌘V) works at every depth: at the source coordinates in a
  canvas of the source's size, and in another size at the same relative
  location (the copy's centre keeps its fraction of the canvas). A typed
  payload larger than the system
  pasteboard accepts (over 255 MiB: a 24-megapixel float copy is 384 MB) stays
  in the Doctored instance that copied it, so other apps and other instances
  get only the TIFF/PNG rendering. Edit ▸ **Paste Special** holds Paste
  in Place beside **Paste Into** (⌥⇧⌘V) and **Paste Outside**: with a
  selection active (not in Quick Mask) the clipboard lands as a new layer
  centred on the selection's bounds, with a canvas-sized, unlinked layer mask
  built from the selection — or, for Paste Outside, from its inverse — so the
  pasted pixels can be moved around behind the mask or the mask painted
  anywhere on the canvas; the selection is consumed in the same undo step
  (Select ▸ Reselect brings it back) and the layer, not its mask, stays the
  edit target. MCP `paste` takes `placement: "into"` / `"outside"`.
  Pasted pixels are converted into the document's depth and
  profile with no prompt: a float copy is tone-mapped under its own map, a
  16-bit or 8-bit copy and an image from another app are promoted into a
  32-bit float document as linear light, an image from another app enters
  16 bits exactly (×257), and another profile is converted, keeping the
  picture's appearance. An image from another app with no profile keeps its
  numbers in the document's profile, and Paste in Place of an image from
  another app, which has no source position, lands like Paste. Image ▸ Mode ▸
  Working Space ▸ **Ask When Pasting** (off by default) brings up a
  **Paste Profile Mismatch** alert for
  pixels or layers in another profile: **Convert**, **Don't Convert** (keep
  the numbers) or Cancel; MCP `paste` takes the choice as `profile:
  "convert" | "preserve"`. A paste whose PPI differs from the document's says
  so in its History row. **New from Clipboard** makes a document at the copy's
  own depth, profile and PPI, sized to what was copied (the box of its
  visible pixels; a fully transparent copy keeps its size), and rebuilds it
  around a copied text, shape or adjustment layer so that layer stays
  editable (an adjustment opens as an 8-bit document above a transparent
  Background); a record that cannot be restored opens as pixels and says
  why. A recorded Paste stores no pixels: it pastes
  whatever the clipboard holds when the action runs (pixels or copied
  layers), and fails by name when the clipboard holds no image. **Copy** and
  **Copy Merged** are recorded steps too, and copy from the document the
  action runs on — a recorded Copy decides pixels or layers from that
  document's own selection — so a recorded Copy → Paste replays on any
  document. Over MCP, `paste` with `record_payload: true` records the
  saved-payload form instead, which replays the same pixels without the
  clipboard (packaged actions carry it as an asset), and `copy` takes
  `mode: "auto"`. Copying a whole
  **text, shape or adjustment layer** (no selection) keeps it editable:
  Paste restores the text, the shape or the adjustment's settings as a
  re-editable layer at the document's own depth, under its own name (in a
  32-bit float document its colours are rewritten into linear light so it
  looks the same);
  a selection copy, a raster layer or a RAW layer copies pixels
- **Copy and paste layers between documents**: with layers or groups
  selected and NO pixel selection, Copy (⌘C) copies the selected layers —
  each with its whole subtree, masks, blend modes, opacity, styles, clipping
  and adjustment layers — and Paste (⌘V) in any open document adds them above
  the active layer as ONE undo step, their combined content centred on a
  selection or else on the visible canvas, the selection kept; Paste in
  Place (⇧⌘V) keeps their source canvas coordinates, or in a canvas of
  another size their relative location. Into
  8 and 16 bits any copy pastes, narrowed or promoted exactly (×257) and
  converted to the profile silently — or, with Ask When Pasting on, converted or kept by the
  Paste Profile Mismatch alert. Don't Convert keeps every layer's numbers;
  it is offered unless the
  document's profile fails the Assign check for the copied text, shape or
  calibrated Exposure/White Balance layers. Into a 32-bit float document any copy pastes too:
  8- and 16-bit layers are promoted to linear light and another
  profile is converted (or, with Don't Convert, its numbers kept).
  Text and shape layers stay editable (with
  Don't Convert, only from the document's own profile); warps land as their
  pixels; a Pass Through group lands Pass Through; the
  blend modes a float document refuses (Screen, Overlay, …) land
  Normal, while Linear Burn, Linear Light and Pin Light are kept; layer styles and Blend If land, an Outer Glow, Inner Glow or
  Bevel highlight in Screen taking Linear Dodge. A copied
  layer the float mode cannot hold — an effect blend mode outside the float
  set, or an Invert, Shadows/Highlights or legacy White Balance
  adjustment — refuses the whole paste and
  names the layer and
  the feature. The MCP reply lists what changed (`rasterized`,
  `blend_reset`, `effect_blend_reset`). The clipboard keeps an ordinary image for other apps:
  one layer's own Copy, or the copied layers' merged composite when several
  top-level layers are copied. Paste Into / Outside are
  unavailable while copied layers are held, and **New from Clipboard** opens
  the layers themselves: a document the size of their content box at their
  depth (float lands as 16 bits), profile and PPI, the layers by name over a
  white Background. Alpha channels, guides and color samplers do not travel
  with them; a pasted style that uses the global light casts by the
  destination's light. The layer copy lives in this app only. MCP `copy`
  takes `mode: "layers"` and `paste` `mode: "auto" | "layers" | "pixels"`
  (`mode: "pixels"` pastes the ordinary image, Into / Outside included);
  `get_clipboard` reports `layers`. Dragging does the same without the clipboard:
  rows dragged out of the Layers panel, or a Move-tool drag that leaves the
  window, dropped on another document's window copy the selected layers there
  through the layer-paste path as one undo step in the destination, and the
  source document gets no step. A camera RAW layer lands as its current
  pixels. MCP `copy_layers_to_document` is the same path.
- **Copy** (⌘C) with a selection puts the active layer's own pixels within
  the selection on the clipboard — raw, so layer opacity, blend mode and the
  layer mask stay out of it and the copy round-trips through Paste as New
  Layer unchanged (with no selection it copies the layers, above).
  With a group active, Copy takes the group's own composite at every depth:
  its children blended together, without the group's own opacity, blend
  mode or mask. **Copy Merged** (⇧⌘C), by contrast, takes the same region
  of the flattened composite, every visible layer with its opacity, blend
  modes, masks, clipping and adjustment layers applied. Both copy only the
  SELECTED pixels:
  the clipboard image spans the selection's bounds (the whole canvas with
  nothing selected), but anything inside those bounds and outside the shape
  comes out transparent, and a feathered or anti-aliased edge fades out
  proportionally — a lasso or Select Subject outline copies exactly what it
  encloses. Written as TIFF and PNG. **Cut** (⌘X) is Copy then Clear in one
  step: it needs a selection and a layer (a group has no pixels of its own to
  clear), puts the same pixels on the clipboard, and clears the selected
  region of the active layer to transparency as a single undo step
- Full undo/redo, recent files
- Drag image files onto a window to open them; File > New from Clipboard (⌥⌘N)
- **File > New…** (⌘N) starts a document with no image at all: width and
  height in pixels, inches, centimetres or millimetres at a print resolution,
  a colour profile (sRGB, Display P3 or an RGB ICC file on disk), 8 or 16
  bits or 32-bit float, and a background of transparent, white, black or a custom colour —
  with named presets so "8 × 10 inches at 300 ppi in Display P3" is one
  click (**Save Preset…** names the fields as they stand, **Delete Preset**
  removes the selected one); a preset file that cannot be read is listed
  under the Preset popup with its reason and a **Delete File** button. A
  transparent document's single layer is named "Layer 1" and a filled one's
  "Background".
  A 32-bit float document's white is exactly 1.0 and its black or
  transparency exactly 0. Color Mode **Grayscale** makes a
  Grayscale document at 8, 16 or 32 bits in the working Gray, one of Color
  Settings' Gray spaces or sGray, or a Gray ICC file, filled with the
  background's luminance; an RGB document refuses a grayscale profile by name.
  Color Mode **Lab Color** (#1) makes an 8- or 16-bit Lab Color document.
  A 16-bit document may be as large as an 8-bit
  one, 100 megapixels (10000 × 10000): a stored plane in the `.doctored` it will
  be reopened from may hold 1 GiB
- Checkerboard backdrop for transparency

The editor offers **16-bit RGB document mode** for genuine 16-bit PNG/TIFF
sources — RGB and RGBA (a Gray or Gray+alpha file opens as a Grayscale
document) — and typed native documents.
Choose preserving import to retain source
samples and profiles. Layer RGB, supported groups and tone/color adjustments,
u16 masks/channels, geometry, all existing brush paint blends, erase, bucket fill,
gradients, dodge/burn, fractional/transformed clone and
healing, spot healing, red-eye and retained editable text/shapes use the typed core. A 16-bit stroke commits as
one cancellable background job (Esc or ⌘Z cancels it) while its ghost stays on the canvas. Display is a separate 8-bit preview;
readouts and exact clipping counts use the authoritative 16-bit projection.

The four pure **structure** operations work at 16 bits and at 32-bit float too:
reorder one layer or a whole multi-row drag, Arrange (Bring to Front/Forward,
Send Backward/to Back), group ANY set of layers that share one parent — not
just the whole top level — ungroup, and delete a set. Each is one undo step and
moves no pixel: a layer's samples, its mask and its style are the same data
afterwards.

**Merge, locks and links work at 16 bits and at float too.** Merge Down, Merge
Layers over a multi-selection and Merge Visible composite their members
through the same renderer the picture itself uses, so clip runs, blend modes,
opacities, masks, Blend If and layer styles bake exactly as they look. Merge
Down and Merge Layers keep the union of their layers' extents at every depth,
so anything a merged layer held OUTSIDE the canvas — after a crop, a move or a
smaller Canvas Size — is kept, exactly as at 8 bits; Merge Visible is
canvas-sized at every depth, like 8-bit. One difference remains: at 16 bits
and at float a masked adjustment layer adjusts merged pixels beyond its mask
by the mask's outside coverage, where an 8-bit merge leaves them unadjusted.
The four layer locks —
transparency, pixels, position and Lock All — and link groups behave exactly as
they do at 8 bits, refuse the same edits with the same wording, and are saved
in the `.doctored`. A group's disclosure triangle opens and closes it at every depth,
and the open or closed state is saved in the `.doctored` (and in a 16-bit PSD's
folders) at every depth too. **Layer > New Layer works at every depth**: a 16-bit or 32-bit
float document gets a transparent canvas-sized leaf through the core's own
typed op, at the same slot an 8-bit document's would land at, from the Layers
panel's New Layer button too. **Stamp Visible and
Flatten Image work at every depth too**: both render the document's own
projection into one canvas-sized layer at every depth, like 8-bit, because an
8-bit flatten already renders the canvas. Stamp Visible lands its layer UNCLIPPED directly above
the layer you name, whole group and all, and needs one contributing raster
layer — at 16 bits and above an adjustment layer does not count as one, so
Merge Visible and Stamp Visible grey out there exactly when the core would
refuse them. Flatten Image names its one layer "Background", drops hidden
layers and the document's layer comps, and keeps the channels, guides and
ruler origin.

**Resolution (PPI) can be changed at any depth**, and it resamples nothing at
all: every pixel buffer is literally the same data afterwards. Image Size's
Resample checkbox and Resolution field are live on a 16-bit document exactly
as they are on an 8-bit one.

**Layer ids are saved.** Every `.doctored` this build writes carries one identity
table, so the id a layer has now is the id it has after a save and reopen,
after a crash recovery and after loading a snapshot. Files written by earlier
builds still open and simply get fresh ids; earlier builds refuse the new files
by name rather than misreading them. **Layer comps use that saved id**, so a
comp keeps pointing at the right layers after a save, a reopen, a reorder and a
rename — and a recorded Action's `$layer` target does the same, as long as the
layer still carries the name it was recorded under.

**Guides and the ruler origin work at every depth.** View ▸ New Guide…, Clear
Guides, dragging a guide out of a ruler and the ruler corner box all edit a
16-bit or 32-bit float document exactly as they edit an 8-bit one, with the
same rules: a position is an absolute canvas coordinate, both canvas edges are
legal, and every crop, rotation, flip and Image Size moves the guides the way
it moves the pixels. They are saved in the `.doctored`, restored by a crash
recovery, carried across a precision conversion, and undone one step at a
time. Canvas Size, the crop, the quarter turns and the flips move them at
every depth, and a STRAIGHTEN moves them exactly as its trailing crop does (a
rotated guide is not a guide, so a straighten shifts and drops rather than
turning them). An ARBITRARY canvas rotation moves them the same way —
shifted by the canvas expansion and dropped if they leave it — while its
exact quarter turns keep permuting them, which is the one
visible discontinuity at 90°: 90° swaps each guide's orientation and the print
resolution's axes, 89.999° shifts and drops and leaves the resolution alone.

Retouch can sample Current, Current and Below or All Layers without using display
pixels. A new Retouch layer preserves the original, and native source overlays
use the same matrix as the commit. Clone can paint explicitly expanded transparent
storage; healing preserves destination alpha, so use new-layer output to repair
underlying content.

Image > Mode > Convert Precision prepares a before/after comparison. Its Flatten
choice flattens the captured appearance to one raster. It and the Preserve Layers
choice below both keep the document's saved alpha and spot channels at every depth step
([#1](https://github.com/kpgalligan/doctored/issues/1)). Conversion is one Undo step;
promotion preserves the legacy composite and cannot recover missing source detail.
Its Preserve Layers choice (MCP `convert_precision structure:"preserve"`),
which the sheet opens on unless the document cannot be converted that way,
instead converts keeping every layer, group, adjustment, text/shape description,
mask, channel, style, Blend If, knockout, clipping, lock, link, label, comp,
embedded source, warp, guide and the ruler origin, in every direction that
exists: 8-bit to 16 widens each value by 257, 16-bit to 8 quantizes each record
(RGB nearest or ordered dithering, alpha nearest), and a 32-bit float document
converts to 16 or 8 bits with each record tone-mapped by its own map. Its plan
lists what renders differently, what is not carried and what blocks, `dry_run`
returns that plan without editing or recording, and the preview reports the
measured difference in codes of 65535. Image ▸ Mode ▸ 32 Bits/Channel… opens it
with float chosen (MCP `convert_precision mode:"rgba32"`, Preserve Layers only): an 8- or
16-bit document becomes 32-bit float with each code decoded through the
profile's tone curve to linear light, and converting back returns every code.
Native `.doctored` saves, the existing history, snapshots and recovery retain typed state.
Layer > Place Embedded… and Place Linked… add a PNG, JPEG or TIFF, or a JPEG XL,
HEIC, AVIF, JPEG 2000 or supported still/movie pair, as an ordinary raster layer
at the document's depth, converted into its profile; the file is not kept in the
document (a gain-map HEIC lands as its disclosed SDR base; 12-bit
at 16 bits at full scale, an identity-matrix AVIF, which macOS decodes left-aligned,
widened by bit replication, except
12-bit 4:4:4 HEIC, which still refuses because macOS's decoder returns no pixels for it).
Unsupported HDR or sample
layouts refuse instead of silently using a byte rendering. A camera RAW file
placed either way lands a RAW layer that keeps its original and re-develops;
Place Linked keeps its link. A placed image lands
ABOVE the selected layer by default at every depth; over MCP, `position` can
put it at the top of the stack or inside a selected group instead, and the
reply reports which. Free Transform on a RAW layer or a selected set,
including true perspective, rerenders from the original master; whole-document
resize, crop, turn, flip, straighten and arbitrary rotation keep that binding,
so scaling down and back up avoids cumulative loss.
When the core open does not keep a file's embedded colour profile, Place
shows its sentence once, in a notice
after the layer lands, without undoing it. A host source's disclosure (a
gain-map HEIC's SDR base) shows in the same notice.
Layer ▸ Rasterize RAW Layer
enables destructive editing; Undo restores its source.
File > Export offers PNG/TIFF16 or deliberate 8-bit output with optional ordered
RGB dithering, while retaining the master. Alpha is rounded without dithering.
The export workspace resamples and sharpens at the document's own depth, before
any depth reduction.

The [operation matrix](https://github.com/kpgalligan/doctored/blob/main/designs/phase-7-operation-support.md) lists supported
workflows and refusals. A camera RAW develops to a 16-bit SDR master or, in a 32-bit float document, to a scene-linear 32-bit float one;
`layer_via_copy`/`layer_via_cut` run at 8, 16 and 32 bits.
Native 16-bit raster, group and selected-set content has on-canvas
affine/perspective Free Transform handles; float supports exact integer Move
dragging and nudging (with a selection active it moves the selected pixels,
and `transform_selected_pixels` transforms them, on a layer and on
a mask or alpha channel) and filtered Free
Transform of layers, with each layer's RGB clamped to its own visible range. Editable text/shapes regenerate
from their descriptions after affine, size and profile changes. Generated outlines
use native area coverage; intrinsic color glyphs keep their declared RGBA8 assets
with continuous authored opacity. Native selections and Quick Mask retain 65,536
coverage levels; geometric, legacy brush and Vision inputs keep their disclosed
byte coverage precision. Native scalar painting, bucket fill, tone/spatial filters,
Auto analysis, morphology, Apply Image, Calculations and luminosity masks preserve
exact native planes. Image ▸ Channels ▸ Modify Native Plane… edits the selected scalar target.
Channel isolation and target state survive Undo, snapshots and recovery. Camera
auxiliary mattes retain source-precision/orientation provenance in native channels.
Native paint adds pressure size/opacity and timed airbrush deposits, with exact
versioned canonical dabs for Actions. MCP `stroke_version=1` generates the same
bounded native representation from points, pressure samples and elapsed times;
at 16 bits an unversioned point call generates the same native dabs (unit
pressure, no timing) and rasterizes them natively, so it is subject to both core
limits. Brush/fill ink can expand
raw storage, while erasing absent pixels remains a no-op. Native paint, gradients
and six dodge/burn modes preserve independent u16 selection gates. The explicit
50,000-dab and 2-billion-work-unit limits refuse an oversized MCP stroke by name,
with no partial stroke applied or committed; a stroke drawn on the canvas that
reaches either limit stops laying ink there and commits what was laid. Hard strokes finish at full
resolution: coverage is evaluated over the stroke's own footprint and only the
hard edge band is area-integrated.
At 8 bits, every stamped brush/eraser/clone/heal/spot-heal/dodge gesture records
its resolved deposits, including ordinary mouse soft/flowed/non-round
tips, pressure-size/opacity and held airbrush. Layer, mask, color-plane, named
channel, size-1 pixel and Quick Mask byte strokes share one validated rasterizer
for commit and replay. A 50,001st dab refuses an MCP stroke by name (the
byte MCP walker too, instead of truncating), while a canvas gesture stops laying
ink there and commits the 50,000 dabs laid; 8-bit unversioned point calls keep
their original footprint. `brush_stroke`, `fill`, `gradient`, `fill_selection`,
`stroke_selection` and 8-bit `add_text` take a `color_space` sibling — `srgb` (the default: an authored sRGB hex converted once into the document), `document`
(the hex IS the document's own bytes, which is what `sample_color` reports; at
16 bits each byte v is the exact code v·257) or `display_p3` — so a wide-gamut
pixel can be sampled and painted back exactly; a recording on a non-sRGB
document carries the document spelling. Every other tool refuses `color_space`
by name at every depth rather than ignoring it. Text and shape layer colours are
continuous sRGB numbers instead: `sample_color` also reports `srgb_rgba` and
`srgb_in_gamut` at 8 and 16 bits, and `srgb_rgba` passed as `rgba`, `fill_rgba` or
`stroke_rgba` reuses the sampled colour; layer-style and channel overlay colours
stay authored sRGB hex by decision.
Layer > Rasterize Artwork explicitly preserves the current raster and drops its
description; true perspective can instead take the per-edit
`rasterize_artwork:true` authorization (or UI confirmation), preserving Undo's
editable original while reporting the exact layers rasterized. Text and shape
layers land ABOVE the selected layer at every depth, and `position` chooses
another slot over MCP. Font files
are not embedded: saved pixels remain usable without fonts, while a rerender
requires the installed base font. Public large-document
qualification passes the documented nine-stage 24 MP window/history/save/export
workflow. Arbitrary large multilayer history remains later work.

Known limits: PSD support is 8-, 16- and 32-bit RGB and grayscale, plus
CMYK, Lab (8/16-bit), Indexed Color, Duotone and Bitmap; Multichannel is
refused. RGB files need a supported RGB ICC profile, or an explicit
sRGB assumption when untagged.
A 16-bit RGB PSD
opens as a 16-bit document (the report's `import_precision` says so before you
choose), including one Photoshop wrote itself: a
[Photoshop-authored 16-bit fixture](https://github.com/kpgalligan/doctored/tree/main/core/tests/fixtures/psd_photoshop/depth16)
with rasters, a drop shadow, a raster mask, a group, a carried adjustment, an
uncarried one, a fill layer and an empty layer imports layered and renders
within the tolerances fixed before that run. A 32-bit RGB PSD or
PSB opens as a 32-bit float document, layered or as its composite; a
Photoshop-authored 32-bit file reads back within 1e-3 relative, and a 32-bit
Solid Color fill layer opens with its style, while a 32-bit gradient or
pattern fill layer is refused by name. An Indexed
Color PSD or PSB opens from its stored composite as an Indexed document with
its palette, transparent index and every index exactly as stored
([#1](https://github.com/kpgalligan/doctored/issues/1)). An 8-, 16- or
32-bit grayscale file opens as a Grayscale document of its depth with its Gray
ICC; a CMYK file
opens as a CMYK document, its saved composite or, with Import Layers, its layers (see the CMYK paragraph above).
A Duotone file opens as a Duotone document with its inks, a Bitmap file's
stored composite as a Bitmap document, and a Multichannel file is refused by
name. Ordinary raster masks import as separate editable
planes on layers and retained folders: one -2 Raw/PackBits channel, a 20-byte
mask body, outside default0/255 and enabled state. A mask's rectangle is read
as canvas coordinates on rasters and folders alike; the PSD position-relative
flag is recorded in the import report and not applied, which is how Photoshop
reads it. Coverage stays exact, including off-canvas samples.
Imported masks are explicitly unlinked; PSD linkage is not inferred.
Extended masks, parameters and unsupported mask codecs produce specific
per-file losses. An 8- or 16-bit RGB file's vector mask (`vmsk`) with no
raster mask beside it opens as the layer's vector mask — a shape layer as its
fill layer under the path — unless its path starts from all pixels or
subtracts or intersects its subpaths, which keeps the layer's stored pixels
with a loss; the file's paths (the work path and saved
paths) open with the document in every mode; layer comps (resource 1065 and
each layer's `cmls`) open in an RGB file with their names, comments and
visibility, a comp's position and appearance reported as not imported.
Malformed admitted mask rectangles or rows refuse the layered choice. Photoshop
layer groups retain structure, names, folder opacity, visibility, recognized
blend modes and open/closed state through a bounded metadata walk. Raster
clipping flags import, and both depths evaluate group clipping too; advanced
blending options remain unsupported; layer locks (`lspf`) are carried,
except a Background's implicit locks. Photoshop Invert,
Threshold, Posterize, Levels, Curves, Color Balance, Hue/Saturation, Photo
Filter, Exposure, Vibrance, Black & White and Channel Mixer adjustment layers
(the binary-body records plus the `vibA`/`blwh` descriptor bodies) are carried
as Doctored adjustment layers with their mask, opacity, blend mode and
clipping, and so are Brightness/Contrast (`brit`, its values read from the
record's `CgEd` descriptor) and Selective Color (`selc`); a Photo Filter colour is read from either an RGB body or the Lab body
Photoshop actually writes; and a body outside the mapped subset (an XYZ
photo filter, a CMYK Channel Mixer, an unread
descriptor item type, out-of-range values) is omitted with the reason. Levels
carries its output levels and its red, green and blue records in both
directions, and Photoshop's own renders of per-channel and output Levels match
Doctored's within 3 codes at 8 and 16 bits. A Curves layer
drawn with Photoshop's pencil (the map form) carries its 256-entry maps for
the composite and each channel, renders within 1 code of Photoshop's own
render at 8 and 16 bits, and exports back to the very bytes Photoshop
writes. Their parameters are mapped
onto Doctored's own arithmetic, and how far that appearance differs from
Photoshop's is measured: against a Photoshop 2025
26.10.0 document holding the original eleven, every kind carried its
parameters, Invert matched exactly, Threshold and Curves matched within
tolerances fixed before the run, and the other eight differ by 6 to 127 code
units at their worst pixel. Channel Mixer
was measured separately against Photoshop-authored files: both are the same linear mix on
encoded values, and Doctored's render of a carried mixer is within one 8-bit
code of Photoshop's own. An imported Exposure layer
evaluates through the document profile (`document_v2`) at both depths. A Pattern Fill
layer (`PtFl`) imports as a Doctored pattern fill layer with its tile from the file's
`Patt`/`Pat2` table, and layered export writes one back the same way. A Solid Color Fill layer (`SoCo`) imports as a Doctored solid
color fill layer with its colour, a colour outside sRGB kept as a wide colour, and
layered export writes one back the same way, RGB and CMYK. A Gradient Fill layer (`GdFl`) imports as a Doctored gradient
fill layer with its stops and their midpoints, style, angle, scale, reverse, Smoothness and Align with Layer
(spanning the bounds of its mask's coverage), naming each setting it cannot
hold (the interpolation method, Offset, Dither), and layered export writes one
back in Photoshop's form, RGB and CMYK. Every other adjustment and fill layer carries no pixels and no
settings this importer reads, so Import Layers omits it and its mask, naming
each record and its key; the stored composite keeps their appearance. A record with no pixels and no adjustment key imports as an
empty canvas-sized layer, also reported. A file whose every record is an
adjustment or fill layer opens its stored composite as a single layer. Unknown group blend keys or malformed folder
records refuse layered import, while an independently valid saved composite
remains available as an explicit choice. Groups beyond ten levels still lose
the extra folder rows; pathological depth and record/metadata limits refuse
before layer decoding.
Admitted Raw/PackBits raster layers retain their complete rectangle,
signed offset and exact straight RGBA, including wholly off-canvas pixels and RGB
under zero alpha. A 16-bit file adds the two forms Photoshop itself writes
there: ZIP and ZIP-with-prediction layer and mask channels, and layer records
kept in an `Lr16` tail block rather than in the (empty) layer-information
section. Those two compressions are read at both depths, for layer channels,
masks and the stored composite. Missing alpha means opaque; incomplete color channels,
reversed or one-side-zero rectangles and malformed rows refuse the layer choice;
a rectangle that is zero on both sides carries no pixels and is handled per
record (an adjustment or fill layer is omitted, any other becomes an empty
layer). Raster storage is limited to 30,000 pixels per side, 100 MP per
layer and 400 MP in aggregate. Masks have independent plane/aggregate caps,
and the conservative decode envelope (the working limit) includes retained mask storage and
mask codec scratch.
This bounds new decoding after the file has been read; it does not reserve memory
for unrelated open documents.
A Photoshop layer's effects (`lfx2`, or `lmfx` with stacked instances: up to 10
each of Drop Shadow, Inner Shadow, Stroke, Color Overlay and Gradient Overlay,
which Doctored also writes back as `lmfx`; a Pattern Overlay's
`patternFill` and a pattern stroke read their tiles from the file's
`Patt`/`Pat2` pattern table), fill opacity (`iOpa`) and blending
ranges are imported as ONE Doctored layer style on a raster, a folder or an
adjustment layer, at
both depths, with the file's global light: the effect parameters are mapped
onto Doctored's own renderer and the settings it does not model are ignored,
so the appearance can differ from Photoshop's. More than 10 stacked instances
of one effect, legacy effects (`lrFX`), a stroke or overlay naming a pattern
the file does not carry, Stroke Emboss and a
gradient whose transparency stops do not match its colours are discarded with
their reason; blending ranges on several of gray, red, green and blue are
carried together, and a range naming an alpha channel imports as Gray, which
is where Photoshop writes and renders it. The file's global light becomes the document's own at both depths.
Opening a PSD first presents its compatibility report and offers supported
layers and the author's stored composite when each is available. The report separates
general limits from losses detected in that file. The composite preserves
the saved appearance as one raster layer, with the embedded RGB profile and
PPI when present. Stored composites support raw, PackBits and ZIP compression
(a ZIP composite inflates only the picture's own planes, never a file's saved
channels; the spot channels of a Raw or PackBits composite open as spot channels);
unsupported color/depth refuses explicitly; a 16-bit RGB file
opens as a 16-bit document with either choice, never as a converted 8-bit copy.
MCP uses
`inspect_psd`, then `open_document` with the chosen mode and report fingerprint.
Declared merged transparency is decoded from the PSD preview's white-matted
color into straight RGBA. Very faint transparency can retain less color detail
in the saved preview than in the editable layers; imported layer samples remain
exact.
File > Export as PSD… writes a classic RGB PSD, a Large Document PSB
(past 30000 pixels a side or 2 GiB) or a TIFF with Photoshop's layer block,
chosen in the panel's Format popup, at the document's own
depth — 8-bit from an 8-bit document, 16-bit from a 16-bit one, 32-bit from a
32-bit float one (linear-light samples under the profile's linear twin, as
Photoshop writes them; a reopened file whose profile is a built-in's linear
twin returns to that built-in), never a cross-depth conversion — carrying
either the editable stack (raster layers, ordinary masks, groups, clipping, opacity,
Photoshop blend keys, embedded ICC and PPI, with a white-matted composite
preview) or one flattened composite layer. A per-mode preflight runs before
the panel opens: anything the format cannot hold either refuses that mode with
a reason or is listed, so nothing is dropped silently. A layer style is WRITTEN
as Photoshop's own constructs at every depth (at 32 bits a style that would
bake refuses the layered mode instead) — the effects as an `lfx2`
descriptor, fill opacity as `iOpa`, Blend If as the record's blending ranges,
with the document's global light in image resources 1037/1049 — so Photoshop
renders the effects itself and the layer keeps its own rectangle and its own
mask; how far that appearance differs is measured —
Photoshop 2025 26.10.0 reopened a sixteen-case styled export at 8 and 16 bits,
read every `lfx2` and `iOpa`, and rendered nine of the cases inside tolerances
fixed before the run (colour overlay and the hard-edged offset effects
exactly), with the glows, bevel & emboss, the gradient ramp and the outside
stroke's corners recorded with their gaps (the outside
stroke's corners are within about 1–2 codes of
Photoshop's, and the Inner Glow, the soft Drop Shadow, Inner Shadow and Outer
Glow, the Smooth bevels, Emboss and Pillow Emboss follow Photoshop's measured
laws, [#2](https://github.com/kpgalligan/doctored/issues/2),
as do Linear, Angle, Reflected and Radial gradients and the Shape Burst stroke) — because the
setting Doctored does not model (the glows' technique) is not
written. Only what Photoshop's dialog or
a PSD mask cannot hold falls back: a stroke below 1 px, a
satin distance outside 1–250 px, or an enabled mask with density or feather
RASTERIZES the effects into the layer's pixels, reported per layer with that
reason — the grown rectangle is
written, an enabled mask is baked into it, and Blend If is neither written nor
applied. A styled GROUP is written the same way as a styled layer (a Pass
Through group's effects as `lfxs`, as Photoshop writes them). Invert,
Threshold, Posterize, Levels, Curves, Color Balance, Hue/Saturation, Photo
Filter, Exposure, Vibrance, Black & White and Channel Mixer adjustment layers
are written as Photoshop adjustment records (`nvrt`, `thrs`, `post`, `levl`,
`curv`, `blnc`, `hue2`, `phfl`, `expA`, `vibA`, `blwh`, `mixr`) with their
mask, opacity, blend key, clipping and layer style, at every depth (at 32 bits
an adjustment with no Photoshop record refuses the layered mode instead); their
parameters are mapped onto Photoshop's own arithmetic, and the import-direction
differences per kind are measured (the export
direction's appearance in Photoshop is unqualified; a
Doctored-written `mixr` reopens in Photoshop with the exact rows). A Channel
Mixer's hidden row is not written: under Monochrome Doctored writes only the
gray row (Photoshop keeps Gray in the Red record), and without it the gray row
has no record of its own, so a hidden row that is not at its default is
reported as a loss. Brightness/Contrast (without saturation) and Selective
Color are written as `brit` (with its `CgEd`) and `selc`. Any
other adjustment layer is written as a pixel layer holding the picture up to
and including it, reported as a loss per layer (Gradient Map and Color Lookup
among them: PSD has records for both, but the published format does not define
them well enough to write without a Photoshop-written sample). A rasterization
past 100 MP and a group whose style
must be rasterized refuse instead. Editable text, shapes, alpha
channels, guides, link groups and metadata packets are not
written as live constructs (layer comps are written at 8 and 16 bits RGB with
their visibility, the paths in every mode, and a vector mask with no raster
mask beside it as its path), and a RAW layer is written as its pixels; layer locks are written as Photoshop's `lspf`. A 16-bit
document's clipping is written and evaluated exactly as an 8-bit one's. Appearance in
Photoshop is claimed, at 8 and 16 bits, for Normal, Darken, Multiply, Color
Burn, Linear Burn, Lighten, Screen, Color Dodge, Linear Dodge, Overlay, Soft
Light, Hard Light, Pin Light, Exclusion, Subtract, Hue, Saturation, Color,
Luminosity and Pass Through, and for Difference, Lighter Color and Hard Mix at
100 % Fill only (#2, three Photoshop takes); Dissolve, Darker Color, Vivid
Light, Linear Light and Divide, and a clip base below 100 % Fill, are written
but reported as unqualified, and a CMYK file's claim stays Normal, Multiply,
Screen and Pass Through. Group clipping is written and reopens here clipped,
but Photoshop reads a clipped group as unclipped, which the export reports.
A file past
Doctored's layered decode envelope (the working limit) reopens here as its flattened
composite only — reported before the write. MCP uses
`inspect_psd_export` and `export_psd`, whose result reports the `bit_depth` it
wrote. The export is qualified in an independent application: seven
Doctored-written PSDs — 8-bit layered and flattened, 16-bit, a written layer
style and preserved clipping — reopen in GIMP 3.2.6 with their structure,
names, opacity bytes, blend keys, clipping, mask samples, profile and PPI exact
and their appearance identical, with receipts under
[`core/tests/fixtures/psd_10b/`](https://github.com/kpgalligan/doctored/tree/main/core/tests/fixtures/psd_10b); the layer-style case's GIMP receipt was
re-taken for the written-effects form on 2026-09-16, and it found that GIMP
reads none of the written `lfx2`, so that one case's reopened appearance is
recorded rather than gated.
The bounded report also identifies known style, adjustment/fill, type,
smart/placed, vector and advanced blending metadata markers. It distinguishes
documented defaults from discarded nondefault or unrecognized settings;
marker presence does not imply an active effect. Unknown keys and global
additional-info blocks remain outside this finite inspection.
Animated GIFs load their first frame only, so ⌘S
on a GIF deliberately routes through Save As instead of overwriting the
animation in place. A multipage TIFF opens each page that matches page 1 as a
hidden layer "Page n" (page 1 visible), and the open's warnings name any page
left out; ⌘S then routes through Save As, as for any layered document. A
multipage TIFF whose first page opens as an 8- or 16-bit Grayscale, a Lab Color,
an Indexed Color, a Bitmap or a CMYK document opens that page alone, and its
import warnings say so. Document-level rotate,
flip and resize compose into text and shape descriptions,
so a re-edit after one lands in place. A RAW layer's source geometry uses
its explicit source-layer transform. At 8 and 16 bits, Canvas Size, Image
Size, crop, turn, flip, straighten and arbitrary canvas rotation preserve
RAW masters and compose their source maps; linked files are not read
to reopen the native document. Crop straighten, filtered rotation and Image Size regenerate text
and shape artwork from its description at native depth; missing fonts refuse
preservation. A Live Photo layer from an earlier build opens as its saved
pixels. Movie and Live Photo input is SDR frames from selected codecs, and an
HLG or PQ HEVC frame opened as a 32-bit float document; Dolby Vision
video, animation disposal and broader cross-decoder conformance remain later work. Metadata preservation is per container
and packet: JPEG and PNG retain their existing packet conventions; TIFF adds
standard EXIF/GPS/Interop, XMP and IPTC with embedded ICC and actual document
PPI. TIFF capture fields are relocated safely, a MakerNote travelling as one
opaque value and other private fields reported as omissions. A TIFF export that cannot relocate an EXIF packet
without loss omits that whole packet and reports it. Byte RAW/HEIC still imports
preserve the standard capture fields ImageIO exposes, with XMP serialized separately;
original packet bytes, private camera dictionaries and MakerNotes are outside
that guarantee. Native files retain the resulting packets exactly. WebP
output can carry ICC and EXIF, but its input packets are not scanned. PSD,
Live Photo video frames, BMP and GIF do not contribute capture packets.
Typed RAW imports retain all original metadata inside the encoded
resource, and typed RAW and host-still imports reconstruct the platform reader's
standard capture packets and the file's own PPI into the document, so a typed
export carries them; the exact original bytes, MakerNotes and private camera
dictionaries included, live only inside a RAW layer's retained original file. A selected movie FRAME has no still
carrier to read and still reports that gap. A HEIC export carries the profile,
EXIF, XMP and the ppi; its IPTC is dropped by the system encoder and the export
report says so. A PDF export carries the profile and no capture packets. Native still
and RAW auxiliary mattes become channels with declared decoded coverage precision
and orientation; the import notice reports these boundaries. `get_metadata` and export notices distinguish uninspected sources from
verified omissions. BigTIFF opens at 8-bit, 16-bit and float with its metadata;
a BigTIFF saves and exports as classic TIFF.
Convert to Profile transforms
layer pixels only: layer masks and alpha channels are coverage rather than
colour and are left alone, and so are layer-style and text-layer colours —
those are authored in sRGB like every other colour you type, and they convert
into the document's space where they are used (a style's when it composites,
a text layer's when it re-renders), so an effect's colour matches a fill of
the same hex on any document and survives a convert unchanged. An adjustment
layer's parameters are left alone too by DEFAULT, but for a different reason:
a curve point or a saturation amount is not a colour and there is nothing to
convert it into. That is also why a convert keeps each LAYER looking the same without
keeping every composite the same — a non-Normal blend mode, an adjustment
layer and a style effect that blends are all computed from the numbers the
convert changes, so on such a document the picture visibly moves
(correctly). The sheet and `convert_profile` say which case
the document is in rather than promising it will look identical. The explicit
alternative is `adjustments: "preserve_appearance"`, which keeps `exposure`
and `white_balance` at `document_v2` evaluation — both are light-linear
operations through the document's own profile, so the same parameters mean the
same appearance in any space — and bakes every other adjustment into a raster
layer named `"<name> (baked)"` in its own place, leaving every other layer
where it is. It never flattens the master, it is never silent (the sheet
previews it and names every layer it would bake), it refuses rather than
guessing when an adjustment is inside a group, is clipped or sits over
transparent pixels, and it changes nothing about how an old document
evaluates. Into a profile with NO matrix/TRC model (a LUT-only RGB profile)
a `document_v2` Exposure or White Balance layer cannot be evaluated, so keep
parameters refuses, naming the layer (and offering preserve appearance when
that bake would succeed), preserve appearance bakes a visible one under the
rules above, and a linear-light document refuses the profile outright —
rather than landing a document that cannot render. Assign Profile refuses
the same profiles by name, in its own sentence (Assign never bakes), while
such a layer exists or the document uses linear-light evaluation: the sheet
shows it beside the choice and keeps Apply disabled, and `assign_profile`
errors with it.

Assign Profile refuses by name while a RAW layer is present, because
relabelling would reinterpret the RAW layer's developed master, and offers
"Rasterize N RAW layers, then assign" as one undo step. Convert to Profile on such a
document keeps every source bound when it converts at relative, perceptual or
saturation between two matrix/TRC profiles, or at relative colorimetric to or
from a LUT profile at 8 bits (each source re-renders through the same
relative, no-BPC transform as the pixels); black point compensation, absolute
colorimetric, and perceptual or saturation with a LUT side refuse with a
sentence naming how many sources it cannot keep bound and the setting that
would keep them (relative colorimetric, black point compensation off); the
Convert sheet's "Rasterize N RAW layers, then convert" (`rasterize_sources`
over MCP) rasterizes them and converts as one undo step. LUT-based (A2B/B2A)
profiles whose colour tables this build cannot read are kept and re-embedded but
cannot be converted from, and a
document tagged with one is painted in "the numbers are the numbers" mode:
every authored colour lands as the sRGB number you typed, from the brush, a
fill and a layer-style effect alike, so one hex is one colour throughout.
Display, relabelling and re-embedding stay exact. On a document tagged with a
LUT profile this build converts both ways, layer-style, text, shape, brush,
fill and gradient colours are converted from sRGB into the profile at 8 and
16 bits, so they keep their appearance, while paint on a
mask, Quick Mask or channel still reads its gray in sRGB, so white writes
255. A LUT profile whose tables
this build CAN read (v2 `lut8`/`lut16`, v4 `mAB `/`mBA `) is a full conversion
source and destination at 8 bits only, and `get_color_profile` reports both bits:
`convertible` stays the narrower matrix/TRC question the calibrated adjustments
ask, and `can_decode` is the wider one. At 16 and 32 bits both profiles of a
conversion must be matrix/TRC, so there `can_decode` asks the matrix/TRC
question, `convertible` adds the high-depth precision checks, and the Convert
sheet offers only matrix/TRC destinations that pass them. A matrix/TRC profile
whose tone curves fail those checks can be assigned and converted OUT of at
every depth (gamma 0 reads as linear, a falling table as its running maximum),
while Convert to Profile into it is refused up front with a sentence naming
it. Assigning one is admitted at every depth — with or without "Rasterize N
RAW layers, then assign", since a relabel encodes nothing — unless the
document uses linear-light evaluation, holds text or shape layers, or holds
a calibrated (document_v2) Exposure or White Balance layer, which would have
to encode into it or evaluate through it: each refuses up front, at every
depth, with one Assign sentence naming the profile and what to change (switch
Evaluation back to Encoded, rasterize or delete the artwork, or merge down or
delete the named calibrated layer). An edit that would encode colour INTO a document's own such profile — 16-bit
paint, text and shape colours, linear-light evaluation, a soft proof, a placed
image's 16-bit render, a converted paste or a Color Lookup abstract look — is
refused with one sentence naming the profile and suggesting Image > Mode >
Convert to Profile…; there is no fallback encoder. (A new or re-edited
calibrated Exposure or White Balance — layer, Options edit, destructive apply —
is refused with that same sentence at every depth, and an 8-bit editable paste
of one pastes as pixels with it as the reason; an existing 8-bit layer keeps
rendering, so a saved document keeps its look and merge down remains the exit.) A profile the LUT parser cannot read is refused with a sentence naming what it really is,
checked class first: a named-colour profile (a list of colours), a device-link
or abstract profile (a transform), an `nCLR` separation space (named by its
colourant count, `4CLR` included) or any other space but RGB, Gray and CMYK, a
`multiProcessElements` table, a profile whose only tables are the
floating-point `D2Bx`/`B2Dx` (the tag is named), and a colour table past
64 MiB. Proofing through `D2Bx`/`B2Dx` tables and `nCLR` separation
profiles is not built. Lab profiles are still refused as DOCUMENT
profiles (a Lab Color document holds the built-in ICC Lab encoding instead), a CMYK profile is one only of a CMYK document and a Gray profile
one only of a Grayscale document
([#1](https://github.com/kpgalligan/doctored/issues/1)) — an RGB
document's pixels are RGB — but a Gray or CMYK profile is a
perfectly good PROOF profile, which is what soft proofing reads them for. So
are device-link and abstract profiles, which describe a transform, and
named-colour profiles, which are a list of colours, rather than a space to
read pixels in. Missing EXIF resolution and
orientation tags are patched only where they already exist, never inserted
— inserting one would shift every out-of-line datum and corrupt a
MakerNote. When an EXIF resolution cannot be patched in place (both axes
sharing one stored value on a document whose two axes now differ, say), the
whole EXIF block is dropped and reported dropped rather than written
contradicting the density beside it: a packet is in authority for as long as
it exists — `sips` and ImageMagick answer with its numbers and ignore the
JFIF header even when its unit says "none" — so handing the density back its
authority means not writing the packet. In the
XMP packet only the `tiff:` orientation, resolution and resolution-unit
properties are rewritten, and only where the packet already carries them
under that conventional prefix; the only properties removed are Google's
pixel-bound `GImage:`/`GDepth:` ones and a stale `xmpNote:HasExtendedXMP`.

## Built-in assistant

The Assistant tab of the right panel (View > Assistant, ⌃⌘A) is a chat
agent built into the app: it edits the window's document by calling the
same tools the MCP server exposes, sees the canvas by rendering it, and
verifies its own work. The agent loop runs in a bundled helper,
`Contents/Helpers/doctored-agent`: a Swift executable built from
`agent/` on the Slaude Claude API client, which streams replies and
the model's progress notes into the panel, caches the prompt, retries
rate limits and overloads, and cancels at any point. The helper owns no
tools — each call comes back to the app over the helper's stdio and runs
through the same dispatcher as MCP. Most tools are found through the
API's tool search rather than loaded up front (only `render`,
`get_document`, `list_documents` and `open_document` load at the start), and the
conversation is append-only (earlier messages are never rewritten) except
for compaction: once a turn ends with the context past the auto-compact
limit (300,000 tokens by default), the API summarizes the conversation and
the summary replaces it. The panel's context meter shows tokens in use
against the model's window. Every assistant edit is a normal undo step.

Chat Settings (the panel's … menu) holds the API key and context
management: the auto-compact limit or turning it off, and an opt-in to
let the API clear old tool results past 150,000 tokens (tool search
results are never cleared). Bring your own API key: it is stored
user-only (0600) in `~/Library/Application Support/Doctored/anthropic_api_key`,
and changing or removing it starts a new conversation. Launching with
`ANTHROPIC_API_KEY` set also works, and wins. A key an earlier build
kept in the keychain migrates into that
file on the next launch — one final keychain prompt, then never again.
The model defaults to `claude-opus-5-5`; override with
`defaults write com.kgalligan.Doctored AssistantModel <model-id>`.

## AI agent access (MCP)

Any MCP client can drive the editor: open, inspect and render documents (the
agent *sees* the image as PNG), edit layers, masks, selections and channels,
paint and retouch, run filters and adjustments, manage colour, export, and run
Actions. Agent edits run on the main thread through the same edit path as the
UI: each tool call is one undo step, marks the document edited, and updates the
open window live. The [MCP tool reference](/reference/mcp-tools/) lists every
tool and its arguments.

An MCP client reaches Doctored through `doctored-mcp`, a stdio MCP server
bundled inside the app at `Doctored.app/Contents/Helpers/doctored-mcp`.
**Settings ▸ MCP** shows the helper's path and a **Copy MCP Command** button that
puts the line adding it to Claude Code on the pasteboard:

```sh
claude mcp add --scope user doctored -- '/Applications/Doctored.app/Contents/Helpers/doctored-mcp'
```

Any other stdio MCP client runs the same helper path as its server command.
Listing the tools never launches the app; the first tool call that finds no
Doctored running opens its own bundle in the background under a lock, so
concurrent clients start one instance. The helper reaches the app over a Unix
socket the app binds at every launch,
`~/Library/Application Support/Doctored/Agent/agent.sock`, in a `0700` folder.
There is no TCP endpoint.

The socket's folder mode is its access control: web pages, other user
accounts and sandboxed apps cannot reach it, while any process running as you
can, with no further client authentication. Every tool argument that names
a file or folder is checked before the tool runs, for MCP clients, the
Assistant, Actions and Batch alike: a path that resolves (after `~`, `..` and
every symlink) into a sensitive location — credentials, keychains, browser
profiles, Doctored's own support folders, `/etc` or `/System` — is refused
in-band with one sentence naming it, and so is a write into an `.app` bundle
or `/Applications`.
