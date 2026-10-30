# Assisted edge annotation task — v1

This card accompanies an isolated annotation workspace containing anonymous
source photographs and starting masks. It contains no reference image or score
information. No annotation has been collected by preparing this card.

Before starting, record your human participant ID and whether you have previously
seen reference cutouts/masks, candidate results or scores for the photographs.
If you have, record that exposure and do not annotate that case for this protocol.

Refine the intended subject's edge using only the observed photograph and
starting mask. Mark confidently intended foreground/background; use Unknown to
release a mark when uncertain. A foreground mark means fully included in the
refinement mask; it does not prove physical opacity.

Protect any area whose current pixels and mask must stay exact. Restrict alpha
and color changes independently where useful. Both initially allow changes
everywhere; protection initially covers nothing. A feathered region authorizes
fractional changes inside its boundary. Marks that conflict with protection or
insufficient alpha-write coverage are rejected without changing the last valid
state; revise the relevant region or protection explicitly if needed.

You have **5 minutes and 20 editing gestures for each case**, with one attempt.
The timer starts when the source and prior first appear. Use brush diameters
3, 9, 27 or 81 native pixels. Region polygons allow up to 32 vertices and inward
feather widths 0, 4, 16 or 64 native pixels. A stroke, polygon, region replacement,
Undo, Redo or rejected edit each uses one gesture. Zoom/pan and switching among
source, prior and your guidance overlays use time but no gesture.

Inspect native pixels as needed. Do not consult external images, reference
masks, model outputs or scores, and do not import masks or reuse another case's
guidance. No candidate preview is available. Submit once; submitted guidance
locks permanently. Unmarked pixels and annotation mistakes remain in the
evaluation. When time or gestures expire, the tool keeps the last valid state.

This v1 collection tool offers the three label brushes only: there is no
protect tool, no alpha-write or RGB-write region tool, no polygon and no
feather control, so the paragraphs above about protection and independent write
regions describe the protocol rather than anything you can draw. Your replay
declares the constant write planes that follow from that. Record your prior
exposure before you begin: under the gate v2 amendment, a case you have already
inspected — its source, its reference cutout, a comparison panel or any score —
is annotated unblinded, flagged `annotator_exposed: true` and stays in the
reported population; it is not dropped and it is not a reason to skip the case.

The tool must record your actual events, elapsed times and displayed input
hashes. Nobody may create a replay or source-only attestation on your behalf.
This task requires the frozen annotation tool and prepared source-only workspace
before it can be performed; this card alone does not supply them.
