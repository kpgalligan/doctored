# Jobs and saved-revision AppKit probe

Run `make rust project`, then `python3 scripts/jobs-probe/verify.py --keep` on macOS.
The runner compiles current app sources with the release Rust library, Swift
`-Onone` and whole-module compilation. Real NSDocument saves need access to macOS
file-coordination services, so run outside the filesystem sandbox. The run is
bounded at 1337 s (`RUN_TIMEOUT_SECONDS`; the full list measured ~234 s, and the
opt-in slider run described below 891 s, measured with its since-removed 16-bit
4000 × 3000 row), and
`run.log` is printed with undecodable bytes replaced, on success and failure alike.

Two environment variables, inherited through the runner's environment, narrow a
run:
`DR_JOBS_PROBE_CASES` is a comma-separated list of exact case names (unset or
empty runs every case; an unknown name fails before any case runs), and
`DR_JOBS_PROBE_REPEAT` a positive integer, the number of times the selected list
runs (default 1). For example
`DR_JOBS_PROBE_CASES="agent serve-thread wait,agent release mid-call" DR_JOBS_PROBE_REPEAT=10 python3 scripts/jobs-probe/verify.py`.
The loop sets `DR_EDR_HEADROOM` before each case, replacing any inherited value: 4 for the five
`hdr …` cases, 1 for every other, so HDR Preview engages only where a case asserts it.

After every case the loop checks the preview registry (`registry_drain.swift`): it reads
`SelectionWorkBudget.scratchBytes` when the case begins, pumps the main run loop
after it ends until the registry is back at that value, and prints
`INFO: preview registry after [<case>]: <now> B (<entry> B at entry), drained in <t> s`.
The ceiling is 120 s and only catches reservations that are never released: how fast
a registry drains depends on the machine's load, so the time is reported, never
asserted (`core/CLAUDE.md`'s load-robustness rule). A case that
leaves preview reservations registered fails with `FAIL [<case>]: left <n> B of
preview reservations registered in SelectionWorkBudget`. The check is
relative to the case's entry, so only the retaining case fails and later cases do
not cascade. It stays as the regression guard; a passing run reads `0 B (0 B at
entry)` for every case. Every case runs inside its own autoreleasepool, because
top-level `main.swift` has none.

"transforms" checks that the Free Transform commit's modal wait services the main
run loop with a gated heartbeat: a 2 ms timer in the common and modal-panel
modes counts a tick only while `BackgroundWork.isWaiting`, and its first counted
tick opens a `ProbeGate`. Before the commit the probe queues a block on the serial
`BackgroundWork.queue` that spins on that gate (capped at 10 s), so the commit's
worker cannot finish before main has serviced a tick inside the wait, however fast
the transform runs. Before this a transform that finished inside the wait's first
5 ms poll sleep could score 0 ticks. A wait that never services the run loop
still fails, after the cap, and the failure says the heartbeat gate timed out.

Checks cover actual fill-sheet Prepare/Cancel, heartbeat responsiveness, stale
revision refusal, spot-heal gesture callbacks, one-step undo/redo, saved-revision
and window dirty state, failed atomic writes, real Free Transform and MCP
perspective/group paths, Actions replay, rasterization reports, pending-job
Stop Recording responses/saved Actions, Esc cancelling a cancellable modal
wait (and no other key doing so), the Convert sheet's baking preflight
running on the worker with the sheet up (inline when the worker is occupied;
Esc cancels it with the sheet up and Apply disabled) and the same at 16
bits ("convert preflight 16", where Apply then lands with three "(baked)"
layers under the Convert's own plan), and the
transform panel's Cancel on the UI Free Transform commit, MCP
`transform_layer` and the 16-bit ⌘T and Crop gestures: nothing is edited,
dirtied, recorded or alerted, and each session stays open and re-arms.
"agent worker wait" checks that an MCP call waits, bounded, for a
transient worker instead of refusing (an observer, or an edit during a job,
answers well inside the wait limit), and "commit during preview" that
a Free Transform or 16-bit ⌘T commit releases its own in-flight preview
reservation before admission. "agent serve-thread wait" checks that an
MCP call waits for that worker on the serve thread while main keeps
returning to its event loop: the probe's own main-thread pump releases the
worker while the call is still pending, and sees a still-held call pending
mid-wait, so a main thread parked in the call fails deterministically (the
longest pump gap is reported, not asserted), and "agent release mid-call" that
releasing the agent socket while a call runs returns at once (under 3 s, behind a 10 s
watchdog) instead of joining a serve thread blocked on main: the socket file is gone, the call
in flight still answers and its save finishes, and a rebind at a fresh path answers
`list_documents`. Every agent case binds the app's own listener on a scratch socket
(`withLiveAgentServer`: `DR_AGENT_SOCKET` pointed at a fresh `0700` folder under
`NSTemporaryDirectory()`, then `AgentServer.bindSocket()`), never the person's, and posts with
`/usr/bin/curl --unix-socket` in a `Process` whose termination handler finishes the reply, so
main never blocks on a call. Every main-thread pump names its step in a timeout, (c)
waits for the worker to settle before it starts, saves to a per-call destination, and a reply
that arrives before the release could run fails at once naming the reply: (c)'s only release
trigger is a 1 ms timer that sees the save's modal worker wait. The 30 s bound stays, since
every step's own bound is far below it. "mcp settings pane" drives Settings ▸ MCP's
Copy MCP Command on a private `NSPasteboard(name:)` and a scratch helper file: an executable
helper enables it and the click puts `AgentServer.mcpCommand` on that pasteboard with the
copied status; a helper that is not executable disables it with the missing status; the
helper's path is selectable monospace text. "heal gesture", "patch commit", "heal leak", "gesture guard" and
"lazy panel" check that the 8-bit Healing Brush (layer and new
layer) and Patch commit as nonmodal jobs equal to the direct `healJob` /
`healToNewLayerJob` heal / MCP `patch_region` replay with one undo, a display pin (none over a channel display),
cancel and stale endings; that a stroke whose target moved mid-gesture leaks no
live edit; that the gesture guard refuses and settles as designed and a refused
Patch commit keeps its region; and that a job's panel appears only after 0.25 s.
"agent heal jobs" checks that MCP `heal_stroke` (8-bit) and
`patch_region` wait by default and with `background: true` return a running job
that `cancel_job` stops, with equal pixels, one undo, one recorded step naming
the resolved layer, a no-op answering `changed: false`, a persisted-warp
refusal in-band (waiting and background, with no alert, while a
UI-started job's refusal still presents one), and a 16-bit background
`heal_stroke` completing with one recorded step naming the layer, undoing to the
base, and ending `cancelled` under `cancel_job`.
"precision stroke jobs" (`precision_stroke_jobs.swift`) drives a 16-bit
Brush stroke through the canvas's own `beginStroke` / `continueStroke` /
`endStroke`, with the serial worker queue held until the first checks are done:
the stroke must leave one running "Brush Stroke" job with nothing published, the
ghost pinned (`strokeGhostPin`), no panel yet, its worker-held footprint and
samples as its `scratch`, and a second stroke refused; then
completed pixels, the pin cleared and exactly one undo and redo; a stroke
cancelled before the first poll ends `cancelled` with no undo step, and one
followed by a selection change ends `stale` with its "document changed" alert
(not a rename: at 16 bits `applyEdit` prepares its display on the worker, so a
rename is refused while the job holds it), each with the pin cleared; a Spot Healing stroke reaches `completed` the same way.
"stroke stops at cap" (`stroke_caps.swift`) stands for a Brush drag that
reaches a stroke cap: a canvas session (`NativeBrushStrokeSession` with
`.stopInk`) stops laying ink there and the stroke commits what was laid. Session
only, on a 12500 × 8000 extent: 5,000 px dabs at the centre until the session is
full lay 71 soft and 64 hard dabs (the charge `(⌈d⌉ + 2)²`, plus `64·⌈4πaρ +
πρ²⌉` for a hard tip, against the 2 billion work cap less `2·canvas`), one more
stamp adds nothing, and the laid dabs resolve and plan in the core; on 64 × 64
the 50,001st 23 px dab is dropped while the default session still refuses by
name. Canvas, at 16 and at 8 bits: `beginStroke`, the session swapped for one
filled to 50,000 dabs, a `continueStroke` that adds nothing and refuses nothing,
then `endStroke` — one history entry, changed pixels, no alert, and one undo
back to the base. A SMUDGE session (`smudging: true`) also stops ink at
Smudge's own 256 million bound (`NativeBrushStroke.maximumSmudgeWork`, the
core's `MAX_SMUDGE_WORK`): soft 200 px dabs 50 px apart charge each move
`50 · ((200 + 2)² + 1)`, so 125 moves are laid and the next stamp is dropped
while the same dabs leave a Brush session far from full; the laid dabs plan in
the core (`dr_precision_doc_smudge_plan`), and a stroke there and back over the
same 6,250 px is refused by the core with "smudge stroke exceeds 256 million work
units; use a shorter stroke or a smaller brush".
"preview fallback" (`preview_fallback.swift`) pins that an interactive
preview is never refused for work, at the 2 GiB test pin (the case fails at any
other limit). (a) A 32-bit float document (the committed 11F fixture resized to
3900 × 2600 by `image_size`) under ⌘T at 17° Lanczos3: the session's own
request is refused as a preview (`interactive`, "typed geometry exceeds the
bounded pixel-work limit") and plans as a commit at more than 2 billion work
units; a float document has no reduced proxy, so the gesture runs the commit
request as its cancellable preview job and the status must read "Preview
current" at full size. (b) The same session at 200 %: the commit plan is
refused for memory, and the status must start "Preview unavailable:" and must
not contain "pixel-work limit" (the fallback's own error is the one reported).
(c) A 16-bit ⌘T of the selected pixels, a 3400 px square on 4800 × 3400 turned
30° at Lanczos3 (a landing over the 2 billion units the core once refused
at): the preview lands "Preview current". It runs third, before the cases
that leave documents and caches behind.
"precision patch commit" (`heal_jobs.swift`) drags a 16-bit Destination
Patch 150 px: the region's placement now runs inside the Patch job, so the job
must start running with its worker-held coverage (the placed copy; the region is
already in the retained payload, and the core reserves its own plan scratch) as its
`scratch`, nothing published and no panel, then
complete with changed pixels and one undo, and a second drag cancelled before
the first poll must end `cancelled` with the base kept.
"byte render-neutral carry" checks that an 8-bit rename, its Undo and
Redo, and MCP `set_layer_properties` with only `name` keep the projection and
preview objects and that the carried projection's pixels equal a fresh render,
while an opacity change (alone or with a name) renders anew; a pixel edit
falsely declared render-neutral is the negative control the pixel comparison
must catch.
"closed view panels" checks that while view 2 is open a rename reaches
its Layers panel, marks its hidden Channels panel for a reload and starts a new
Info histogram (the positive control), and that after `close_second_view` the
closed view's Layers, Channels and Info panels ignore the document's edits while
view 1's Layers panel follows them.
"byte proof retained" checks that the 8-bit proof of the retained
projection equals the proof of a fresh render (pixels, space and out-of-gamut
count), that a pixel edit moves the retained projection, and, after a pixel
edit falsely declared render-neutral makes the retained projection differ from
a fresh render, that the canvas proof equals the proof of the retained
projection and not of the fresh render.
"group expansion keeps proof" checks that opening or closing a group changes its
disclosure and keeps the retained projection at 8 and 16 bits, so a proof stays on
and is not re-rendered.
"layer thumbnail costs" (`thumbnail_costs.swift`) resizes an 8-bit
`fixture(600, 400)` and a 16-bit `precisionFixture(600, 400)` to 6000 × 4000
over MCP `image_size` and times layer 0's 34-pixel thumbnail: `before_ms` is the
median of three runs of the path it replaced (8 bits `layerImage(0)` +
`resized(BILINEAR)`, 16 bits `PrecisionWorkContext(image:)` + `previewRGBA8` +
`RasterDocument.fitted`), `miss_ms` the median of three fresh
`LayerThumbnailCache` misses and `hit_ms` the following hit. It requires equal
dimensions, at most 2 codes on alpha and every premultiplied channel (reported,
never widened), `miss_ms < before_ms` and the second lookup a hit. Mask
thumbnails (`add_layer_mask reveal_all`) miss then hit, a rename hits, and
`remove_layer_mask` + `add_layer_mask hide_all` misses once — on the 8- and
16-bit 6000 × 4000 documents and a 16-bit 600 × 400 one. The 16-bit
mask miss is fitted in the core, and on the 600 × 400 document it is measured
against the path it replaced (`nativePlaneDisplay(target: "mask", canvas:
false)` + `RasterDocument.fitted`, median of three) under the same three
requirements. The 8-bit mask miss is fitted in the core too
(`fittedLayerMaskImage`), and on the 6000 × 4000 document it is measured
against the Swift path it replaced (`layerMaskImage(0)` + the former fit: scale
`min(28 / longest, 1)`, rounded, at least 1, `resized(BILINEAR)` when it
shrinks) under the same requirements. The 16-bit Channels panel's
mask and alpha rows fit in the core: after the 600 × 400 mask check the probe
selects a 200-pixel square, saves it over MCP `save_selection`, and times
`planeImage(for: .layerMask / .alpha(0), composite: nil, activeLayer: 0,
maxSide: 34)` (median of three) against the whole-plane Swift path it replaced
(`nativePlaneDisplay(target:index:maxSide: 34)`, median of three), under the
same three requirements. Float thumbnails (`floatFixture()`) miss then hit, a
rename hits and `set_tone_map reinhard` misses; a 2400 × 1600 float
layer (an uncompressed float TIFF the probe writes: r = 2x/w, g = y/h, a hard
blue edge at 37%) is compared with `displayRGBA8` + `RasterDocument.fitted`
for drift (≤ 2 codes); its timing is reported, not required, since both paths
tone-map every sample through the same encoder and differ only by the
whole-layer copy the miss saves.
One `INFO:` line per measurement.
"composite readers" checks that Apply Image / Calculations from Merged, their
sheet previews, the Evaluation readout and Remove Red Eye read the retained 8-bit
projection only for its own handle, and that a render failure answers the core's
sentence (one alert in the UI, `isError` over MCP, nothing edited) while a stale
stored error does not refuse a render that now succeeds.
"job escape", "job undo keys", "agent undo during job" and "refused job no step"
check that Esc on the canvas and ⌘Z/⇧⌘Z cancel the running
UI-started job (a held job and a real Healing Brush stroke) instead of
deselecting or moving history, while a crop session keeps Esc and an MCP job
leaves both keys as before; that MCP `undo`/`redo` refuse by `job_id` while a
job runs; and that a job result `applyEdit`'s depth guard refuses ends `refused`
with no empty undo step.
"history during job", "window escape", "no empty undo step" and "truthful agent
undo" check that choosing another History state cancels a
running UI job (the current row does nothing) and navigates over an MCP job,
that MCP `restore_history` refuses by `job_id` while any job runs, that Esc
reaching `EditorWindow` cancels the UI job while a text view keeps its Esc and an
MCP job is left alone, that a depth refusal in `performGroupedEdit` and
`applyPrepared` leaves no empty undo step or alert, and that MCP `undo` answers
an error, not ok, for an empty undo step. "window escape" (b) drives Esc from a
throwaway table, because the window itself as first responder now hands keys to
the canvas.
"session commit no empty step", "window keys to canvas", "floating window
escape", "silent refusal beeps", "group open no undo" and "guides refused
mid-gesture" (`key_polish.swift`) check that
`withUndoGroupDiscardingEmpty` adds no step for an empty body, keeps Redo and
leaves a nested group to its outer one, and that an untouched Free Transform
committed by MCP `save_copy` leaves no undo step; that with the editor window
itself first responder a tool key and Esc reach the canvas while an open text
session keeps them off it; that Esc from History, Snapshots, Layer Comps,
Actions and the editor window — the window itself, or a table or the Actions
controller through the real responder chain — cancels the running UI job and
swallows a second press, while a text view keeps its Esc and an MCP job is left
alone; that a `SilentEditRefusal` presented through its document beeps with no
alert while any other error alerts once; that MCP `set_layer_properties open`
adds no history entry, dirties the document and keeps the retained projection
at 8 and 16 bits, answers `changed:false` when repeated, and with `name` is one
undo step that undoes both; and that an agent guide edit under a live gesture
answers an error with no undo step while one with no gesture is one step.
"panel table keys", "text session window keys", "floating panel undo" and
"progress panel escape" (`key_polish.swift`) run after them. The Layers
and Channels tables are `CanvasKeyTableView`s: a bare V sent to the table
selects Move and puts the canvas in focus, and, with the table itself first
responder (the app's tables refuse it, so the probe lifts
`refusesFirstResponder` for the check and restores it), V reaches the canvas,
Esc deselects or cancels the running UI job and keeps the selection, and Down,
Space and ⌘V stay in the table. With a text session open, a key that reaches
the editor window or the Layers table goes to the session's text view, which
takes focus again and receives the character, and no tool changes. `undo:` and
`redo:` sent up the responder chain from the History, Snapshots and Layer Comps
tables undo and redo the document, validate as the editor's do, read "Cancel
Held Probe" and cancel a running UI job, and leave the document alone while a
text view in the panel has focus. A nonmodal job's progress panel has Esc as
its Cancel button's key equivalent and `performKeyEquivalent` with Esc cancels
the job; a modal wait with no Cancel has no key equivalent, and the next
cancellable wait on the pooled panel has Esc again.
"session refusal sentences" (`session_refusals.swift`) checks that
a refused canvas-session commit names its reason to the agent inside
`SessionCommitRefusal.capturing`: `beep(_:)` notes its sentence; `applyEdit`'s
nil transform, a document changed under the transform and the live-edit entry
guard each give their own sentence and leave no undo step; at 8 bits
`admitsPublication` of a 16-bit handle gives the depth guard's sentence, and a
presented `SilentEditRefusal` gives its message with no alert; a rasterize
prompt answered Cancel names the layer; and a Free Transform whose job is
cancelled from the panel while MCP `save_copy` commits it answers an in-band
error carrying "could not be committed" and `JobCancelled.message`, keeps the
session open, adds no undo step and writes no file.
"more composite readers" (`more_composite_readers.swift`) checks that
the Info readout (`PixelReadout.at`), `sample_pixel`, Copy Merged
(`mergedCopySource()` and `copy_merged`), the eyedropper
(`eyedropperComposite()` and `sample_color`), Print and `SelectionCapture`
(`select_color_range`) read the retained projection; that under a render
budget each answers the core's sentence — `isError` over MCP with no alert, one
alert for UI Copy Merged with the pasteboard untouched, "—" and a footnote
naming the failure in the Info panel, a silent eyedropper — and that switching
the Info source to a layer clears that footnote; that with the budget lifted
but the stored error still set each reader renders; that the typed Evaluation
readout reads the projection it is handed; and that Apply Image from Merged
onto a text layer under the budget answers the sentence before any rasterize
prompt, keeping the description.
"precision readers" (`precision_readers.swift`) checks
at 16 bits that `PrecisionReadout.at` reads the retained projection and is nil
off the canvas (a reach-1 block touching the corner still reads); that with the
worker held, so the projection cannot refresh, `PrecisionReadout.at` throws the
stored sentence while off-canvas stays nil, the Info readout shows "—" with the
sentence after the fixed 16-bit footnote, the eyedropper samples nothing and
presents nothing, the Info histogram reads "Histogram unavailable" and names
its failure in the footnote, the scoped cursor (a layer source) names its
failure, and an off-canvas scoped cursor freezes; that once the worker is
released the next readout, scan and eyedropper press clear each failure; that
`HistogramLoader` delivers a thrown read to `onFailure` (or `onBins(nil)` with
none set), a `JobCancelled` and a mid-run cancel under a cancelled control as
nil, a throwing `requestReport` to `onFailure`, and nothing at all for requests
it has superseded; and that the typed Evaluation sheet holds the retained typed
projection and measures its readout against it.
"info analysis" (`info_analysis.swift`) checks at 16 bits
that a composite `ScopedAnalysisRead` handed the retained projection keeps it,
plans less scratch than the render and answers exactly what the render answers —
histograms at stride 1 and 3 and a reach-1 sample, with and without a 20×20
coverage — while a layer scope drops it; (a8) the same at 8 bits for a
`fixture(64, 48)` read handed `byteProjection: retainedByteProjection` (seven
answers and one shared refusal, a layer scope dropping it); and that the panel's histogram still
reads "Clipped" under the 16-bit footnote; and at 32-bit float (the
`float_11f/scene.tiff` fixture) that the composite histogram reads "Below
zero:" with non-zero below-zero and above-white markers under the float
footnote and the readout reads; (d) Active layer's histogram arrives
without a failure and its scoped cursor reads; (e) Below active layer with the
active layer at 0 reads "No pixels to count" without a failure; (f) an 8×8
weighted selection requires the scoped cursor, its centre reads and (1, 1)
names "no positively weighted pixels"; and (g) a composite `ScopedAnalysisRead`
handed the retained float projection answers exactly what the render answers —
a histogram and a reach-1 sample, with and without a 20×20 coverage. (s) a New Adjustment Layer sheet over the whole stack
plans `.composite` (a destructive sheet still plans `.layer(0)`), and its
`Histogram.loadForSheet` bins are those of the retained projection — at 8 bits
the stale carried projection a pixel edit falsely declared render-neutral leaves
behind, not a fresh render; at 16 bits those of `retainedPrecisionProjection`.
"alt nudge duplicate", "alt drag duplicates layers", "free transform copy" and
"selected pixels polish" (`move_duplicate.swift`) check the Move
tool's ⌥ duplicate family and the selected-pixels polish: a synthesized ⌥→
through `canvas.keyDown(with:)` over a selection keeps the original, lands the
copy, moves the selection and is one "Duplicate" step (at 16 bits equal to MCP
`move_selected_pixels duplicate`), while a plain → leaves the hole; with no
selection ⌥→ duplicates the layer and moves the copy, one "Duplicate Layer"
step with the copy selected, at 8 and 16 bits; an ⌥-drag with no selection
commits the copy at exactly the dragged offset at each depth, selects it, is
one "Duplicate Layer" step undone to one layer, a click changes nothing, a drag
out and back leaves one in-place copy, a cancel clears the session, and MCP
`move_layers duplicate` answers `copies` with the same state while a zero delta
still duplicates; ⌥⌘T validates only with a selection, keeps the original, is
one "Transform Copy" step and equals MCP `transform_selected_pixels duplicate`
for a translation and a 30° rotation at 8 and 16 bits; the Move tool shows the
copy cursor with ⌥ held and during an ⌥ drag; a selected-pixels drag re-snaps
to the float's opaque bounds once the float query answers; and a
`BackgroundWork.wait` with `revealAfter` keeps its panel hidden until the delay,
shows it at once with 0, and never shows it for a wait shorter than the delay.

"move float" (`move_float.swift`) checks the persistent Move float on a
64×32 fixture with F over a path through W: two ⌃ drags of +20 restore W
exactly with F at 44 and the hole at 4, the marquee offset and the 8-bit
preview (W shown covered, then restored) mid-drag, Esc leaves the handle and
the float, → continues it, one "Move" undo per step restores each handle and
undo past the lift then redo continues the float; Deselect with a new
selection, a layer switch, Save As and a blur each commit it (the next drag
empties W); an ⌥-drag copy floats and ⌥ over a live float stamps; and 16-bit,
32-bit float and CMYK fixtures restore W exactly and equal MCP
`move_selected_pixels` dx 20 twice.

"floating commit refusal" and "discard empty group cost" (`commit_hygiene.swift`)
check commit and undo hygiene. The selected-pixels Free Transform
publishes one "Transform Selected Pixels" step from the UI and from an MCP
writer; `ImageDocument.publishPrepared` called with a live edit open answers
`.refused` with `applyEdit`'s sentence, clears the prepared edit and leaves no
step (the refusal is a backstop no commit reaches end to end, so the case
drives the helper directly), at 8 and 16 bits; and a 16-bit commit refused with
a live edit open keeps its session and commits once the edit is gone. The cost
case builds 50, 500 and 2000 history entries, times 21 empty
`withUndoGroupDiscardingEmpty` bodies with no redo tail and with half the
history as redo, and prints one `INFO` line per size (information, not a gate);
it asserts that the empty bodies keep the history, Undo and the redo tail, that
a body of two renames or of a refused edit and a rename is one entry, and that
a deferred group closed mid-body is reopened at the next registration.

"history bookkeeping cost" (`commit_hygiene.swift`) and "undo while
recording" (`history_safety.swift`) check history bookkeeping. The cost
case builds 50, 500 and 2000 renames under a bookkeeping cap of the same size
and prints one `INFO` line per size (the mean of the last 10 edits, the median
of 21 direct `enforceBudget` calls, the mean of 10 undos and 10 redos and of 10
renames at the cap, each evicting one entry; information, not a gate). One
more field splits an eviction's cost, timed after a capped rename and
outside its interval: `cap_drop_ms` is `dropHistoryUndo` for a target that
owns no registration — the same whole-stack `removeAllActions(withTarget:)`
walk, removing nothing. The history evicts by advancing a head
offset instead of `removeFirst()` (the probe's old `cap_shift_ms` timed a
shift that no longer exists). Medians of three runs, 50 → 2000 entries:
`cap_edit_ms` 0.163 → 0.392 before and 0.151 → 0.307 after, `cap_drop_ms`
0.003 → 0.113 before and 0.003 → 0.115 after, so the growth the walk does
not explain fell from 0.119 ms to 0.044 ms (per run 0.025 / 0.198 / 0.137
before, 0.027 / 0.049 / 0.043 after: every after run is under 0.05 ms, one
before run was too, so runs vary). The NSUndoManager walk is the
residual AppKit leaves (`levelsOfUndo` stays 0). It
asserts that each undo and redo moves the cursor by one, each capped rename
evicts exactly one entry, the running `retainedBytes` is never below a full
`measure()`, and at 50 that every step undoes and redoes, five mask selections
keep the bound, and a 1-byte budget leaves two entries with no Redo and one
reversible step; a windowed document's renames then match the exact measure.
The recording case runs last, on the live agent server: inside an open
`withUndoGroup` held across a modal worker wait it enqueues Assistant-shaped
`undo` and `restore_history` calls and posts an HTTP `redo`, and inside a
deferred `withUndoGroupDiscardingEmpty` it posts an HTTP `undo`; every reply
must refuse with "Another edit is still being recorded on this document. Wait
for it to finish, then <verb>." and change nothing, with no alert and no group
left open, and an `undo` after the recording ends is ok. Before the guard, the
first serviced `undo` raised `NSInternalInconsistencyException` and aborted
the run.

"free transform copy layers", "duplicate snaps to source" and "geometry commit
reveal" (`transform_followups.swift`) check the transform
follow-ups. With no selection ⌥⌘T validates, opens a session over the
duplicate (layers [1], the document untouched, the status "Free Transform ·
copy") and commits one "Transform Copy" step with one recorded
`transform_layers` `duplicate` step: the copy at (7,0), the original in place,
the copy selected, one undo back to the base; MCP `transform_layers duplicate`
answers `copies` [1] with the same state and equals a 30° commit given the
session's matrix, Esc and an identity Return (and an identity MCP call) change
nothing, a Position lock refuses with one alert and over MCP by name, and two
selected layers open nothing at 16 bits; at 8 bits, with two layers selected
and layer 0 active, the ⌥⌘T and MCP `transform_layers duplicate` both keep the
active layer's copy (index 1) as the primary. A text layer's copy stays
editable: at 8 bits ⌥⌘T (7,0) commits with no alert and the copy still
describes its source, ⌥⇧⌘T then adds a second described copy with no alert, a
recorded 15° ⌥⌘T replayed over MCP (`ActionPlayer.runSingle`) keeps the
description and lands the UI's pixels, MCP `transform_layers duplicate` with a
matrix answers `rasterized_layers` [] with the copy described, and a
true-perspective trapezoid refuses with `AgentServer.truePerspectiveRefusal`
until `rasterize_artwork`, which answers `rasterized_layers` [text + 1]; at 16
bits a lone text layer's ⌥⌘T at 15° keeps `hasArtworkDescription` on the
copy. `get_document` reports
row 0's content box (20,20,10,10), and an ⌥-drag with no selection and the
⌥⌘T box keep the source as their only snap target (the live copy is
excluded), a 2 px drag snaps back and the drag lands the copy on the source's
right edge, at 8 and 16 bits alike (the typed content box). The 16-bit Move, ⌘T and Crop and the 8-bit ⌘T and ⌥⌘T commit waits,
each behind a 0.1 s synthetic hold of the worker queue, keep their panel
present and hidden: every reading must be exactly 0 (never seen fails).

"transform again layers", "transform again selected pixels", "transform
again memory" and "transform again perspective" (`transform_again.swift`)
check Transform Again. The memory is the application's,
so the layers case clears it (`TransformAgainMemory.last = nil`) at each
depth; with no memory the item does not validate, adds no step and MCP `transform_again`
answers the empty refusal; a 15° ⌘T stores its matrix (the pivot fixed, kept
by undo and redo); ⇧⌘T opens no session, is one "Transform Again" step and
equals MCP `transform_layers` given that matrix and MCP `transform_again`;
⌥⇧⌘T adds the copy, selects it and equals MCP `duplicate`; both UI items
record `transform_again {}` and `{duplicate: true}` — at 8 and 16 bits (six
`sample_pixel` points at 16). Over a 16 px selection a 15° ⌘T stores a matrix
fixing the box centre, ⇧⌘T moves the selection and equals MCP, at 8 bits it
leaves a new hole and ⌥⇧⌘T none, ⌥⇧⌘T equals MCP `duplicate`, and a Pixels
lock refuses with one alert and over MCP by name. The memory case checks that
Esc and an untouched ⌘T keep it, a 5° ⌥⌘T and a parallelogram set it (the
affine), a true perspective stores its corners (validating, kept by undo),
`transform_layer`, `transform_layers`, a parallelogram `distort_layer` and
`transform_selected_pixels` set it, a perspective `distort_layer` stores its
corners and `transform_again` then repeats it (`corners` in the reply, one
undo), `set_layer_properties` keeps it, and at 16 bits two selected layers
do not validate while one does. As the `transform_again_after_edits.py` takes 1–3 measured it,
a same-size second document validates and repeats it, a 64×48 document
neither validates nor repeats a 64×64 memory (MCP names both sizes), a
transform there replaces it in every document, and closing the document it
was committed in keeps it; on a 64×48 document undo, redo, both undone and
redone, and History jumps before both transforms and back never change it;
`canvas_size` 80×48, `image_size`, `crop` and Rotate 90° make it unavailable
and their undo restores it (as does sizing back to 64×48, after which
`transform_again` repeats it); `flip`, `rotate` 180 and a quarter turn of a
64×64 canvas keep it. The perspective case,
at 8 and 16 bits: a ⌘T with its bottom-right corner dragged (6,4) stores
`Perspective(S, Q)`; ⇧⌘T is one step recorded as `transform_again
{rasterize_artwork: true}` and equals MCP `transform_layers {source_rect: S,
corners: Q}` and MCP `transform_again` (whose reply carries `corners`);
⌥⇧⌘T equals MCP `duplicate`; over a 16 px selection ⇧⌘T equals MCP; at 8 bits
a text-layer target refuses `AgentServer.truePerspectiveRefusal` over MCP
without `rasterize_artwork`, rasterizes with it, and prompts once in the UI; a
trapezoid remembered by `transform_selected_pixels` over (20,20,10,10) and
replayed over a selection above its horizon refuses in band and with one
alert (the core's sentence), and over the layer with
`RepeatableTransform.perspectiveRefusal`, leaving the document, selection and
memory unchanged. Then, at 8 bits, a two-layer ⌘T corner drag over layers with
transparent margins has the content union as its box, commits exactly MCP
`transform_layers` over that box, and the memory replays the same result.

"precision artwork transform" and "perspective
artwork set prompt" (`cmdt_fixes.swift`) check ⌘T over
editable artwork. On a 16-bit text layer a 15° ⌘T is one
"Transform Layer" step with no alert that keeps the artwork and equals MCP
`transform_layers` given the session's matrix (description, rect and six
`sample_pixel` points); a (7,0) translation and a shear keep it too; a true
perspective asks one "Rasterize text layer?" alert, rasterizes and records
`distort_layer {rasterize_artwork: true}`, and replaying that step through
`ActionPlayer` succeeds with the UI's result, at 8 and 16 bits; MCP
`transform_layer {rotate: 15}` and a sheared `distort_layer` (`rasterized:
false`) keep the artwork, and a trapezoid without `rasterize_artwork` refuses,
changing nothing. A 16-bit true-perspective ⌘T over two text layers asks exactly one
"Rasterize 2 editable artwork layers?" prompt and rasterizes both, and a 15°
⌘T over the pair asks nothing and keeps both.

"transform admission" (`transform_admission.swift`),
registered first so no later case's documents count against it, proves per-stage
admission — the largest sequential stage, `max(core, artwork)`, instead of the
old core + artwork + W·H·24 sum (at 16 bits, + the projection) — on five fresh
fixtures, each measured against the free working budget and each printing its
residency once open (`printResidency`: retained and transient bytes, B/px and
every category of at least 1 MB): a 37 MP 4:3 (7040×5280) 8-bit Rotate Canvas
17° (`CanvasGeometryJob`) whose core plan fits while the sum would not, then
publishes the rotated canvas; a 22 MP 3:2 (5760×3840) background plus text
layer turned 17° about (2880,1920) through MCP `transform_layers`
(`TransformWork.wait`), whose largest stage fits while the sum would not, then
answers `changed:true` with no rasterized layer, a background wider than
5760 px and the text layer still editable; a 30 MP 4:3 (6336×4752) 16-bit
Crop of a 5% inset straightened 7° with the default Catmull-Rom sampler
(`PrecisionGeometryGesture.finishOutcome`; the typed filtered turn's work
is bounded by the one-shot limit), whose largest stage fits while the sum would
not, then publishes the window with no alert; a 48 MP 4:3 (8064×6048) 8-bit
select-all `move_selected_pixels` by 16 px whose core plan fits while
core + W·H·24 would not, then publishes with no alert; and a 28 MP 4:3
(6208×4656) select-all `transform_selected_pixels` 15° bilinear whose
`max(core, σ)` fits while core + W·H·24 + σ would not (σ the transformed
selection), then publishes over the canvas box with no alert. Between the
16-bit Crop and the Move, a 24 MP (6000×4000) 16-bit Crop tool session at
Catmull-Rom takes two successive angle changes through
`refreshPrecisionCropPreview` (box (300,200,5400,3600), 6° then 7°): the
true preview sends `interactive` and keeps the interactive work limit, which
refuses it, so it lands at reduced resolution: status "Preview current
(reduced resolution; the commit renders at full size)"
(`GeometryProxyPreview.status`), `imageIsReduced`, and a 2700×1800
preview drawn into the 5400×3600 window (factor 2, from the commit plan's
work units, which the PASS line prints); the commit publishes 5400×3600 with
no alert. Then three 24 MP 16-bit
halves, each printing its wall time. H1: a ⌘T 17° at Catmull-Rom whose
preview lands reduced the same way, 3000 px wide; its commit, cancelled 0.4 s in
through the panel's Cancel, must return within 1 s of the click with nothing
published or recorded (the job's control reaches the kernel; 1.77 s before
that fix), and the uncancelled commit publishes a layer wider than 6000 px
with no alert; then, on a fresh fixture, a Crop straighten 7° commit cancelled
the same way, its session kept. H2: a bilinear Crop session takes
6° then 7° and BOTH previews must land at 5400×3600 (each was refused when a
preview worker counted its own reservation twice), the commit publishes the
window and the preview registry drains; it prints one handle snapshot
(`RasterResources.liveHandleSummary()`, the report's byte figures and host
counts). Between halves the
document closes and the free budget must climb back to its pre-half figure: the
load-independent fact that its memory is released, polled under a 120 s ceiling
only a leak reaches, with the drain time printed (`drained in <t> s`) and never
asserted, because how fast a 37 MP document drains depends on the machine's load
(a 10 s bound failed the Rotate half twice while other lanes built). The sizes balance both margins from the measured
residency (8 bits 12.00 B/px, 16 bits 20.04 B/px), and each PASS line prints
both.

"canvas geometry cancel" (`canvas_geometry_cancel.swift`) checks that the
Crop straighten and the filtered Rotate Canvas are one cancellable job. Each
Cancel leaves the document, revision, undo, dirty state, Actions, worker slot
and alert log untouched: the 8-bit UI Crop (a 3000×2000 fixture, box
(150,100,2400,1600), 7°) keeps its session's rect and angle, and a retry
publishes 2400×1600 as one undo step; the 8-bit UI Rotate Canvas 17° (a
3000×2000 fixture); MCP `crop` 7° and `rotate` 17°, each an in-band
`JobCancelled`. Each on its own fresh fixture with a text layer — the Crop at
13 MP 4:3 (4160×3120, box (208,156,3744,2808); its own check, "canvas
geometry 13 MP crop", registered second, right after "transform admission",
so the documents later cases leave alive in the process do not count against
its admission), the Rotate Canvas at 4000×3000 (its plan, ≈ 404 MB of
scratch, fits the 2 GiB reference working limit the runner sets) — a Crop and a Rotate
Canvas cancelled once the progress bar reads
≥ 0.6 (past the core step, on a 5 ms timer) keep the text description. Before
the Crop, the probe checks that the job's largest stage fits the free working
budget (the summed core + artwork + W·H·24 estimate fits too, so
"transform admission" carries the discriminating case); an uncancelled 13 MP Crop is then admitted, keeps the description and shows only
the "Crop" panel. A 16-bit 3000×2000 Rotate Canvas 17° cancels, then runs showing only the
"Rotate Canvas" panel (no "Preparing 16-bit Display"). Finally a whole-canvas
`crop` at angle 1e-10, a whole-canvas zero-angle `crop` and a same-size
`canvas_size` answer `changed:false` and record no step, and a straighten
window over 100 MP refuses with "The canvas cannot exceed 100 megapixels" (a
window past the canvas extends it). Last, the Crop tool's own whole-canvas straighten
at angle 1e-10, at 8 and at 16 bits (64×48), beeps
`CropSession.unchangedStraightenSentence` ("Nothing changed: straightening the
whole 64 × 48 canvas by 1e-10° changes no pixel.") with no edit, undo step or
Action, and at 16 bits a later box edit renders a new crop preview.

"auto-align cancel" and "stack mode cancel" (`stack_cancel.swift`) run
after it. Each builds a two-layer 4000×3000 document (the fixture and its
duplicate), calls MCP `auto_align_layers` or `stack_mode` (median) under a
`PanelCanceller` on the job's own panel ("Auto-Align Layers", "Stack Mode"),
and requires an in-band `JobCancelled` with the document, its revision, undo,
dirty state, Actions, worker slot and alert log untouched (`requireCancelled`,
shared from `transform_cancel.swift`). The same call, uncancelled, then answers
ok: the identical layers register with nothing `unregistered`, and the stack
adds its "Median" layer at index 2.

"merge panorama cancel" (`stack_cancel.swift`) runs after them. It
writes two identical 4000×3000 PNGs into the probe's folder and calls MCP
`merge_panorama` (Blend on) under a `PanelCanceller` on the job's own panel
("Merge Panorama"), requiring an in-band `JobCancelled`, no new document, no Action
step, a free worker slot and an unchanged alert log. The same call, uncancelled,
answers ok with nothing in `align.unregistered` and opens one document, which
the case closes.

"hdr toning cancel" (`stack_cancel.swift`) runs after it: MCP
`hdr_toning` (Equalize Histogram) on the same two-layer 4000×3000 8-bit
document under a `PanelCanceller` on the job's own panel ("HDR Toning"),
requiring an in-band `JobCancelled` with the document, its revision, undo,
dirty state, Actions, worker slot and alert log untouched. The same call,
uncancelled, answers ok, `flattened` and `bit_depth` 8, with one layer.

"save and open cancel" and "snapshot fingerprint cancel" (`save_open_cancel.swift`) run after
it. Each row clicks its job panel's Cancel (`PanelCanceller`) while a `QueueHold` keeps
`BackgroundWork.queue` blocked until the click lands, so the worker starts with its control
already cancelled however fast it would run. Save rows (8-bit native and JPEG, 16-bit native,
TIFF and JPEG, the last a downconvert-then-encode compound): save, edit, save again and cancel;
the save completes with `NSUserCancelledError`, the file keeps the first save's bytes, no staged
name is left, the document stays edited, no alert is recorded and the worker is free; a native
row also cancels the close-time `save(withDelegate:…)`, which answers `didSave` false; then an
uncancelled save succeeds. Open rows (an 8-bit PNG, a `.doctored`, a 16-bit TIFF) throw
`JobCancelled` and open nothing, then open. MCP `save_copy` answers `JobCancelled` in-band and
writes nothing. The second case cancels `ImageSourceFile.fingerprint` and
`RawSourceSnapshot.capture` (an uncancelled fingerprint is the file's SHA-256), then the
"Capturing Camera RAW", "Reading Image" and "Checking Linked Image" waits from MCP
(`place_image`, `get_image_source` over a linked `develop.dng`), each answering `JobCancelled`
rather than a link status, and each running uncancelled after.

"float quick mask fill" and "target default layer" (`depth_targets.swift`) run
after "info analysis". The first opens the committed 32-bit float fixture
(`core/tests/fixtures/float_11f/scene.tiff`) without activating the app, sets
u16 coverage 129 and enters Quick Mask: the six fill items validate, Fill with
Foreground white fills the coverage to 65535 as one file-clean "Fill" entry
recorded as one `fill_selection` step with `target: "quick_mask"`, Undo/Redo
walk it, MCP `fill_selection` black empties it, `stroke_selection` still
refuses, and outside Quick Mask Fill… and Fill with Foreground validate and
`fill_selection` fills the float layer (`changed: true`, `storage:
"rgba32_float"`), while a pinned `target: "quick_mask"` fill refuses
("is not in Quick Mask") and paints nothing. The second puts the panel
on a 16-bit mask: an untargeted Multiply `eraser_stroke`
gets "16-bit retouch supports Normal blending only." from `DocumentPrecision`
and `target: "mask"` is admitted (the case used `gradient` until the
16-bit gradient gained a blend mode).

"float paint strokes" (`float_paint.swift`) runs after "float quick mask
fill", on the same float fixture. `DocumentPrecision.allows` admits the Brush,
Eraser and Gradient, and since 4dbba7db the bucket and Dodge too. A
canvas Brush stroke (`beginStroke` / `continueStroke` / `endStroke`) with a
(4, 0.5, 0.25) foreground is one "Brush Stroke" job whose centre pixel reads
those numbers, is one undo, and records one `brush_stroke` step carrying
`rgba32` and no `color`, `rgba` or `color_space`. An Eraser stroke at opacity
0.5 halves alpha and keeps RGB; a stroke cancelled before the first poll ends
`cancelled` with nothing applied. With a fresh reveal-all mask targeted, an
Eraser stroke zeroes the u16 mask in its core as one undo step, and
`gradientPrecisionColor` black to white writes the coverage ramp (0 at the
left edge, 65535 at the right) as one step recorded with `target: "mask"` and
no `start_rgba32`, with the layer's pixels unchanged (read with the mask
disabled). On the layer, `gradientPrecisionColor` with that foreground records
`start_rgba32` / `end_rgba32` alone and lands both endpoint values. Last, a
Brush whose stored mode is Screen starts no stroke and records one alert
naming the mode.

"float retouch strokes" (`float_retouch.swift`) runs after it. The float
fixture is enlarged to 160 × 120 with `canvas_size`, ramped with `gradient`
(red 0 to 8 across the width, so most of it is above white) and given one
dark `brush_stroke` blob. `DocumentPrecision.allows` admits the Clone Stamp,
Healing Brush, Spot Healing, Patch, Remove and Content-Aware Move, Red Eye
and the Blur, Sharpen, Smudge and History brushes, and since
4dbba7db the bucket, Dodge, the Sponge and the Magic Wand. With the Clone Stamp selected and `canvas.cloneSource`
set, `retouchSourceOverlayImage()` and, mid-stroke, `strokeSourceImage` each
return an image with no alert (a float plate is displayed through the
document's tone map); the stroke is one job whose centre pixel reads the
source's above-white values, and one undo; a clone cancelled before the first
poll ends `cancelled` with nothing applied. A Healing Brush stroke from 40
rows up and a Spot Healing stroke over the blob each complete as one job and
one undo and read the scene there (2 % and 5 %). A Patch drag of a square
around the blob (`patchSession`, `onPatchCommit`), a Remove stroke
(`removeStrokeCommitted`) and a Content-Aware Move of that square
(`contentAwareMoveCommitted`, the blob landing 35 rows up) each complete as one
job and exactly one undo. With the work path current and a (4, 0.5, 0.25)
foreground, Stroke Path validates enabled and `strokeCurrentPath` is one
"Stroke Path" undo step whose recorded `stroke_path` carries `rgba32` and no
`color` or `color_space`, the line reading those numbers. Last, a Clone Stamp
whose stored mode is Screen starts no stroke and records one alert naming the
mode.

"float retouch brushes" (`float_retouch_brushes.swift`) runs after it,
on the same 160 × 120 ramp with one `brush_stroke rgba32: [5000, 5000, 5000,
1]` blob. `DocumentPrecision.allows` admits the Blur, Sharpen, Smudge and
History brushes, and since 4dbba7db the Sponge. Each canvas stroke (`beginStroke` /
`continueStroke` / `endStroke`) is one job, and one undo returns the document
handle and the history's current state to what they were. A Blur stroke
across the blob raises no alert, lifts a pixel beside the blob to a green
above 1 and lowers the blob's centre, which stays above 1. A Smudge stroke
from the blob 30 px left leaves the stroke's end, scene before, reading a
green above 1000. With `history.brushSourceID` set to the picture's state and
a bright `brush_stroke` painted over the ramp, a History Brush stroke at full
opacity makes the line's middle pixel read exactly what it read before the
paint.

"float filters" (`float_filters.swift`) runs after it, on the
same float fixture. All twenty-four Filter menu items a float layer runs
validate on the layer (the eleven linear ones, Add Noise and Pixelate, and the
eleven run on the display encoding, Presence and the Develop Filter among
them), and so does Remove Red Eye; none of the twenty-four validates with the
mask targeted; in Quick Mask (entered with a selection, so there is coverage) only
`showBlur:` validates, as the Quick Mask blur. A Gaussian Blur request through
`PrecisionAdjustmentSession` lands a preview in `canvas.previewImage` with the
document untouched; its commit changes the float layer as one undo step and
one recorded `apply_filter` `blur` step, with a red above 1 still in the 8.0
block's row; `repeatLastFilter:` then validates and adds one more step.
`showLiquify(nil)` presents the workspace with `canvasView.image` at the
proxy's size (the layer's own 48 × 32) and Apply disabled; a full-strength
size-16 push from (20, 16) to (26, 16) through `strokeBegan` / `strokeMoved` /
`strokeEnded` changes that image and enables Apply; the Apply button moves the
layer's linear sample from (20, 16) to (26, 16) in one undo step.

"byte set perspective prompt", "set prompt decline and truncation",
"parallelogram copy replay" and "copy preview tick cost"
(`transform_prompts.swift`) run after them. An 8-bit
true-perspective ⌘T, ⇧⌘T and ⌥⌘T over two text layers each ask exactly one
"Rasterize 2 editable artwork layers?" prompt and rasterize both (⌥⌘T the
copies, the originals stay described); the background plus one text layer asks
one "Rasterize text layer?" carrying `setRasterizeReason`. A declined set prompt (a probe-local `NSAlert.runModal`
swap, restored in a `defer`) at 8 and 16 bits publishes nothing, keeps the
session and names the decline note; six text layers list five names and
" and 1 more". An 8-bit ⌥⌘T parallelogram over a text layer asks nothing, keeps
the copy described, and its recorded `transform_layers` step and a direct call
land the same copy. The last case times the 24 MP 16-bit text ⌥⌘T preview's
duplicate, per-tick work and whole ticks against ⌘T's (information, not
assertions); the recorded run printed `24 MP 16-bit text ⌥⌘T preview tick
on main (Swift -Onone, release core): duplicate median 3.88 ms, max 5.05 ms;
⌥⌘T work median 26.62 ms, max 28.31 ms; ⌘T work median 22.02 ms, max 23.84 ms;
⌥⌘T tick median 30.90 ms, max 32.96 ms; ⌘T tick median 27.15 ms, max 32.41 ms;
frame budget 16.7 ms; verdict ok`.

"preview own lease" (`preview_admission.swift`)
runs after them. It opens a 3000 × 2000 16-bit document, takes P =
`projectionScratchEstimate()`, evicts, holds a `PrecisionEditPayload` ballast
leaving F = 1.5P + 64 KiB free (it requires P + 64 KiB ≤ F < 2P + 64 KiB and
prints both), and requires the typed opacity drag's preview to land: its worker
admits P beside its own P + 64 KiB reservation unless it runs that reservation
as its own lease, so with the drag on `run` the case fails. The ballast is then
released, the drag cancelled, and the registry must drain. Then a 16-bit and a
masked 8-bit text-layer ⌘T 17° must each hold, while the preview is in flight,
at least 132 MiB of payload bytes (the artwork stage's lease, which every
admission counts on every thread) beside a nonzero core reservation, land, and
release both once the session ends.

"stroke layer edge" (`stroke_layer_edge.swift`) runs after them: Edit ▸
Stroke… with nothing selected, through `applyStroke` / `strokeSelection(nil)`
and MCP `stroke_selection`. A text layer added at least 30 px inside a 200 × 140
document, stroked Outside 10 px (its buffer keeps 4–8 px of transparent padding
round the glyphs, which a 4 px band never leaves), must raise exactly one
"Rasterize text layer?" alert and one "Stroke" step, drop the description and
grow to a rect that
strictly contains the old one inside the canvas; after undo MCP must answer
`outline: "layer_edge"`, `changed: true` and land the same rect and pixels. A
background moved 20 px left, stroked Inside 6 px, must keep canvas (0, H/2),
paint layer-local (0, H/2) the stroke colour, keep its rect and equal MCP.
It then prints an INFO line (no bound): the median of three Outside 4 px
strokes, scratch query plus job, of a sparse 6000 × 4000 layer holding one
200 × 200 square, whose rect must stay unchanged.

"crop delete cropped", "fill grows trimmed layer", "convert precision default",
"new entries into group", "window memory" and "long text line"
(`v1_fixes.swift`) run after them. On a 64 × 48
document at 8 bits (with a 10 × 10 layer straddling the window's corner) and at
16 bits, a Crop tool commit over (10, 8, 30, 20) with Delete Cropped Pixels and
MCP `crop delete_cropped:true` must leave each plain raster as its intersection
with the new canvas holding the source's own samples, as one "Crop" undo step
whose recorded step carries `delete_cropped`; without the option the layer keeps
its whole rect and the step has no such key. A 5° straighten over the same
window must leave each plain raster as the plain straighten's raster clipped to
the canvas with its samples, from the Crop tool's commit and from its recorded
step replayed over MCP. (The Live Photo claim row went with 35befd6b, which
removed Live Photo layers and the straighten's `rasterized_layers`: every
description a straighten meets now re-renders.) Fill… (`applyFill`) and
`fill_selection` with nothing selected must grow a 10 × 10 layer at (20, 15) to
the 64 × 48 canvas, every pixel the fill colour, as one "Fill" step, and keep
its rect under Preserve Transparency and for a selection inside it.
`ConvertPrecisionSheetController.initialStructure` must answer Preserve Layers
for a two-layer 8-bit, a 16-bit and a 1,024-layer document (the most the host
can build: the core refuses the 1,025th layer, so the record cap's blocked plan
stays with the core's tests), and Flatten with the core's sentence for a layer
carrying a Live Photo claim; the sheet's radios and status line must agree. With
a group active, New Layer and a parameterless New Adjustment Layer must land as
the group's top child, select it, leave the disclosure as it was and record
`position: "into_group"`, and the recorded `new_layer` step must land the same
place over MCP; with a plain layer active both land above it and record no
position, at 8 and 16 bits. `EditorWindowMemory.placement` must keep a frame
with no window at its origin, step it (22, −22) once per open window at the
stepped origin, and fit a frame off or larger than the display onto it; a memory
must round-trip through a private `UserDefaults` suite and load nil when empty
or unreadable; `EditorWindowMemory.isEnabled` must be false under the runner, a
moved and closed editor window must write nothing, and `applyPanelLayout` must
set the panel's visibility and tab. A 1,000-character single line of prose,
added at 24 px with `wrap_width: 0`, must render as one line with opaque ink
and ink in its last tenth, and one undo must remove it; 2,000 characters must
refuse with "artwork outline exceeds the flattened edge limit" and change
nothing (the bound is 524,288 edges, and the pangram is about 450 a character
at 24 px). `ArtworkGeometryWork.scratchBytes` over two text layers must be the
118 MiB coverage bookkeeping once plus each layer's own planes, and an MCP 3°
straighten of that document must re-render both.

"late busy reopen", "typed style preview" and "layer via typed"
(`v1_16bit.swift`) run last. The first, at 8 and at 16 bits
(320 × 240), presents the real Gaussian Blur sheet on a text layer, moves its
slider to sigma 7 and presses Apply under a probe-local `NSAlert.runModal` swap
(restored in a `defer`) that answers "Rasterize" and, on the first prompt only,
takes the worker slot with a `WorkerHolder`: the slot is taken after the sheet's
own slot wait, behind the prompt. Apply must return with exactly one prompt
asked, no alert recorded and the document, history and undo stack unchanged,
and a second `SliderSheetController` must be up with its slider at 7 and
`BackgroundWork.busyMessage` on `tone.preview.status`. After
`holder.releaseNow()` a second Apply on that sheet must ask again, commit as one
history entry that rasterizes the layer and undo back to the source; at 8 bits
the "Gaussian Blur" job panel's bar (every panel's bar zeroed before the press)
must read at least 0.999 when Apply returns. Each depth waits out
`BackgroundWork.anonymousWaitLimit` (5 s) once. The second opens the Layer
Style sheet on Color Overlay (`editLayerStyle(0, initial: .colorOverlay)`) on a
16-bit 512 × 512 and a 3000 × 2000 document: the canvas preview must land at
512 × 512, and at `ProxyScale`'s 600 × 400 with
`PrecisionStylePreview.reducedStatus` on `layer-style.status`, and its mean
colour must be the overlay's red (an opaque layer under a 100% Normal overlay);
Cancel must leave no preview, `SelectionWorkBudget.scratchBytes` at its value
before the sheet and no edit, with nothing landing in the next 0.3 s; Apply on a
fresh sheet must publish one step whose layer carries the authored overlay on
the full-size document, leave the registry at the same value, record no alert
and undo back. A Color Overlay has no lengths, so one more 3000 × 2000 document
carries a 100 px inside #00ff00 Stroke and opens the sheet on Stroke: on the
600 × 400 preview's middle row column 10 must be the stroke's green and column
30 must not be, which is the 20 px band `scale: 1 / factor` gives (an unscaled
style draws 100 px, one scaled the wrong way 250 px). The third sets a (10, 8, 30, 20) selection on a 16-bit 64 × 48
document: `layerViaCopy:` and `layerViaCut:` must validate, and each action must
add "<name> copy" above the source, active, holding the source's samples inside
the selection and alpha 0 outside (the cut leaving alpha 0 inside the source
and its samples outside), as one "Layer Via Copy" / "Layer Via Cut" step that
one undo removes, with no alert.

The v1 UI coverage cases drive the sheets and menu actions of the 32-bit float,
stack and Develop batches through their own controls, as a user would, and come last in
`checks` (the Develop cases at the very end). `ui_support.swift` holds their
shared helpers: `ModalSteps` drives one `ModalCardWindowController` card (the
modal window with the given title) from a timer in `.common` and `.modalPanel`,
one step per tick, and aborts the modal on a throw or a 90 s stall;
`uiControl` finds a control by its accessibility identifier; `uiValidates`
asks `validateUserInterfaceItem`; `uiRecorded` brackets a body with
`ActionRecorder`. Each case closes every document it opened (including the
ones Load Files into Stack and Merge to HDR create), stops the recorder in a
`defer`, and asserts the handle, the history, the undo stack, the recorded
steps and `AutomationAlerts.recorded`. File panels are never run: Load Files
into Stack and Merge to HDR are entered past their open panel
(`AppDelegate.loadFilesIntoStack(urls:)`, `AppDelegate.mergeToHDR(urls:)`).

"view exposure sheet" (`ui_float_view.swift`): 32-bit Exposure…
validates on a float document only and Reset only at non-zero stops; the
card's field at −2 previews in the canvas's proof slot and Cancel puts 0 back;
the slider at −3 and Done keep "Exposure -3.00 EV" with no step, undo, dirty
flag, Action or alert; across an MCP `canvas_size` no exposed image of the old
size is up at any pump tick; a render refused for memory (a held
`PrecisionEditPayload` ballast) clears the slot. "tone map sheet preview": a
chosen operator is a candidate drawn in the proof slot with the document
untouched; Cancel clears it with no step; Apply is one "Tone Map" step and one
recorded `set_tone_map` step that one undo restores. "float export rows": the
float-only formats are hidden on an 8-bit document and offered on a float one;
OpenEXR and Radiance pin the depth to 32-bit float, half samples encode smaller
in OpenEXR and TIFF, Radiance disables them; Cancel leaves no step, dirty flag,
Action or alert. The write itself sits behind `NSSavePanel.beginSheetModal`
and is NOT reached.

"presence sheet" (`ui_presence.swift`): at 8 and 16 bits Filters ▸
Presence… opens three sliders; Apply at 0/0/0 is no step and records nothing;
30/20/10 previews with the document untouched, and Apply is one step, one
recorded `apply_filter` `presence` step carrying the three integers, and one
undo; it validates on a float document too (see "presence float").

"smart sharpen sheet" (`ui_smart_sharpen.swift`): at 8 and 16 bits Filter
▸ Sharpen ▸ Smart Sharpen… opens ten sliders and the Remove popup at their
defaults; amount 150, radius 2, noise 20, shadows fade 30 and Gaussian Blur
preview with the document untouched, and Apply is one step and one recorded
`apply_filter` `smart_sharpen` step carrying the settings; ⌃F repeats it as one
more step and two undos restore the document; the item validates on a float
document.

"develop filter sheet" (`ui_develop_filter.swift`): Filters ▸
Develop Filter… sits directly above Lens Correction…; the sheet opens with
its preset popup on Default, Contrast 40 reads Custom, and Reset puts Contrast
back to 0 with the popup reading Default; Cancel changes and records nothing.

"load files into stack", "auto-align action", "stack mode items" and "reduce
noise stack sheet" (`ui_stacks.swift`) work on value-noise PNGs the case
writes. Load Files into Stack sorts "frame 10", "frame 2", "frame 1" into
natural order bottom-first; a file of another size grows the canvas and shows
exactly the notice the load returns (none for these files); a file that is no
image shows one "Could Not Load the Files into a Stack" alert and opens no
document. Auto-Align Layers validates with two
selected raster layers and not one, aligns a layer drawn (24, −16) off within
8 codes at five interior points as one entry and one recorded step, and on two
identical layers changes and records nothing (this found the defect below);
a (3, −2) shift on a 4096 × 3072 frame registers within 1 px at every corner
and aligns as one entry.
Each Stack Mode item, by its tag, adds a layer named after the mode on top,
active, with its sources hidden, as one entry and one recorded `stack_mode`
step; a tag out of range changes nothing. Reduce Noise from Stack opens on
Median; Cancel changes nothing; Mean and Apply add "Noise Reduced" as one
recorded `reduce_noise_from_stack` step. Found: on two identical 800 × 600
layers Vision's two-pass homography moved the corners 0.12 to 0.39 px, past
`AutoAlign.unchangedThreshold`, so Auto-Align resampled an already-aligned
layer and left an undo step; `AutoAlign.register` now gives way to the
identity when the answer agrees no better than it and moves no corner more
than half an analysis frame pixel (`AutoAlign.identityRadius`).

"merge to hdr sheet" and "merge layers to hdr sheet" (`ui_merge_hdr.swift`),
on three exposures of one scene (gains ¼, 1 and 4): the card shows one
EV field per file; an empty or non-numeric one disables Merge; an edited EV,
Align off and Remove Ghosts on merge into one new 32-bit float document with
the files' bytes unchanged; Cancel opens nothing. Merge Layers to HDR…
validates on three selected layers of an 8-bit document and not in a float
document whose two selected raster layers pass Auto-Align's test (so only the
float rule disables it); its Merge opens a new float document and leaves the
source's handle, history, dirty flag and recorder alone.

"float lens correction sheet", "float liquify sheet" and "paths panel stroke
float" (`ui_float_sheets.swift`) on a float layer: Lens Correction's
first slider previews with the document untouched, Cancel leaves nothing, and
a second sheet's Apply is one step and one recorded `apply_filter` step that
keeps a red above white; Liquify's push then Cancel leaves the handle and the
undo stack, and a second workspace's push and Apply record exactly one
`liquify` step; the Paths panel's Stroke Path button and its row menu's Stroke
Path item, each sent up the responder chain, are one "Stroke Path" step
recorded with `rgba32` alone and the line above white. The case also sends MCP
`gradient` with three `stops` to the float layer and prints whether it answers
ok (it does).

"new document float" (`ui_new_document_float.swift`, 698b2d10) runs File
▸ New… (`AppDelegate().newDocument(nil)`) under `ModalSteps`: Pixels, 64 × 48,
sRGB, the Depth popup's "32 bits/channel (float)", then Create. White reads
[1, 1, 1, 1] at every pixel on a layer named "Background", Black [0, 0, 0, 1],
Transparent [0, 0, 0, 0] on "Layer 1", and Custom #336699 (the popup item
selected without its action, which would order the colour panel on screen,
and `colorPicked:` sent straight to the card, the popup's retitled item showing
it landed) the linear sRGB decode of its codes within 1e-5 absolute, the
core's own bound for the built-in TRC (`core/tests/float_promote.rs`: a
1024-entry 16-bit `curv`). Each is a float document with no undo step, nothing
recorded and no alert. `NewDocumentProfile(name:)` refuses the Generic Gray
profile with a sentence naming its path and ending "Grayscale documents are
not supported.", and `new_document` at depth 32 refuses in band with it and
opens nothing.

"convert precision float" (`ui_convert_precision.swift`, 698b2d10):
Image ▸ Mode ▸ 32 Bits/Channel… opens the Convert Precision sheet on "To
32-bit float"; Prepare Preview and Apply Conversion turn a 64 × 48 8-bit
document into one "Convert to 32-bit Float (Preserve Layers)" step recorded
`convert_precision {mode: rgba32, structure: preserve}` whose pixels are the
linear sRGB decode (within 1e-5), and Convert Precision… "To 8-bit" (Preserve,
no dither) returns the original bytes exactly, recorded with `dither: none`; a
16-bit document returns every sample through "To 16-bit". With a Screen layer
the open-at-32 status reads `Preserve Layers cannot convert this document:
layer 1 "Layer 2": the screen blend mode is unavailable …`, Prepare keeps it
with Apply disabled, "To 16-bit" clears it with Preserve on and Flatten
enabled, and back to float restores it exactly with Flatten disabled; Convert
Precision… opens on "To 16-bit" with no status and choosing "To 32-bit float"
shows the same sentence with Apply disabled. Nothing blocked changes, records
or alerts. Found: `choiceChanged(_:)` re-derived the status line only when
leaving 32-bit, so entering 32 through the Mode popup showed an empty (or the
previous mode's) status while 32 Bits/Channel… showed the float plan's refusal;
every mode change now re-derives the status, and the radios still change only
on leaving 32-bit.

"select people" (`ui_people.swift`, 0af0e921) reads
`multiple-people/IMG_9639.jpg` and `phase6/photos/fur.png` in place from
`DR_JOBS_PROBE_FIXTURES` (`verify.py` passes it, defaulting to
`~/devel/doctored-fixtures`); PRIVACY: nothing derived from the photos is
written and the case prints numbers only. On the 8-bit photo Select ▸ Select
People… lists "Person 1" and "Person 2"; row 1 alone and Apply select exactly
`analysis.mask(subjects: [1])`, the other face's pin outside, as one step
recorded `select_subjects {model: people, instances: [1]}`; the preview's
click at a face pin toggles person 1 off and on, and adds them to [2]; All
People and Select ▸ All People select `analysis.aggregateMask` as one "Select
All People" step each, recorded `select_people`. After `convert_precision`
to 32-bit float the same flow finds the same two people at the same pins, each
x centroid within 1% of the width. On fur.png each command records exactly one
"No people were found in this image." and presents, selects and records
nothing.

"lens correction profile" (`ui_lens_correction.swift`, 58f5cc9c): a
quarter-size JPEG of Z6.NEF with its EXIF (`verify_v1_lensfun.py`'s camera and
lens). That drive develops the NEF in the app, which plans 2.46 GB, over this
process's 2 GiB pin, so ImageIO writes the JPEG from the camera file's own
embedded preview and metadata into `DR_JOBS_PROBE_DIR`; it opens as
`open_document` opens it. The Lens Profile popup opens on "Auto — NIKKOR Z
24-70mm f/4 S" with the status naming the Nikon Z 6, the lens and "Lensfun
v0.3.4"; Apply is one "Lens Correction" step recorded `profile: "auto"` that
keeps the centre within 1 code and moves a 16 × 16 corner patch, with the same
bytes as `apply_filter lens_correction profile: "auto"`; the lens picked by
name records its id with Auto's pixels; None adds no step. On a picture with
no EXIF the popup reads "Auto — no match" with None selected ("No lens
profile."), Auto's status starts "No Lensfun profile:", and its Apply adds no
step.

"float paint bucket", "float dodge", "float sponge", "float magic wand",
"float quick selection" and "float color range sheet" (`ui_float_tools.swift`,
4dbba7db) run the canvas's own entry points on 160 × 120 bands 0.25 |
1.0 | 2.0 | 8.0 (the R1 display copy reads the last three as white). A
tolerance-0 Paint Bucket click (`onFillClick`) on the 2.0 band fills the three
white-reading bands with (4, 0.5, 0.25) exactly, recorded `rgba32` with
`fill_mode: color_match`. A hard Dodge stroke's gain at 2.0 and 8.0 is its gain
at white within 1e-5 (R4) and it changes the 0.25 band, as one job; on a mask
seeded to 32896 (`fill target: mask #808080`, F46 (b)) the stroke raises the
coverage under it, leaves every sample 25 px from its centre line at 32896 and
no layer pixel, recorded `dodge_burn` with `target: mask` (R4m). A Saturate
Sponge stroke over (4, 0.5, 0.25) moves each channel away from its Rec. 709
luma by one multiple, red past 4. A tolerance-0 Magic Wand click
(`onWandClick`), a Quick Selection drag (`beginSelectionStroke` …
`endSelectionStroke`) and Select ▸ Color Range…'s preview click and Apply each
select the white-reading bands and not the 0.25 band. Each is one step, one
recorded step and one undo, with no alert.

"select and mask float" and "magnetic lasso float"
(`ui_select_mask_float.swift`, 8d1f952b), on a 0.1 background with an
80 × 60 square above white: Select and Mask opens with Decontaminate Colors and
its Amount disabled; Edge Radius 4 and Smart Radius with each output is one
step recorded `refine_selection` with that output, `edge_radius` 4 and
`smart_radius`: the selection and the layer mask cover the square's inside and
not the background, New Layer keeps [8, 4, 2, 1] above a clear outside, and New
Layer with Layer Mask keeps [8, 4, 2] under a revealing mask. Four Magnetic
Lasso clicks (`magneticLassoMouseDown`) with hovers (`moveMagneticLasso`) along
the square's edges, closed by a double-click, are one "Magnetic Lasso" step
recorded `select_freehand` selecting at least 95% of the square and nothing
farther than the Width from it.

"presence float" and "develop filter float" (`ui_presence_float.swift`,
3cf8a8aa, 8d1f952b), on P = (4, 0.5, 0.25) with Q = P / 4 at one pixel, a
block H = (8, 1, 0.5) and an in-range ramp: Presence's three sliders are
enabled and Dehaze 40 keeps P's red at exactly 4 (rule D) while changing the
ramp. The Develop Filter's Highlights, Shadows, Vibrance and Saturation rows
and sliders are enabled; Highlights −50 with Shadows +50 keeps H exactly (rule
H holds a pixel whose luma on the display-encoded copy is at least 1; P's is
about 0.95, so P is not held) and changes the ramp; Saturation +40, and
Vibrance +40 from a fresh base, keep out(P) = 4·out(Q) within 1e-4 (rule V).
Each Apply is one step recorded `apply_filter` with its values, one undo; the
Develop Filter's step is named "Develop Filter", as at 8 bits (the 16- and
32-bit commit takes the sheet's action name).

"develop range rows", "develop generated masks" and "develop output space"
(`ui_develop.swift`) open camera files `raw_fixtures.py` builds into
`DR_JOBS_PROBE_RAW_DIR` (`verify.py` runs it as a child process and prints its
log; a missing file fails only the case that needs it, naming the file):
`develop.dng` (`phase3_fixtures.synthetic_dng`), `flat.dng` and `portrait.dng`
(`verify_v1_raw_subject`). In Redevelop RAW a New Range local's Low, High and
Low falloff reach the last operation and the local, the overlay changes with
them, Colour with no colour asks for one click with Sample Colour and the click
resolves an L a b readout into the local, and Cancel publishes nothing. On the
portrait New Subject and New People make generated locals with coverage,
Recompute is enabled only for a generated one, and Apply is one undo step; on
the flat picture each button leaves no local and names its sentence on the
status line, with no alert. In the import dialog the output popup lists the
four spaces, Adobe RGB (1998) and ProPhoto RGB land previews carrying their
profiles, the depth note follows `RawOutputSpace.depthNote(eightBit:)` at both
depth rows, and Open returns ProPhoto RGB.

"develop lens line" (`ui_develop_lens.swift`, 58f5cc9c, b7114688) runs last,
on `camera/camera.NEF` from `DR_JOBS_PROBE_FIXTURES` (no decoder lens profile,
a Lensfun match), with its own copies of the Develop helpers. In Redevelop RAW
the line under Enable Profile Corrections reads "corrected by the Lensfun
profile <lens> (Lensfun database v0.3.4)" with the checkbox enabled and on;
unticking it changes the preview and ticking it changes it back. A radial over
the centre (radius 12% of the frame) with Exposure +1 and Apply is one recorded
`edit_raw_development` step whose top and bottom 8% strips keep the
development's 16-bit samples within 1 code while the centre is brighter, one
undo.

"default shortcuts" (`shortcut_defaults.swift`) walks
`AppDelegate().buildMainMenu()` whole — the app menu, the system selectors and
the hidden twins included, with `NSApp.windowsMenu` and `helpMenu` put back —
and requires every chorded item to equal the probe's complete identity → chord
table (the audit in `designs/v1-shortcuts-fade.md`), no chord held by two items,
and `EditorTool.keyCharacter` to equal its expected letter map (Paint Bucket on
G). It then runs the withhold rule through `ShortcutMenu.apply` alone, writing no
file: an override putting ⌃⌘M on Rotate 90° Clockwise leaves Window ▸ Minimize
with no chord, and empty bindings give it back. Last, a set decoded through
`ShortcutBindings.decode` with overrides under the earlier identities
`showCameraRawFilter:` and `photomerge:` (`LegacyNames`) puts each chord on
Develop Filter… and Merge Panorama…, and a set binding both
`showCameraRawFilter:` and `showDevelopFilter:` keeps the current identity's
chord.

"fade sheet" (`ui_fade.swift`, v1) runs Image ▸ Adjustments ▸ Invert at 8 and 16
bits; Edit ▸ Fade… then validates titled "Fade Invert…", its sheet at 50 %
previews with the document untouched, and Apply is one "Fade Invert" step in
place of "Invert" (the history keeps its entry count) recorded as `fade`
{opacity 0.5, mode Normal}, at 8 bits mid-grey within one code; one undo restores
the original, and a selection change after a second Invert disables the item. At
32 bits (`floatFixture`) the sheet's Mode popup lists exactly
`NativeLayerBlending.floatModes`, Screen omitted.

"curves sheet" (`ui_curves.swift`) requires Curves… to carry ⌘M, then at 8
and 16 bits lifts the midtone on the sheet's own curve and presses Apply: one
"Curves" step recording `apply_filter` with filter "curves", undone by one undo.

"curves pencil" (`ui_curves_pencil.swift`) draws on the Curves dialog's plot with its
Pencil, presses Smooth and Apply (destructive at 8, 16 and 32 bits on RGB and at 8 and 16 bits on
a CMYK document's ink, and an adjustment layer's create and edit arms), checking each map against
the file's own stroke and Smooth and points → map against the core's byte table.

"cmyk type tool" (`ui_cmyk_artwork.swift`, the CMYK artwork batch) admits the Type and shape tools
and the New Fill Layer items on a CMYK document, picks K 100 as the foreground ink, types "Ink" in
a canvas session and requires the committed text layer's stored ink to be exactly `[0, 0, 0, 1, 1]`;
an unchanged reopen adds no step and an edit of the string keeps the ink. "cmyk fill layer sheet"
reopens a gradient fill layer with a K-only stop from the Layers panel, changes only the angle on
its sheet and requires one "Edit Fill Layer" step with `stop_inks` unchanged.

Not reached by these cases: the file panels themselves (`NSOpenPanel` before
Load Files into Stack and Merge to HDR, the Export window's `NSSavePanel` and
the write behind it, and New Document's Profile File… `NSOpenPanel`), which no
case may run; and the colour panel itself, which "new document float" never
orders on screen (the pick is sent to the card as the panel sends it). Every
float tool case's canvas path runs in-process, so no tool is left undriven. With the thirty-one UI
cases the two passing full runs took 548 and about 463 s (2026-10-13; an earlier 488 s run failed
"info analysis", fixed since; 378 to 417 s with the first sixteen),
well inside `verify.py`'s 1337 s bound.

"large filter jobs" (`large_filter_jobs.swift`) is an OPT-IN case in the
`measurements` list, run with the 16 GB Mac's limit:
`DR_WORKING_LIMIT_BYTES=8589934592 DR_JOBS_PROBE_CASES="large filter jobs" python3 scripts/jobs-probe/verify.py`
(it fails at a smaller limit). At 8 and at 16 bits it builds an 8192 × 5504
(45 MP) document in the core, fills it with Clouds through `apply_filter`, and
then for Dust & Scratches r2, Surface Blur r5 T15 and Radial Blur (Spin, Good,
Amount 10), each through `apply_filter` (the job the sheets' Apply, replayed
Actions and ⌃F run): first cancels the job at its progress panel's Cancel
(`PanelCanceller`) and requires an in-band `JobCancelled` with the document, its
revision, its history position, its undo state and its dirty state unchanged,
the worker slot free and no alert; then lets it complete and requires one
history step, a changed sample among a 12 × 8 grid of `sample_pixel` readings,
and one undo back to the base. Twelve PASS lines, one per row; the seconds they
print are information, not a verdict.

"slider preview latency" (`slider_preview_latency.swift`) is an OPT-IN
measurement: it sits in `main.swift`'s separate `measurements` list, so a run
with `DR_JOBS_PROBE_CASES` unset never runs it, and only naming it does
(`DR_JOBS_PROBE_CASES="slider preview latency"`; the unknown-name check covers
both lists). It drives the real Surface Blur (r5 T15 → r10) and Gaussian Blur
(σ2 → σ12) sheets through their own slider actions and Esc Cancel button, one
fresh document per sheet inside its own autorelease pool, on a 6000 × 4000
layer at 8 and 16 bits. The Reduce Noise (r1 → r2; four changes 1.25, 1.5,
1.75, 2) and Dust & Scratches (r2 → r3; four changes 2.25, 2.5, 2.75, 3)
sheets run on the two 6000 × 4000 rows as well, from one list of sheets.
After each document closes it pumps
(at most 30 s) until the retained set is back within 1 MiB of its value before
the document was made, because AppKit frees the closed window and its document
on a later run loop turn; without that wait the next row was charged for the
previous one (about 290 MB of a closed 8-bit 24 MP document when the 16-bit
rows started). A wrapped
`canvas.onPreviewImageChange` (chaining the editor's own closure) timestamps
every delivery; a delivery whose width is the canvas's is EXACT, a narrower one
a proxy. Per sheet it prints three INFO lines — the opening preview, one change
(made only after the opening exact landed), and four changes 100 ms apart —
each with the first-visible ms (and its width) and the exact ms; the burst's
final exact is the last full-size delivery once none has arrived for
max(5 s, 1.5 × the single change's exact), and must come after the last change.
A 16-bit refusal ends a stage once a proxy has had 3 s to land and is reported
as `exact refused: <the sheet's status line>`. On the 16-bit rows it then
measures Apply twice, each on a fresh sheet of the same kind on the same
document with its Apply button (key equivalent Return) pressed at the opening
value (σ2; r5 T15; Reduce Noise r1; Dust & Scratches r2): `immediate` (as soon as the sheet is visible), then, after
`undoManager.removeAllActions()` and a pump (at most 30 s) until the retained
set is within 1 MiB of its value before the first Apply, `3 s` (3 s after the
sheet is visible, so the sheet's own preview work is running). Each waits up to
600 s for the document's precision handle to change, an alert to be recorded,
or the worker to be idle at least 3 s after the press, and prints
`apply (<delay>): committed in N ms` or `apply (<delay>) refused: <the recorded
[AutomationAlerts] line, or the status line>`; a refused Apply dismisses its
sheet if it is still presented and joins the missing list. A retained set that
does not settle is reported as an INFO line, not a failure: in this probe every
dismissed sheet stays in the editor's `presentedViewControllers` (the probe
process runs with the prohibited activation policy, and an explicit dismiss
raises), so the first Apply's sheet keeps its capture of the pre-Apply source
and the second Apply is measured with that source (192 MB at 24 MP) still
retained. It asserts only that every exact preview and every Apply lands (the
whole list is named in one FAIL line at the end). The 24 MP 16-bit Surface
Blur and Gaussian σ2 previews and both Applies land, the σ12 stages too, and
the 24 MP Reduce Noise and Dust & Scratches rows at both depths, so the run
prints no FAIL. The 8-bit Apply of the sheets with a job panel title (Surface
Blur, Reduce Noise, Dust & Scratches and Gaussian Blur;
`MeasuredSheet.title`, which no measured sheet leaves nil now) runs as a job,
watched by an `ApplyWatch` (a 10 ms timer in
`.common` and `.modalPanel`). Each titled 8-bit sheet first runs a cancel stage:
a fresh sheet's Apply with the watch clicking the job panel's Cancel at the
first occupied tick whose bar reads above 0, FAILing unless the click happened
and the document handle and revision, the undo stack, the dirty flag, the
alert count, the worker slot (free, no scratch) and the preview registry are
unchanged; it prints `apply (cancel): cancel→return N ms`. Its immediate Apply
then FAILs unless the job occupied the worker with the preview registry at its
pre-sheet value, no gap above 250 ms between two occupied ticks (the job gap)
and a progress bar above 0, and prints `apply (immediate): committed in N ms,
job gap G ms, Apply gap A ms (before the job B ms, after the job C ms),
progress max P`. Apply gap is the largest stall among the press, the ticks and
the action's return. B is the main thread's unticked stretch from the press to
the first tick inside the job's modal wait, and C the one from the last tick
to the action's return. B is about 270 ms at every filter and is the sheet's
own dismissal: `dismiss(self)` runs AppKit's closing animation synchronously
(`-[NSSheetMoveHelper closeSheet]`, sampled), as every sheet's Cancel does. C
is the publication: the job also writes the result and prepares
its first projection on the worker, so C fell from about 240 ms to about
12 ms, while the Apply gap, which was B before and after, did not move. B and
C are printed and deliberately not bounded: the probe asserts no duration for
either stretch. The 8-bit 6000 × 4000 and 16-bit 6000 × 4000 Surface Blur rows end
with the busy-slot stage, after `removeAllActions()` and the retained-set
settle: (i) a `WorkerHolder` released 0.5 s after Apply is pressed must be
waited for (committed, the holder taken, no alert, ≥ 400 ms), printing
`apply (slot held 0.5 s): committed in N ms`; (ii) a holder kept past
`BackgroundWork.anonymousWaitLimit` must keep the sheet open (its window still
visible: a dismissed sheet stays in `presentedViewControllers` here) with
`BackgroundWork.busyMessage` on `tone.preview.status`, the document, history,
alerts and first slider unchanged, and a second Apply after
`holder.releaseNow()` must commit and dismiss it, printing `apply (slot held
past limit): kept open after N ms; re-Apply committed in M ms`. The 16-bit row
adds case (i) on the Levels sheet (Black 0.1), which commits through
`PrecisionAdjustmentSession.commit(op:)`. Latency
bounds are judged in `designs/phase-9f-selected-tone-status.md`, so the same
file measures the code before and after a change. A run takes about 15 minutes
(454 s measured with the Surface Blur and Gaussian Blur sheets alone, 670 s
once Reduce Noise and Dust & Scratches joined them, 891 s with the 8-bit job
stages), which is why `verify.py`'s run bound is
1337 s (1.5 × 891 s).

"sky replacement sheet" (`ui_sky_replacement.swift`) opens Edit ▸ Sky
Replacement… on the phase-6 `sky.jpg` at 8 and 16 bits with the sky model and the
sky library from the fixture store (`$DR_JOBS_PROBE_FIXTURES/models`, filled by
`scripts/fill_model_fixtures.py --release-dir ~/devel/doctored-fixtures/models-release`;
SKIP when either is not ready there). It asserts the ≤1024 px proxy and that the
preview's sky is no larger than f = min(1, 4·max(proxyW/sw, proxyH/sh)) allows, that
Lighting Adjustment and a second library sky re-render with the document untouched,
that Decontaminate Edge is on by default, and that OK is one "Sky Replacement"
step adding one active layer, one recorded `sky_replacement` step naming the sky with
`decontaminate` true and a note naming the model, and one undo; at 8 bits it keeps
Output To's default New Layers (the active Sky Replacement Group, five new entries,
"Foreground Edge" first), at 16 bits it picks Duplicate Layer in the popup.
"model selection entries" lists the catalogue's ten entries in order, each Not downloaded
with its licence link, tier and training-data note.

"settings card height" (`model_store.swift`) builds the Settings card over the real
catalogue in a scratch store, without running it modally, and prints its frame and each
tab's content height: at most 700 pt tall and within the main screen's visible frame, the
same size with the Models tab selected, the model rows scrolling inside `models-list` (its
document taller than its viewport) and every model's Download button still in the pane.
"models pane ready size" (`model_store.swift`) lays out a ready Core ML model (package, compiled
`.mlmodelc` and `verified.json`) and a `.partial` download by hand in a scratch store: the ready
row's size line is the bytes the case wrote followed by " on disk", the partial row's stays
"<partial> of <download>".

"raw float alpha rule" (`raw_float_alpha.swift`) calls `RetainedRawDecoder.normalizeOpaqueAlpha`
on one-pixel buffers: alphas within half a 16-bit code of 1 become exactly 1 with RGB unchanged,
and code 65534, 0.9999, 0.5, 0, 1.5, NaN and infinity refuse.

"indexed background key" (`indexed_background_key.swift`) converts an 8 × 4 PNG of four
colour columns to an Exact Indexed Color document with no transparent index and runs
`PrecisionGeometryWork.compute` on `.move`, `.transform` and `.crop` past the canvas with a red
background swatch: every uncovered pixel holds the entry nearest the swatch (the case's own
least-squares oracle), and the twin with no swatch holds white's nearest, which differs; no event
and no window.

"status mode segment" (`status_mode.swift`) converts a 64 × 48 fixture through the MCP
tools into RGB at 8, 16 and 32 bits, Grayscale at 8, 16 and 32, Duotone, Lab and CMYK at 8
and 16, Indexed and Bitmap, confirms each landed through the descriptor, and requires the
status bar's `status-mode` segment to start with the row's literal mode and sample format
("Lab · 16-bit · ", "Bitmap · 1-bit · ") followed by a profile name, and Image ▸ Mode's
checked Bits/Channel item to be the row's literal depth (none on float RGB, Indexed and
Bitmap). Every row is checked before the case fails.

"float spot view" and "hdr spot view" (`ui_float_spot.swift`, #1) add a spot channel (black,
Solidity 100, the left half) to a uniform float window with its Channels panel loaded. The first,
on 0.25, reads the canvas's overprint base: the ink half 0, the other half the sRGB code of 0.25
within 1, then of 0.5 at +1 stop and of Reinhard's (white 4) 0.5 under that candidate map, the
exposure reporting displayed and `viewState.viewImage` being the base. The second, on 4.0 at
headroom 4, sees HDR Preview displayed without the channel, standing down with it (an RGBA8 layer,
the base 255 beside the ink), engaging again when the spot row's eye is off and standing down when
it is on, and engaging after one undo of the add.

All fixtures, settings, Actions, logs and the copy of the probe binary that runs
are confined to a private temporary directory, retained on failure or with `--keep`.
The linked probe is built through `scripts/probe_build.py` and cached under
`build/probe-cache`, with the module cache in `build/probe-module-cache`, so a
second run on unchanged sources compiles nothing. The probe also uses a private
named pasteboard (`DR_PASTEBOARD_NAME`, which the probe refuses to run without and
releases at exit). Agent server
startup is disabled and credentials are replaced with `ANTHROPIC_API_KEY=dummy`.
Use `scripts/verify_phase4.py` for the separate built-app HTTP MCP/resource corpus.
