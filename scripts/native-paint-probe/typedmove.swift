import AppKit

// Phase 12E-2 (P7-27), carried item 4: `RasterDocument.moveLayers`' TYPED
// branch is ALL OR NOTHING — one member that cannot move refuses the WHOLE
// call and the handle it was given is left exactly as it was. That is the
// byte op's rule (`dr_doc_move_layers`) and what the branch's own doc comment
// promises, and a PARTIAL move (the rasters left behind while the linked
// adjustment masks moved) is the failure this case exists to catch.
//
// It is measured HERE, in process, because the property cannot be provoked
// from outside: `AgentServer.moveLayers` pre-checks every position lock by
// name (`rejectLockedMove`) and caps dx/dy at ±1,000,000, so no MCP call can
// present a set with exactly one failing member. Called directly — the way
// the Move tool's drag and the arrow-key nudge call it — such a set CAN be
// built, and that is what this case builds.

/// typed-move-all-or-nothing.
///
/// A 16-bit document holding a raster and an adjustment with a LINKED mask.
/// The raster is first parked near the core's i32 coordinate limit, so one
/// further move refuses the RASTER member (its extent would leave i32) while
/// the adjustment's mask, still at x = 0, would move perfectly well. The set
/// naming both is then refused whole, and nothing moves.
func typedMoveAllOrNothing() throws -> [String: Any] {
    let (document, _, _) = try openDocument()
    defer { document.close(); pump() }
    try require(document.doc?.isHighDepth == true, "Owned fixture is not a 16-bit document")
    let created = try call("add_adjustment_layer", ["op": "invert", "name": "Fx"],
                           document: document)
    try require(!(try responseIsError(created)), "add_adjustment_layer refused: \(created)")
    guard let staged = document.doc,
          let fx = (0..<staged.layerCount).first(where: { staged.layerInfo($0)?.name == "Fx" })
    else { throw DoctoredCoreError(message: "Owned adjustment layer missing") }
    let masked = try call("add_layer_mask", ["layer": fx, "kind": "reveal_all"],
                          document: document)
    try require(!(try responseIsError(masked)), "add_layer_mask refused: \(masked)")
    guard let base = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    let raster = 0
    try require(base.layerIsAdjustment(fx) && !base.layerIsAdjustment(raster),
        "Owned fixture is not one raster plus one adjustment")
    try require(base.layerMaskProperties(fx)?.linked == true,
        "The adjustment's mask is not linked, so the move would carry nothing")

    // Park the raster far to the right. The core's coordinate limit is i32
    // (`precision_geometry_plan`), so 96 px of raster at x = 2,000,000,000
    // leaves i32 under a further 148,000,000 while a mask still at x = 0 does
    // not: the two halves of ONE move then have different answers.
    let park = 2_000_000_000
    let delta = 148_000_000
    guard let parked = try base.moveLayers([raster], dx: park, dy: 0) else {
        throw DoctoredCoreError(message: "Owned far move was refused")
    }
    try require(parked.layerInfo(raster)?.offsetX == park, "The raster did not park")

    // The asymmetry is MEASURED here rather than assumed: the raster member
    // alone is refused, the adjustment member alone moves.
    try require((try? parked.moveLayers([raster], dx: delta, dy: 0)) == nil,
        "The parked raster was not refused, so this case proves nothing")
    guard let maskOnly = try parked.moveLayers([fx], dx: delta, dy: 0),
          maskOnly.layerMaskProperties(fx)?.offsetX == delta else {
        throw DoctoredCoreError(message: "The linked mask member did not move on its own")
    }

    // …so the SET has exactly one failing member — and the whole call is
    // refused, with no partial publication.
    try require((try? parked.moveLayers([raster, fx], dx: delta, dy: 0)) == nil,
        "A typed move published a PARTIAL result: the raster member was refused and the "
            + "linked adjustment mask moved anyway")

    // A position-locked adjustment is the other way a member cannot move, and
    // it is refused before ANYTHING moves — the branch's own pre-check.
    guard let locked = try? base.withLockFlags(fx, .position) else {
        throw DoctoredCoreError(message: "Owned position lock was refused")
    }
    try require((try? locked.moveLayers([raster, fx], dx: 3, dy: 2)) == nil,
        "A position-locked adjustment did not refuse the whole set")

    // Purity, at both refusals: the handle the calls were given still reads
    // exactly what it read before them.
    try require(parked.layerInfo(raster)?.offsetX == park
        && parked.layerMaskProperties(fx)?.offsetX == 0,
        "A refused typed move changed the document it was given")
    return ["case": "typed-move-all-or-nothing",
            "park": park, "delta": delta,
            "raster_member_alone": "refused",
            "mask_member_alone_offset_x": delta,
            "set_with_one_failing_member": "refused whole",
            "position_locked_member": "refused whole",
            "input_unchanged": true]
}
