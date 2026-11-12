//! Phase 13A's REGIONAL DOCUMENT CONTRACT: a bounded, cancellable render of
//! one canvas window — or a nearest-sampled preview of it — of a layered
//! document at 8 bits (`doc_regional`), 16 bits and 32-bit float
//! (`precision_regional`), equal SAMPLE FOR SAMPLE to the full-frame
//! projection (`dr_doc_flattened` / `dr_precision_doc_render`) cropped and
//! sampled the same way. The full-frame projection is untouched and stays the
//! reference.
//!
//! ADMITTED (13A-1 + 13A-2a + 13A-2b + 13A-3a + 13A-3b-i + 13A-3b-ii +
//! 13A-3c + 13A-4a + 13A-4b-i + 13A-4b-ii-a + 13A-4b-ii-b-1 +
//! 13A-4b-ii-b-2): a TREE of
//! LEVELS — the root stack and, since 13A-3a, every GROUP's children, nested
//! to the depth cap — of rasters (any offset, any blend mode the depth
//! admits, any opacity), POINTWISE adjustments (`Adjustment::is_pointwise`:
//! every op except Shadows/Highlights whose guide is read) — and, since
//! 13A-4a, a GUIDED Shadows/Highlights at the ROOT, since 13A-4b-i in
//! ISOLATED groups and MATERIALIZED subtrees too, since 13A-4b-ii-a directly
//! inside PASS-THROUGH groups, since 13A-4b-ii-b-1 as a CLIP MEMBER of an
//! UNSTYLED base and since 13A-4b-ii-b-2 of a STYLED one but the 8-bit
//! package (below) — and GROUPS, isolated or
//! pass-through (plain, or gated by an opacity below 1 and/or an enabled
//! mask), as lone entries, clip bases, clip members and baseless clipped
//! entries, hidden or empty; each entry with or without a layer mask (raw
//! placement, density and feather; enabled or disabled), grouped into CLIP
//! UNITS, plus Blend If on a raster or isolated group (lone, clip base or
//! member) at every depth — typed Blend If on the layer. Since 13A-3b-i EVERY
//! layer style on a RASTER at 8 and 16 bits — enabled effects (the planes
//! `LayerStyle::rendered_with_job` / `render_effects_with_job` build, the reference's own
//! preparation stage), fill opacity, Blend If, disabled effects — as a lone
//! entry, a clip base, a clip member and inside groups, and an effect-less
//! style (fill opacity, Blend If) on a GROUP at both depths. Since 13A-3b-ii
//! EVERY layer style on an isolated GROUP at 8 and 16 bits — a group whose
//! style has an enabled effect is MATERIALIZED (below) — lone, as a clip
//! base, as a clip member, inside other groups and nested. Since 13A-3c a
//! KNOCKOUT (Shallow / Deep) and "Blend Interior Effects as Group" (below) at
//! 8 and 16 bits, wherever a style sits. A float
//! document's styles render regionally through the same typed path, their
//! colour and effect blend taken from `StyleEnv::float_style`. Everything else is
//! REFUSED by name — there is no full-frame fallback — with the sentence
//! [`refusal`] builds.
//!
//! GUIDED SHADOWS/HIGHLIGHTS (13A-4a). A Shadows/Highlights whose guide is
//! read is admitted at the ROOT level, 8 and 16 bits, as a lone entry or a
//! baseless clipped entry (8 bits: a base with no positional run; typed: the
//! lone arm, a hidden-only run included), EXACTLY (tolerance declared before
//! measuring). It is evaluated through STAGES (`regional_guide`): the layers
//! below it are evaluated at divisor 1 over a declared plane `R_{k−1}` —
//! the next stage's region (the request window for the last guide) grown by
//! the guide's reach and aligned outward to the guide's cell lattice times
//! `style_render::SUB_PLANE_ALIGN`, clamped to the canvas — the reference's
//! own guide is built from that plane (`Adjustment::guide_with_job` /
//! `PrecisionAdjustment::guide`, `adjust_guide::build`), and the reference's
//! own sample reads it (`adjust_backdrop_sample` / `adjust_backdrop_sample16`
//! with the guide value in place of `0.0`). The plan declares every stage —
//! `"stages": [{"layer", "plane", "cell_pixels", "reach_cells"}, …]`, `layer`
//! the guided row's LAYER index (never a unit position) — and
//! `"border_policy": "canvas_clamped"`: a plane meets the reference's own
//! edge handling at the canvas edge and nowhere else. The guided row is
//! `Spatial` with `max(mask pad, factor · (reach_cells + SUB_PLANE_ALIGN))`.
//! D5's Background split runs in stage 0 and its snapshot is held as one more
//! plane over `R_0`. A guided BASE over a non-empty run contributes nothing
//! (its clip buffer is transparent) and keeps its ordinary row (no stage, no
//! guide halo).
//! STATED COST: a PREVIEW below a guided row still evaluates its stages at
//! divisor 1 — no resolution cut — so its work is `Σ |R_k| · rows_k` plus
//! the guides, not `pixels · rows`; a CLIP MEMBER's stage counts its
//! `rows_k` from its unit's FIRST row (13A-4b-ii-b-1: every sample of its
//! fill re-runs the unit from the base, so the charge is never under).
//! No document-compositor operator is global (decision in the status doc).
//!
//! NESTED GUIDES (13A-4b-i). A guided Shadows/Highlights that is a lone unit
//! (8 bits: no positional run; typed: no visible member) of an ISOLATED
//! group's level — nested to the depth cap, at any unit position, the group
//! itself lone, a clip base, a clip member, inside plain or gated
//! pass-through groups, masked, styled without an effect — is staged exactly
//! as at the root, per LEVEL: its planes are aligned relative to and clamped
//! by the level's BUFFER (8 bits: the group's rect, `rendered_group`'s
//! buffer, whose guide lattice is buffer-anchored; typed: the canvas, since
//! `render_layers` renders a group into a canvas-sized plane), its last
//! region `R_m` is the enclosing level's region for its unit intersected
//! with the buffer, and its LAST stage is a divisor-1 plane over `R_m`,
//! HELD and read in place of the group's children at every sample
//! (`Source::Staged` / `Node.staged`). Every nested level is staged before
//! the root loop (`stage_nested`); a window that misses an 8-bit group's rect
//! stages nothing for that level. A guided Shadows/Highlights anywhere below
//! a MATERIALIZED group — sticky down every descendant level, isolated or
//! pass-through — is admitted and never staged: the reference renders that
//! subtree. The plan's `"stages"` list every staged level's stages, level
//! by level in pre-order, each with its `"level"` (null at the root, else
//! the group's index) and `"buffer"`. STATED COST: a nested level's last
//! stage is at divisor 1 over window ∩ buffer even for a preview. Directly
//! inside a pass-through level a guide is staged since 13A-4b-ii-a, as a clip
//! member of an unstyled base since 13A-4b-ii-b-1 and of a styled one since
//! 13A-4b-ii-b-2 (both below).
//!
//! PASS-THROUGH GUIDES (13A-4b-ii-a). A guided Shadows/Highlights that is a
//! lone unit directly inside a PASS-THROUGH group — plain or gated by an
//! opacity below 1 and/or an enabled mask, pass-through groups nested, at
//! the root or inside an isolated group's level — is staged by the ENCLOSING
//! Root / Isolated level, EXACTLY (tolerance declared before measuring). A
//! pass-through level is never a staged level of its own: the walk ABSORBS
//! its record into the enclosing one (`regional_guide::LevelRecord::absorb`),
//! so its guided rows are stages of that level and its isolated children
//! that level's children. The EXACTNESS argument is 13A-4b-i's unchanged: a
//! plain pass-through's children composite straight onto the enclosing
//! accumulator (`doc_group::composite_unit_into`'s pass-through branch), a
//! gated one's onto `acc.to_vec()` (`composite_gated_pass`), the typed arm's
//! always onto a canvas-sized copy (`precision_render::render_layers`'
//! `PassThrough` arm) — in every case a buffer with the ENCLOSING buffer's
//! size and origin — and the reference builds the guide from that buffer, so
//! the enclosing level's lattice and clamp apply. The stage plane holds the
//! working sample at the guided unit's CURSOR (unit positions down through
//! the pass-through children — byte `ResolvedLevel::path_of_layer`, typed
//! `guided_paths`): the enclosing accumulator, or — gated — the copy, which
//! starts equal to it. The sample as each pass-through group's children
//! began, which the FINISH lerps back to (`blend::gate_source_into` /
//! `precision_render::gate_pass_through_sample16`) and a Shallow child
//! punches to, is HELD as a plane over the region of the fill that ENTERS the
//! group (`regional_guide::Holds`, filled by the chunked `plane` /
//! `zero_plane`) and read by every later fill until the one that FINISHES it
//! — at 8 bits only where it is read (`pass_through_holds`: the gate or a
//! visible Shallow child), at typed depth always. Each fill runs the units
//! from one cursor to the next (`sample_between`). The plan's stage gains
//! `"through"` (the pass-through groups between the level and the row,
//! outermost first; `[]` for a level's own row), and each distinct held
//! group charges one more plane over `R_0` (scratch `16·|R_0|`, work
//! `|R_0|` — the fill; conservative, a hold is never dropped early). A
//! pass-through clip base with a VISIBLE member (and every member) is
//! isolated at both depths and staged as its own level, as before.
//!
//! CLIP-MEMBER GUIDES (13A-4b-ii-b-1). A guided Shadows/Highlights that is a
//! CLIP MEMBER of an UNSTYLED base (8 bits: `style_of(base)` is `None`, the
//! base's unit not dropped for an empty extent; typed: the base carries no
//! style — a Blend If base is admitted, its members clamped by `min`) — at
//! any admitted level, pass-through groups between included — is staged by
//! that level, EXACTLY (tolerance declared before measuring). The reference
//! builds the member's guide from the unit's PRIVATE buffer, which is
//! accumulator-aligned at 8 bits (`doc::composite_clip_group_into`,
//! `doc.rs:579`) and canvas-sized at typed depth (`precision_render.rs:866`),
//! so the level's lattice and clamp apply unchanged. CURSORS: unit positions
//! down through pass-through bases, optionally ending in the member STEP of
//! the last unit, which is then a CLIP unit (8 bits a `clip_run` with a
//! non-pass-through base, the step index counting clamp-only steps; typed
//! `full` over a non-empty `members`) — an element after a pass-through unit
//! is a child unit, after any other unit a member step, and a member cursor
//! is always terminal. The fill that ENTERS the unit (`to` ends in its member
//! step) holds the sample as the unit BEGAN (`regional_guide::Holds`, one
//! plane over the fill's region; the 8-bit hold schedule, `enter_holds`, also
//! enters a clip unit whose member step ends `to`, found by `clip_unit_at`)
//! and leaves the unit BUFFER at the cursor
//! in the stage plane — the guide's input only. Every later fill that
//! RESUMES it RE-RUNS the unit from that held sample (the base, then every
//! member with its guide set) to the next member cursor in it or to its
//! FINISH (`acc = began`, then the buffer composited as ever) —
//! `ResolvedUnit::sample_clip_until` / `precision_regional_tree::
//! clip_unit_sample`, the ordinary clip-unit statements extracted. The plan
//! declares nothing new (`layers[].clip_base` names the base); each distinct
//! clip unit holding a guided member charges one more plane over `R_0`
//! (scratch `16·|R_0|`, work `|R_0|`), and a member stage's rows count from
//! the unit's first row. Inside a materialized subtree the reference renders
//! it (no stage, the halo declared).
//!
//! STYLED-BASE MEMBER GUIDES (13A-4b-ii-b-2). The same guided member under a
//! STYLED base — fill-only, effects, knockout, a materialized group base — is
//! staged the same way, EXACTLY at 8 and 16 bits (tolerance declared before
//! measuring): the reference builds the guide from the style's own private
//! unit buffer — the styled-direct member loop (`style_composite.rs:422-436`,
//! typed `precision_style_render::composite_direct_into`) or the routed
//! unit's (`style_knockout.rs:660-682`, typed `precision_style_knockout`),
//! accumulator-sized either way, so the level's lattice and clamp apply. The
//! staged cursor's member index is a STOP: `styled_sample` /
//! `styled_sample16` run the Below contributions, then the base's pixel (or
//! unit) pass and the members before the stop, leave the private buffer in
//! the sample — outside the layer rect too, where the members composite over
//! the transparent unit clamped to the footprint's 0 — and return before
//! `composite_source_into` / `K` and the Interior contributions; the FINISH
//! re-runs the whole styled sample from the held began sample
//! (`ResolvedUnit::sample_clip_until`, which dispatches a base with planes or
//! a routed style to `styled_sample`; typed `clip_unit_sample` any styled
//! node to `styled_sample16`). The TYPED package is one canvas-sized copy:
//! under a stop the copy holds the buffer, with no window test and no mix.
//! The 8-bit PACKAGE — planes, opacity below 1, not Dissolve —
//! (`composite_package_into` runs its members per 256-row STRIP) refuses
//! "is Shadows/Highlights with a guide under a packaged styled base (8-bit
//! effects below opacity 1)" (owner 13B). No plan field changes.
//!
//! KNOCKOUT (13A-3c). A style the reference ROUTES to its unit pass
//! (`style_knockout::routes_unit`: a knockout, or a grouped interior — D6's
//! five grouped contributions, on a clip base with visible members too since
//! P7-39 lifted the narrowing) runs `composite_interior_unit_into` ONE SAMPLE
//! at a time (`regional_knockout`): the pixel pass into a private unit sample
//! (at fill 1 under D6), a clip base's members frozen to the footprint, then
//! the grouped contributions, the alpha restored after each, then `K =
//! over(B0, U, q, M)` and `mix(acc, K, s·opacity)` — Below and the remaining
//! Interior contributions and the package exactly as for any style. `B0` is
//! the reference's own `KnockoutBackdrop` over ONE-element slices, read by
//! `backdrop_at` at the SAME sample — pointwise, so no plan field changes
//! (`contract_version` 1) — and is born where the reference births it: D5 at
//! the ROOT (8 bits: `composite_projection_into`'s test — some visible style
//! asks for a knockout, entry 0 is the Background, the root's first unit is
//! entry 0 alone; typed: the scene's own `KnockoutSeed::Projection {
//! background }`), the first unit sampled with no backdrop and every later
//! unit against the sample right after it as BOTH planes; D4 in a
//! pass-through group (plain or gated) the sample as its children began as
//! the Shallow plane — handed ALWAYS, as the typed arm does, since only a
//! visible Shallow entry of that level reads it, exactly when the byte
//! reference snapshots it (`level_wants_shallow`) — Deep riding through;
//! `NONE` into an isolated or materialized group (the hard stop); and for a
//! clip MEMBER the unit sample right after the base's pixel pass, as BOTH
//! planes, in all three member loops (`member_env`). Under `linear_light` the backdrops are the
//! linear accumulator's own samples, so a knockout needs no transfer of its own.
//!
//! STYLES (13A-3b-i). A styled sample runs `composite_styled_into`'s order at
//! one sample, target-agnostic (the accumulator for a lone node, the unit
//! buffer for a member): the underlying sample read before anything (the
//! Blend If snapshot's value everywhere the pixel pass reads it); each Below
//! contribution whose plane pixel lands on the sample
//! (`composite_contribution_sample`); the pixel pass (`composite_pixels_sample`
//! / `composite_pixels_sample16`) onto the target, or — a clip base — into a
//! private sample at Normal/1, the members, the alpha clamped to the planes'
//! shape (or the pad-0 shape without planes) after every step, then
//! `composite_source_into` at `alpha · opacity`; each Interior contribution.
//! At opacity below 1 with planes and a non-Dissolve blend the PACKAGE: inside
//! the affected window (`affected_window` within the enclosing accumulator)
//! the steps run at opacity 1 on a copy of the sample, mixed back by
//! `mix_premultiplied` — always inside the window, even where nothing landed
//! (the mix is not an identity in f32); outside it the sample is untouched.
//!
//! MATERIALIZATION (13A-3b-ii). An isolated group whose style has an enabled
//! effect is rendered BEFORE the loop exactly as the reference renders it,
//! then sampled as a styled RASTER through the STYLES statements above. At 8
//! bits `doc_group::rendered_group` itself builds the synthetic raster over
//! the group's extent within the enclosing rect (charged against the
//! render's `RenderBudget`), and its own cropped mask, its style (planes by
//! `LayerStyle::rendered_with_job` over the synthetic) and its blend are read from
//! it; an empty extent drops the unit or the step as an empty isolated
//! group's. At typed depth `render_styled`'s own group buffer: the children
//! rendered by `precision_render::render_layers` into a canvas-sized plane,
//! the planes built over it (`typed_shape` + `render_effects_with_job`), the pixel
//! pass reading the buffer. The children are walked for the refusals and the
//! plan rows only; the reference renders them. Materialization is the
//! reference's own preparation, polled at 8 bits through the env's job since
//! 13A-5b-ii-a (`rendered_group`'s chain, `CompositeEnv::job`, under the
//! materialization's range in `resolve_materialized` since 13A-5b-iii-b-2)
//! and by `render_layers` per layer and per 4096-pixel chunk at typed depth;
//! the planes built over it poll at both depths (13A-5b-i). A cancel returns
//! as the CANCELLATION MODEL (below) bounds it — within one checkpoint of
//! the running byte `rendered_group`. The BUDGET COUNTERPART: the 8-bit plan
//! dry-runs, for every materialized group the reference renders, the
//! reference's own charge and nesting points inside it
//! (`doc_regional_style::materialization_charges`) against the render's
//! group budget, after the walk (structure refusals first), so plan and
//! render refuse with the reference's sentences ("rendering exceeds the
//! group buffer budget (N pixels); …", "rendering exceeds the group nesting
//! limit") and never refuse a render the reference completes. A member of a
//! unit whose base group has an empty extent is never charged (the reference
//! returns before resolving it).
//!
//! GROUPS (13A-3a). At 8 bits a group's style is read through `Layer::style`
//! (a group's meta is never interpreted) and its isolation through
//! `doc_group::is_isolated` (which reads `renders_style`), exactly as
//! `composite_unit_into` decides it: a clip base isolates when a member is
//! VISIBLE, a baseless clipped group passes `baseless: true`, and a group
//! MEMBER is always isolated (`resolved_member`). At typed depth the scene
//! builder's own rule: a base isolates when a member is visible, a member
//! always. An isolated group's sample is its children's level evaluated on a
//! transparent sample, encoded at the boundary as `rendered_group` encodes it
//! (`doc_evaluation::encode_sample`, then decoded again by
//! `composite_layer_sample` — the byte group is a synthetic RASTER over its
//! extent, Pass Through meaning Normal there), or at typed depth kept in f32
//! (`composite_group_sample16`). A pass-through group's children composite
//! straight onto the enclosing sample, or — gated by a sub-1 opacity or an
//! enabled mask — onto a copy of it lerped back by `gate_source_into`
//! (`composite_gated_pass`; typed `gate_pass_through_sample16`, which gates
//! always). A byte group whose extent is empty (nothing visible inside, or
//! nothing inside the canvas) renders to NOTHING, as `rendered_group`'s
//! `None`: as a base its whole unit contributes nothing, as a member it is
//! dropped from the member loop like a hidden one.
//!
//! UNITS: a unit is an unclipped entry plus the consecutive clipped run above
//! it, or a single baseless clipped entry below the level's first unclipped
//! entry, which never has members — at EVERY level. At 8 bits a hidden member
//! is dropped from the unit's member loop, while a VISIBLE zero-opacity member
//! stays in it as a CLAMP-ONLY step: `composite_layer_into` returns at once for
//! it, but the reference's post-member alpha statement (`clip_unit`'s Settle
//! against the styled base's shape or the base's alpha) still runs — it has
//! no plan row and no work unit. Consecutive baseless entries are each their
//! own unit (`doc_group.rs:752-770` composites each with `&[]` and `i += 1`;
//! `precision_document::render_layers` likewise). The byte sampler chooses a
//! unit's path by the base's CONTRIBUTION FIRST, as `composite_unit_into` tests
//! `!is_isolated` before anything else: a PASS-THROUGH base always composites
//! straight (or gated) onto the sample and its members — all hidden, or it
//! would have isolated — are ignored; `clip_run` is inspected only for a
//! raster, adjustment or isolated base. A byte unit's `clip_run` is the
//! POSITIONAL run (hidden members included, `composite_clip_group_into`'s
//! `group.is_empty()`). Typed depth uses `scene.layers` as the reference's own
//! builder produced them: `clipped_members` holds the run's `visible` (uncut)
//! records regardless of opacity, so "the run is non-empty" is
//! `!clipped_members.is_empty()` — the reference's test, and it DIFFERS from 8
//! bits: a run of only hidden members is EMPTY at typed depth, NON-EMPTY at 8
//! bits (a pass-through base keeps pass-through at both).
//!
//! REFUSAL ORDER — FIXED, bottom-first over the UNITS of a level, recursing
//! into a group's level where step 2 reaches it:
//!
//! 1. the UNIT SKIP — a hidden base (8 bits: or one at opacity 0) skips its
//!    whole run AND every subtree in it unchecked; a hidden member (8 bits: or
//!    one at opacity 0) is skipped but, at 8 bits, still makes the run
//!    non-empty (a visible zero-opacity one is a clamp-only step);
//! 2. on every contributing entry, in order: a GROUP's children level is
//!    walked (this same order) and its row pushed AFTER theirs; a clip
//!    MEMBER that is a guided Shadows/Highlights is checked right after its
//!    row — at 8 bits under a PACKAGED styled base (planes, opacity below 1,
//!    not Dissolve) it refuses "is Shadows/Highlights with a guide under a
//!    packaged styled base (8-bit effects below opacity 1)" (owner 13B;
//!    under any other base, at the root, in an isolated group's level and
//!    directly inside a pass-through level it is staged, inside a
//!    materialized subtree the reference renders it).
//!
//! The "is a group" / "is inside a group" refusals of 13A-1…13A-2b are gone,
//! and so are 13A-1…13A-3a's "carries a layer style", 13A-3b-i's "is a group
//! with layer-style effects" and 13A-3b-ii's "carries a knockout" and
//! "blends interior effects as a group" (13A-3c). A byte group at the
//! reference's depth limit refuses with the reference's own sentence
//! ("rendering exceeds the group nesting limit"). Planning raises every
//! refusal rendering can (the document's evaluation-policy walk, the
//! calibrated-context check and the walk above), so a plan that succeeds
//! names a render that will not refuse for structure; the render reuses the
//! walk's tree.
//!
//! SAMPLING: output pixel (i, j) is canvas pixel (x + i·d, y + j·d) — the
//! top-left pixel of each d×d cell, the Phase 4 contract's convention
//! (`nearest_top_left_cell`); the output is ceil(w/d) × ceil(h/d). Divisor 1 is
//! authoritative; any larger divisor is a preview. The window is walked
//! SAMPLE-MAJOR: each ROOT sample evaluates the whole tree (every row) before
//! the next, so a cancel returns within 4096 samples of the running stage loop
//! (nested and pass-through paths and hold fills included), within one
//! checkpoint of the running byte mask or guide blur (13A-5a) or style plane
//! render at either depth (13A-5b-i), or within one checkpoint of the running
//! byte `rendered_group` (13A-5b-ii-a; the CANCELLATION MODEL below). Each
//! sample runs the reference's own per-pixel bodies
//! (`doc::composite_layer_sample`, `style_composite::composite_pixels_sample`,
//! `style_composite::composite_contribution_sample`,
//! `style_composite::mix_premultiplied`, `style_render::Shape::coverage_byte`,
//! `doc::adjust_backdrop_sample`, `blend::composite_source_into`,
//! `blend::gate_source_into`, `doc::quantize_sample` /
//! `doc_evaluation::encode_sample`,
//! `precision_render::composite_raster_sample16`,
//! `precision_render::adjust_backdrop_sample16`,
//! `precision_render::composite_group_sample16`,
//! `precision_render::gate_pass_through_sample16`,
//! `precision_style_render::composite_pixels_sample16`,
//! `precision_style_render::typed_coverage_byte`,
//! `style_blend_if::source_weight_16`, `precision_render::checked_alpha`,
//! `style_knockout::backdrop_at`) and reads a mask through the reference's own
//! coverage (`Layer::effective_coverage_at` over `MaskProperties::derived_with_job`;
//! `PreparedMask::at` over `precision_render::prepare_mask`), so exactness is
//! the one tolerance. A clip unit runs `composite_clip_group_into`'s
//! private-buffer statements per sample.
//!
//! STATED DIFFERENCES from the full render: a window refuses (a sub-floor
//! alpha, a nonfinite kernel output) only when a refusing sample lies inside
//! it, where the full render refuses when one lies anywhere; and only a
//! MATERIALIZED nest allocates (and is charged against
//! `doc_group::MAX_GROUP_BUFFER_PIXELS` / `RenderBudget` as the reference
//! charges it): a reference budget refusal caused by an unmaterialized level
//! — an isolated or gated group the regional path samples per sample, a
//! zero-opacity isolated base whose extent (and depth limit) the reference
//! still charges, or a visible zero-opacity isolated group MEMBER of an
//! unmaterialized unit (one more isolated group the regional path never
//! materializes, no different from an opacity-1 member sampled per sample;
//! inside a materialized nest `unit_charges` pays it because the reference
//! renders that nest) — does not refuse regionally. So too the two KNOCKOUT
//! snapshots the reference holds while every later unit renders: D5's
//! Background snapshot (`style_knockout::composite_projection_into`, canvas
//! pixels held across the root level after entry 0) and an UNMATERIALIZED
//! pass-through level's Shallow snapshot (`doc_group::composite_unit_into`,
//! gated by `level_wants_shallow`; inside a materialized nest `unit_charges`
//! already pays it). The budget bounds memory a nest HOLDS, and the regional
//! path holds neither (per-sample one-element backdrops; where the staged
//! path does hold `S` or a pass-through hold over `R_0`, `stage_scratch`
//! charges it); a dry-run charge for an allocation that never happens would
//! refuse renders that fit, so the difference stays one-directional — the
//! regional path admits a superset — by design (13A-5c-3). Since 13A-5c-3 a
//! materialization's plan term also includes the reference's UNCHARGED
//! per-unit buffers inside the nest — unit buffers, package strips and their
//! knockout windows, the run base's guide, a pass-through's mask crop, and
//! each nested synthetic's RGBA8, mask crop and planes
//! (`doc_regional_budget::materialization_charges`' third figure, a stated
//! conservative upper bound).
//!
//! EVALUATION NEEDS: the plan declares one [`EvaluationNeed`] per contributing
//! layer, groups included (`group` names each row's enclosing group). A layer
//! whose ENABLED mask is feathered declares `Spatial` with the feather's blur
//! reach (the `padded_dimensions` pad) as its halo, a staged guided row
//! `Spatial` with the larger of that pad and its guide's halo; every other
//! layer is `Pointwise`. The plan's halo is `regional_guide::input_halo`:
//! the largest, over the rows, of a row's own halo PLUS the halos of every
//! guide above it (a row below a guide is evaluated over that guide's plane),
//! which is the largest row halo when no guide is staged; the plan's
//! `required_input_region` is the window grown by it, clamped to the canvas,
//! and contains every stage's plane. A raster whose style
//! renders planes (its `"style"` is `"effects"`) declares `Global {
//! analysis: "layer_style_planes" }`: plane rendering reads the WHOLE layer
//! before the loop (`Shape::of_layer` / `typed_shape` over every pixel, a
//! layer-aligned gradient's span from the shape's bounds, interior
//! `distance` offsets), so no halo contains it. Such a row contributes no
//! halo (its mask's feather is subsumed); the plan's `global_analysis` is then
//! the list of distinct analyses and its `required_input_region` the WHOLE
//! canvas. A non-empty MATERIALIZED group's row (13A-3b-ii) declares `Global
//! { analysis: "group_materialization" }` whatever its `"style"` —
//! `rendered_group` (typed: `render_layers`) reads the whole extent before
//! the loop even when the synthetic renders no planes (`"fill"`: a padded
//! plane past `MAX_PIXELS`); an empty one is `"fill"` with the ordinary
//! need. `global_analysis` lists the distinct analyses in row order. Without
//! one, `global_analysis` is `false`.
//!
//! CANCELLATION MODEL (13A-5a). Every stage `s` declares a CHECKPOINT
//! INTERVAL `W_s` in its own units, and the render one RETURN FLOOR `T_f` =
//! 1 ms (unwinding from a checkpoint to the FFI return: dropping the live
//! planes, the join, a poller's sleep). A cancel issued in stage `s` returns
//! within `T_f + W_s · c_s`, `c_s` the stage's per-unit cost calibrated on
//! its OWN uncancelled run on the measuring machine; a measurement reports
//! the latency, `c_s`, the bound, `max(0, latency − T_f) / c_s` ("units past
//! the floor") and PASS/MISS. Intervals: every sample loop and hold fill,
//! 4096 samples (`JobContext::every`, the hold `plane`); the byte guide build
//! (`adjust_guide::build`), 4096 cells, its grid blur's `feather` polling
//! per 4096 cells and per row; the byte mask feather blur
//! (`MaskProperties::derived_with_job`) at factor 1 (sigma ≤ 3), 4096 cells
//! per `feather` pass, and above factor 1 ONE Triangle resample of the
//! extended plane (`image::imageops::resize`, polled only at its boundaries
//! — replacing it would change the mask's bytes). Since 13A-5b-iii-b-2 the
//! byte PREPARATION reports: the unstaged accumulator fill advances and
//! reports per 4096-sample chunk (`pixels` in `work_units`); the byte plan
//! counts every top-level mask blur (`MaskProperties::work_units`, the typed
//! `PrecisionMask::plan` figure through the infallible
//! `plane_blur::float_work`), every styled raster's planes
//! (`style_scratch(..).1`) and every materialization (`|rect|·(1 + rows
//! inside)` + the subtree's mask/plane work + the group's own mask blur,
//! which runs inside `rendered_group` because `Node::resolve` dispatches a
//! materialized group before deriving any mask); each stage runs under
//! `run.range(done/total, (done + g)/total)` then advances by `g`: the mask
//! blur ramps start → end (`feather`'s rows; above factor 1 the reduced
//! plane's rows, the resamples flat), the style planes ramp through EQUAL
//! per-effect sub-ranges (13A-5b-iii-c-1: effect `i` of `n` under
//! `range(i/n, (i + 1)/n)`, `style_scratch` charging a uniform `8·p` per
//! effect; each effect's first blur ramps its sub-range and its later blurs
//! hold that sub-range's end), the materialization is flat at its start (the
//! chain reports nothing; children's blurs and the group's own land there);
//! driver reports during preparation carry stage 1, inner blurs 6, so
//! `stage == 6` is each stage's first inner report; a cancelled stage still
//! caches nothing. Since 13A-5b-iii-a the byte guide build reports INSIDE ITS
//! RANGE on the typed pattern (`job.range(done/total, (done + g)/total)`, `g`
//! its plan work): flat at the range's start through `adjust_guide::build`'s
//! grid pass and per-cell means (their polls report nothing); since
//! 13A-5b-iii-c-1 each grid blur ramps its OWN sub-range
//! (`GuideWork::blur_ranges`, the plan's terms: `p` pixels, `c` cells,
//! `taps`, `t = p + c·(2 + 4·taps)` — the first blur
//! `(p + c)/t → (p + c + 2·c·taps)/t`, the second on to `(t − c)/t`; its
//! `feather` rows, `plane_blur::blur` at the reduced sigma, above sigma 3 its
//! resamples and copies polling only and the reduced plane's rows ramping;
//! `taps` 0 leaves both empty), the combine is flat, then the driver's
//! advance lands on the range's end — the typed guide (and the full-frame and
//! destructive builds) have the same shape; its interval stays 4096 cells.
//! The byte stage-0 `prev` and D5 `snapshot` fills advance and report per
//! 4096-sample chunk (each `|R_0|` in the byte plan's `work_units`, the typed
//! `fill`'s twin; the typed snapshot is pushed inside stage 0 and uncounted —
//! a stated difference). A byte HOLD fill advances and reports progress per
//! 4096-sample chunk (its `|R_0|` is in the plan's work). The STYLE PLANE
//! RENDER (13A-5b-i: byte `LayerStyle::rendered_with_job` through
//! `Styled::of`, typed `render_effects_with_job` in `prepare_style` and
//! `composite_styled_into`) reports inside its range at 8 bits
//! (13A-5b-iii-b-2, above) and, since 13A-5b-iii-c-1, at typed depth in
//! `prepare_style` (`progress.job.range(completed/total, (completed +
//! work)/total)`, `work` its `style_scratch` charge, the same equal
//! per-effect sub-ranges, then the advance re-reports the end); since
//! 13A-5b-iii-c-2 a typed materialization's styled composite
//! (`render_styled` → `composite_styled_into`, no `checkpoints_only`) renders
//! its planes under the same range from `render_styled`'s own `completed`,
//! the Below contributions and the pixel pass after them report nothing and
//! its base advance re-reports the end (held under `fetch_max`) before its
//! style advance (the bake's `bake_style16` narrows `checkpoints_only` at
//! its call site: no change there) — and a cancelled byte render caches
//! nothing (the cache stores on success only). Its intervals: inside a blur,
//! the blur's own (above: 4096 cells per `feather` pass at factor 1, ONE
//! Triangle resample of the extended plane above it); inside a dilate/erode,
//! the distance transform's own polling and 4096 cells of the mapping; ONE
//! ROW of the plane inside each bespoke per-pixel loop (the bevel's
//! height-field adds, `shade` and coverage maps, the gradient overlay's loop,
//! the stroke's gradient fill, satin's combine)
//! and, since 13A-5b-ii-b, ONE ROW of the padded plane (`pw` cells) in
//! `Shape::of_layer`, bounded by `T_f + c_map` (a row is a fraction of the
//! one-pass `c_map`); every other primitive ONE unit polled at its
//! boundaries — `Shape::inverted`, the coverage `clone` / `to_vec`, the
//! all-zero scans, `shift_plane`, `multiply_planes`, `subtract_planes`,
//! `invert_plane`, `push_banded`'s multiply — each one memory-bound pass
//! over the plane. INSIDE BYTE `rendered_group` (13A-5b-ii-a: the chain
//! `rendered_group` → `composite_level_into` → `composite_unit_into` →
//! `composite_clip_group_into` / `composite_layer_into` →
//! `composite_styled_into` polls `CompositeEnv::job`, NONE for every
//! full-frame composite and live only from `resolve_materialized`, under
//! the materialization's range since 13A-5b-iii-b-2, with every inner call
//! site narrowed `checkpoints_only`, so the chain reports nothing and inner
//! blurs report at the range's start): ONE ROW of the layer rect
//! within the buffer in `composite_layer_into`, `composite_adjustment_into`,
//! the knockout unit pass's mix loop and the gated lerp; a nested guide
//! build, a mask feather — the group's own AND every child layer's enabled
//! mask, whichever branch composites it — and a style plane render each run
//! their own 13A-5a / 13A-5b-i interval (reached through `guide_with_job`,
//! `effective_mask_rect_with_job` / `active_mask_with_job`,
//! `rendered_with_job`, all narrowed `checkpoints_only` at the call site);
//! since 13A-5b-ii-b, ONE ROW of the buffer (or of the strip, plane, layer
//! rect or crop) inside `composite_pixels_into`, `composite_contribution`,
//! `clip_pass_shape`, `composite_buffer_into`, `BackdropSnapshot::of`,
//! the package strip's copy and mix, `encode_projection_with_job` /
//! `quantize_with_job`, `Shape::of_layer` and the mask crop
//! (`effective_mask_rect_with_job`; the aligned plane is a move —
//! `aligned_mask_with_job` returns its Borrowed case before `into_owned`);
//! still ONE unit: the buffer allocs, the `acc.to_vec()` / `buf.clone()`
//! snapshots, the alpha scans and resets, the coverage clone and
//! `drop(buf)` — each one memory-bound pass, bounded by `T_f + c_copy`,
//! `c_copy` the worker's own `acc.to_vec()`, the longest inter-checkpoint
//! gap of its own uncancelled chain (13A-5b-iii-a: `common::probe`'s
//! `Timeline`, read at the count the cancel was issued at). Bounds: rows
//! `T_f + c_row`, `c_row = t_mat / ((N−1)·H)` (`t_mat`
//! the uncancelled materialization time of N child composites less that of
//! one) or, inside the style/knockout primitives, `c_row = t_chain /
//! n_chain` (the chain's mean per-checkpoint cost over its own uncancelled
//! run); one-unit `T_f + c_copy`; a child mask blur `T_f + 4096·c`
//! (13A-5a's factor-1 form, `c = t_blur / (2·pw·ph)`). Since 13A-5b-iii-c-2
//! the typed styled path hands its live job to the shared primitives
//! (`composite_contribution`, `clip_pass_shape`, `BackdropSnapshot::of`,
//! `composite_buffer_into`: ONE ROW each), so a cancel inside a typed
//! materialization's styled composite returns within `T_f + c_row`, `c_row =
//! t_comp / n_comp` (the composite's mean per-checkpoint cost from its
//! planes' last report to its style advance, uncancelled). Since
//! 13A-5b-iii-a the byte stage-0 and D5 snapshot fills report, and since
//! 13A-5b-iii-b-2 the unstaged accumulator fill advances and reports per
//! 4096-sample chunk: every byte fill reports. Since 13A-5c-1 the byte
//! DESCRIPTION PREPARATION is a stage: the admission forecasts one parse's
//! scratch (`adjust_calibrated::parse_scratch` — the largest non-group
//! `meta` length × 256 under every profile (13A-5c-3), since the walk parses
//! each adjustment node's description whatever the profile; a term of the
//! plan's scratch), reserves it on the render's
//! job, then runs `validate_context_with_job` under that job (a check before
//! and after each layer's parse), the walk polls once per node, and
//! `composite_level_into`'s re-validation inside a materialization runs
//! under the chain's live job; its interval is ONE meta parse (serde cannot
//! be polled inside it), bounded by `T_f + c_parse`, `c_parse` the worker's
//! own uncancelled checkpoint-timeline gap at the count the cancel was
//! issued at. The typed render parses no description (bound at edit time).
//! Since 13A-5c-2b the byte walk, its budget dry run and the tree take a
//! `Cut` (gating contribution exactly as the reference's `env.cut` does,
//! never `visible`): `Cut::NONE` for every regional render, the retouch
//! sampling plate's cut for `doc_regional_plate::regional_cut_plate` only.
//!
//! The Phase 4 image-level kernel contract's region/preview rules — the 4096-pixel cadence, nearest
//! top-left divisor sampling, named refusals, spatial/global operations declaring their
//! neighbourhood per layer (`EvaluationNeed`), never a silent full-frame fallback — live on in the
//! document regional renderer.
//!
//! Every level is grouped into CLIP UNITS — an unclipped entry plus its positional clipped run, or
//! a lone baseless clipped entry — and the admitted set adds Blend If (typed on the layer; at 8
//! bits inside the style) and, since 13A-3b-i, every layer style on a RASTER at 8 and 16 bits
//! (enabled effects — the planes `LayerStyle::rendered_with_job` / `typed_shape` +
//! `render_effects_with_job` build before the loop, the reference's own preparation, polled since
//! 13A-5b-i (a cancelled byte render caches nothing) and reporting inside their range at 8 bits
//! since 13A-5b-iii-b-2, at u16 in `prepare_style` since 13A-5b-iii-c-1 and in `render_styled`'s
//! composite (full frame and typed materialization; the shared primitives polling its live job per
//! row) since 13A-5b-iii-c-2 — fill opacity, Blend If, the package model). The plan types and the
//! staged-level planner (`plan_levels`, `input_halo`, moved in 13A-4b-ii-b-2) live in
//! `regional_plan.rs` (re-exported by `regional` / `regional_guide`), the cursor blocks in
//! `doc_regional_sample.rs` / `precision_regional_sample.rs` (13A-4b-ii-b-1). The admission walks
//! live in `doc_regional_walk.rs` / `precision_regional_walk.rs` (split out in 13A-4b-ii-a).
//! Everything else refuses BY NAME ("regional render refuses layer N (name): …") in a FIXED order
//! over each level's units — the unit skip (a skipped base's subtree is never checked), then entry
//! by entry a group's own level (its row after its children's) or the guided operator as a clip
//! member of a packaged 8-bit styled base (the knockout and grouped-interior refusals are gone
//! since 13A-3c, "is a group with layer-style effects" since 13A-3b-ii); the 13A-1 structure
//! refusals are gone — and plan and render refuse identically (the plan raises `validate_context`,
//! the 11E/float `transfer()` walk and the walk; the render reuses the walk's tree). A byte group
//! isolates by `doc_group::is_isolated` exactly as `composite_unit_into` decides (a member always),
//! and the byte sampler branches on the base's CONTRIBUTION first — a pass-through base composites
//! straight or gated whatever its positional `clip_run` — then on `clip_run` (hidden members
//! included) for a raster/adjustment/isolated base; the typed unit test is the scene builder's
//! `clipped_members.is_empty()`. The window is walked SAMPLE-MAJOR, so a cancel returns within 4096
//! samples of the running stage loop (nested and pass-through paths and hold fills included; a byte
//! hold fill also reports progress since 13A-5a), within one checkpoint of the running byte mask or
//! guide blur (13A-5a: `MaskProperties::derived_with_job` polls, and a cancelled mask blur caches
//! nothing; since 13A-5b-iii-b-2 the byte preparation REPORTS — the unstaged accumulator fill
//! advances and reports per 4096-sample chunk (`pixels` in `work_units`); the byte plan counts
//! every top-level mask blur (`MaskProperties::work_units`, the typed `PrecisionMask::plan` figure
//! through the infallible `plane_blur::float_work`), styled raster's planes (`style_scratch(..).1`)
//! and materialization (`|rect|·(1 + rows inside)` + the subtree's mask/plane work + the group's
//! own mask blur, which runs inside `rendered_group` because `Node::resolve` dispatches a
//! materialized group before deriving any mask); each stage runs under `run.range(done/total,
//! (done + g)/total)` then advances by `g` (`doc_regional_progress::Run`): the mask blur ramps start →
//! end, the style planes ramp through EQUAL per-effect sub-ranges (13A-5b-iii-c-1: effect `i` of
//! `n` under `range(i/n, (i + 1)/n)`, each effect's first blur ramping its sub-range and its later
//! blurs holding its end), the materialization is flat at its start; driver reports during
//! preparation carry stage 1, inner blurs 6, so `stage == 6` is each stage's first inner report; a
//! cancelled stage still caches nothing; while since 13A-5b-iii-a the u8 `GuideSample::blur`
//! reports INSIDE its guide's range — `job.range(done/total, (done + g)/total)`, flat through the
//! grid pass and, since 13A-5b-iii-c-1, each grid blur ramping its OWN sub-range
//! (`GuideWork::blur_ranges`: `(p + c)/t → (p + c + 2·c·taps)/t` then on to `(t − c)/t`, the typed
//! guide the same), the combine flat, then the driver's advance — and a staged level's stage-0
//! `prev` and D5 `snapshot` fills advance and report per 4096-sample chunk, each `|R_0|` in the
//! byte `work_units`; a materialized group's child masks too, since 13A-5b-ii-a, through
//! `Layer::active_mask_with_job`), within one checkpoint of the running style plane render at
//! either depth (13A-5b-i: blurs, dilates/erodes, per row inside each bespoke per-pixel loop, every
//! other primitive at its boundaries), or within one checkpoint of the running byte
//! `rendered_group` (13A-5b-ii-a/b: the chain and its style/knockout primitives poll per row; only
//! allocs, copies, clones and alpha scans stay one unit), or within one meta parse of the byte
//! DESCRIPTION PREPARATION (13A-5c-1: `validate_context_with_job` under the render's job, one
//! parse's scratch — `adjust_calibrated::parse_scratch` under every profile since 13A-5c-3, because
//! the walk parses each adjustment node's description whatever the profile — planned and reserved
//! first, the walk polling per node; since 13A-5c-2b the byte walk, its budget dry run and D5's
//! Background test take a `Cut` exactly as the reference's `env.cut` gates contribution —
//! `Cut::NONE` for every regional render, the sampling plate's cut only in `doc_regional_plate`) —
//! each stage's bound is `regional.rs`'s CANCELLATION MODEL (`T_f + W_s · c_s`).
//! `doc_regional_tree.rs` / `doc_regional_sample.rs` (the byte per-sample level and unit bodies) /
//! `doc_regional_style.rs` (the byte styled per-sample bodies, `Styled::of`,
//! `resolve_materialized`) / `doc_regional_budget.rs` (the budget dry run
//! `materialization_charges`, moved in 13A-4b-ii-b-2) / `regional_knockout.rs` /
//! `precision_regional_tree.rs` / `precision_regional_style.rs` (the typed styled per-sample
//! bodies) hold the trees and per-sample evaluation and NO admission logic; no pixel math lives in
//! any of them: the per-pixel bodies are `doc::composite_layer_sample` / `doc::quantize_sample` /
//! `doc_evaluation::encode_sample` / `style_composite::composite_pixels_sample` /
//! `style_composite::composite_contribution_sample` / `style_composite::mix_premultiplied` /
//! `Shape::coverage_byte` / `precision_style_render::composite_pixels_sample16` /
//! `typed_coverage_byte` / `doc::adjust_backdrop_sample` / `blend::composite_source_into` /
//! `blend::gate_source_into` / `precision_render::composite_raster_sample16` /
//! `adjust_backdrop_sample16` / `composite_group_sample16` / `gate_pass_through_sample16` /
//! `checked_alpha` / `style_blend_if::source_weight_16` / `style_knockout::backdrop_at`, which the
//! full frame also calls; a byte mask sample is `Layer::effective_coverage_at` over
//! `MaskProperties::derived_with_job`, a typed one `PreparedMask::at` over
//! `precision_render::prepare_mask` (every contributing mask prepared once before the loop and
//! charged in the plan's scratch). Only a materialized group allocates a per-level buffer; its
//! BUDGET COUNTERPART is the 8-bit plan's dry run (`materialization_charges`, after the walk, over
//! every materialized group the reference renders — never a member under an empty base group) of
//! the reference's own charge and nesting points inside it, against the same group budget the
//! render then charges (`render_region_with_group_budget` / `render_region_plan_with_group_budget`,
//! the `flattened_with_group_budget` twins), so plan and render refuse with the reference's
//! sentences and never refuse a render the reference completes, and (13A-5c-3) whose third figure —
//! the reference's UNCHARGED per-unit buffers inside the nest: unit buffers, package strips and
//! their knockout windows, the run base's guide, a pass-through's mask crop, each nested synthetic
//! — joins the plan's scratch; a reference budget refusal caused by an unmaterialized level — or by
//! the D5 Background / unmaterialized pass-through Shallow snapshot the reference holds and the
//! regional path never allocates — does not refuse regionally (a stated difference; the typed
//! plan's materialized-subtree mirror adds `collect`'s `knockout_scratch` term). A styled sample
//! runs `composite_styled_into`'s order at one sample (Below, pixels — a clip base into a private
//! sample clamped to the planes' or pad-0 shape after every step — Interior; the package inside
//! `affected_window` only, mixed back always). The plan declares one `EvaluationNeed` per
//! contributing layer (`Pointwise`; `Spatial` with a feathered enabled mask's `padded_dimensions`
//! pad; `Global { analysis: "layer_style_planes" }` for a raster whose style renders planes;
//! `Global { analysis: "group_materialization" }` for a non-empty materialized group, whatever its
//! `"style"` — the plan's `global_analysis` is then the analysis list in row order and
//! `required_input_region` the whole canvas; the typed plan charges a materialized group's buffer
//! plus a mirror of `PrecisionScene::plan`'s `collect` over its subtree; no document-compositor
//! operator is global — 13A-4a's recorded decision) plus `clip_base`/`group`/`mask`/`blend_if`/`style` and the plan's `group_depth`. The
//! sub-slice plan and open items are `designs/phase-13a-regional-status.md`.

pub(crate) use crate::regional_plan::{
    EvaluationNeed, LayerNeed, MATERIALIZATION_ANALYSIS, STYLE_PLANES_ANALYSIS,
};
pub use crate::regional_plan::{RegionRequest, RegionalPlan};

/// The construct a guided Shadows/Highlights clip MEMBER of a PACKAGED byte
/// base refuses with (13A-4b-ii-b-2; the package runs members per strip,
/// owner 13B).
pub(crate) const PACKAGED_BASE: &str =
    "is Shadows/Highlights with a guide under a packaged styled base (8-bit effects below opacity 1)";

/// The construct a STYLED adjustment layer refuses with: its
/// effects take the mask over the canvas as their shape and the adjustment
/// applies after its Below effects (`style_adjustment`), which the
/// per-sample bodies do not evaluate. Inside
/// a materialized subtree the reference renders it and nothing refuses.
pub(crate) const STYLED_ADJUSTMENT: &str = "is an adjustment layer with a layer style or Blend If";

/// The ONE refusal sentence: "regional render refuses layer {index} ({name}):
/// {construct}".
pub(crate) fn refusal(index: usize, name: &str, construct: &str) -> String {
    format!("regional render refuses layer {index} ({name}): {construct}")
}
