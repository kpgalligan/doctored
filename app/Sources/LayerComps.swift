import AppKit

// `editingLayerComps(_:)`'s typed branch is the core's `{"op":"layer_comp", …}` edit and its byte
// branch `layerCompsEditHandle`; nil is the core's exact no-op. Every RULE belongs to the CORE
// (`core/src/doc_comps.rs`) — what is captured, what makes an entry stale, which locks apply, the
// 64-comp limit, the duplicate-name refusal — so this side carries values and shows the core's
// sentence verbatim. `LayerCompApplyReport.applyReport(for:changed:)` is the shared pure report
// builder, and BOTH apply paths read the comp BEFORE the edit so the names they report are the ones
// the list showed.
// Applying a comp hides every non-group layer it has no entry for (`unrecorded`); the core
// decides which, and this side only names them in the report.
//
// A document holding layer comps is native version 44 in the typed format and 43 in the byte
// format; `isPrecisionNative` admits 44 and refuses 43.

/// LAYER COMPS — named recordings of per-layer visibility, position and
/// appearance (Phase 12D-1).
///
/// A comp is deliberately NOT a snapshot. `NamedSnapshots` holds the whole
/// editing state — pixels, selection, edit target — and restoring one puts
/// every byte back; a comp holds a table of small per-layer facts, bound by
/// the core's stable layer identity, and applying one is an ordinary
/// undoable property edit. Every RULE is the core's (`core/src/doc_comps.rs`):
/// which attributes a comp records, what makes an entry stale, which locks
/// apply, what a duplicate name earns and when an op is a no-op. This file
/// carries the values and the depth branch and decides nothing.
///
/// `RasterDocument.editingLayerComps(_:)` is the ONE writer the Layer Comps
/// window, the menu items and the six MCP tools share.

/// Which attributes a comp records. The core's three flags, in the core's
/// spelling.
struct LayerCompCaptures: Equatable, Codable {
    var visibility: Bool
    var position: Bool
    var appearance: Bool

    /// All three — what a save with no flags means, the core's own default.
    static let all = LayerCompCaptures(visibility: true, position: true, appearance: true)

    /// The compact `V P A` badge the window's list draws; a flag that is off
    /// becomes a dash, so the column is the same width on every row.
    var glyphs: String {
        "\(visibility ? "V" : "–") \(position ? "P" : "–") \(appearance ? "A" : "–")"
    }

    /// The request keys, for the one writer.
    var arguments: [String: Any] {
        ["visibility": visibility, "position": position, "appearance": appearance]
    }
}

/// A recorded canvas offset, in pixels.
struct LayerCompOffset: Equatable {
    let x: Int
    let y: Int
}

/// A recorded APPEARANCE: the three values a layer's compositing reads that
/// are not its pixels, as the core reports them.
struct LayerCompAppearance: Equatable {
    /// Layer opacity, 0…1, widened from the core's f32.
    let opacity: Double
    let blend: DrBlendMode
    /// The layer style as the core's canonical JSON text, or nil when the
    /// layer carried none when the appearance was recorded.
    let style: String?
}

/// One layer's row in a comp, as the core reports it.
struct LayerCompEntry: Equatable {
    /// The layer's CURRENT index, or nil when the entry is stale.
    let layer: Int?
    /// The core's stable layer id; 0 for a stale entry.
    let id: UInt64
    /// The layer's name — the current one for a bound entry, the last known
    /// one for a stale entry, which is the only handle it has.
    let name: String
    /// The layer this entry names is gone.
    let stale: Bool
    /// The recorded POSITION is one a lock forbids, so applying the comp
    /// skips it. A real answer at both depths: a typed layer carries locks
    /// too (12E-1b).
    let locked: Bool
    /// The recorded visibility; nil when the comp does not capture it.
    let visible: Bool?
    /// The recorded offset; nil when the comp does not capture position or
    /// the entry is not a raster layer.
    let offset: LayerCompOffset?
    /// The recorded appearance; nil when the comp does not capture it.
    let appearance: LayerCompAppearance?
}

/// One layer the comp has no entry for and would HIDE on apply (it is not a
/// group), as the core reports it.
struct LayerCompUnrecorded: Equatable {
    let layer: Int
    let id: UInt64
    let name: String
}

/// One named comp, as the core reports it.
struct LayerComp: Equatable {
    let index: Int
    let name: String
    let comment: String
    let captures: LayerCompCaptures
    let entries: [LayerCompEntry]
    let staleCount: Int
    let lockedCount: Int
    /// The layers applying this comp hides because it has no entry for them.
    let unrecorded: [LayerCompUnrecorded]
}

/// What applying a comp did. Built from the comp read BEFORE the edit, so the
/// names it reports are the names the user saw in the list.
struct LayerCompApplyReport: Equatable {
    /// The layer indices the comp's bound entries name.
    let applied: [Int]
    /// Stale entries, by name.
    let stale: [String]
    /// Bound entries whose recorded position a lock forbade, by name.
    let locked: [String]
    /// The layers the comp has no entry for, hidden by the apply, by name.
    let hidden: [String]
    /// Whether an undo step exists at all.
    let changed: Bool

    /// The ONE report builder the window, the menu path and the MCP handler
    /// share. Pure: it reads the comp and nothing else.
    static func applyReport(for comp: LayerComp, changed: Bool) -> LayerCompApplyReport {
        LayerCompApplyReport(
            applied: comp.entries.compactMap { $0.stale ? nil : $0.layer },
            stale: comp.entries.filter(\.stale).map(\.name),
            locked: comp.entries.filter(\.locked).map(\.name),
            hidden: comp.unrecorded.map(\.name),
            changed: changed)
    }

    /// The MCP reply's fields, and the window's status sentence's source.
    var fields: [String: Any] {
        [
            "changed": changed, "applied": applied, "stale": stale, "locked": locked,
            "hidden_unrecorded": hidden,
        ]
    }

    /// One sentence naming what could not be honoured, or nil when
    /// everything was.
    var caveat: String? {
        var parts: [String] = []
        if !stale.isEmpty {
            parts.append("\(stale.count) stale (\(stale.joined(separator: ", ")))")
        }
        if !locked.isEmpty {
            parts.append("\(locked.count) position-locked (\(locked.joined(separator: ", ")))")
        }
        if !hidden.isEmpty {
            let names = hidden.joined(separator: ", ")
            parts.append("\(hidden.count) hidden, not in the comp (\(names))")
        }
        return parts.isEmpty ? nil : parts.joined(separator: "; ")
    }
}

extension LayerComp {
    /// Decodes the core's report form. Returns nil for a row that is not one,
    /// which a caller turns into a thrown error naming the read.
    init?(json: [String: Any]) {
        guard let index = json["index"] as? Int, let name = json["name"] as? String,
              let captures = json["captures"] as? [String: Any],
              let rows = json["entries"] as? [[String: Any]]
        else { return nil }
        self.index = index
        self.name = name
        comment = json["comment"] as? String ?? ""
        self.captures = LayerCompCaptures(
            visibility: captures["visibility"] as? Bool ?? false,
            position: captures["position"] as? Bool ?? false,
            appearance: captures["appearance"] as? Bool ?? false)
        entries = rows.map { row in
            let offset = (row["offset"] as? [NSNumber]).flatMap { pair in
                pair.count == 2 ? LayerCompOffset(x: pair[0].intValue, y: pair[1].intValue) : nil
            }
            // Appearance is recorded as a whole, so `opacity` stands for it.
            let appearance = (row["opacity"] as? NSNumber).map { opacity in
                LayerCompAppearance(
                    opacity: opacity.doubleValue,
                    blend: DrBlendMode(rawValue: (row["blend"] as? NSNumber)?.uint32Value ?? 0),
                    style: row["style"] as? String)
            }
            return LayerCompEntry(
                layer: row["layer"] as? Int,
                id: (row["id"] as? NSNumber)?.uint64Value ?? 0,
                name: row["name"] as? String ?? "",
                stale: row["stale"] as? Bool ?? false,
                locked: row["locked"] as? Bool ?? false,
                visible: row["visible"] as? Bool,
                offset: offset,
                appearance: appearance)
        }
        staleCount = json["stale_count"] as? Int ?? entries.filter(\.stale).count
        lockedCount = json["locked_count"] as? Int ?? entries.filter(\.locked).count
        unrecorded = (json["unrecorded"] as? [[String: Any]] ?? []).compactMap { row in
            guard let layer = row["layer"] as? Int else { return nil }
            return LayerCompUnrecorded(
                layer: layer, id: (row["id"] as? NSNumber)?.uint64Value ?? 0,
                name: row["name"] as? String ?? "")
        }
    }

    /// The MCP row — the core's report form, re-emitted field for field,
    /// with each entry's recorded state in get_document's vocabulary: a key
    /// is present iff the comp recorded it, `blend_mode` is the display name,
    /// `style` the object set_layer_style takes (null for an unstyled layer)
    /// and `opacity` the core's f32 rounded to four decimals.
    var json: [String: Any] {
        [
            "index": index, "name": name, "comment": comment,
            "captures": captures.arguments,
            "stale_count": staleCount, "locked_count": lockedCount,
            "entries": entries.map(Self.entryJSON),
            "unrecorded": unrecorded.map { ["layer": $0.layer, "id": $0.id, "name": $0.name] },
        ]
    }

    /// One entry's MCP row.
    private static func entryJSON(_ entry: LayerCompEntry) -> [String: Any] {
        var row: [String: Any] = [
            "layer": entry.layer.map { $0 as Any } ?? NSNull(), "id": entry.id,
            "name": entry.name, "stale": entry.stale, "locked": entry.locked,
        ]
        if let visible = entry.visible { row["visible"] = visible }
        if let offset = entry.offset { row["offset"] = [offset.x, offset.y] }
        if let appearance = entry.appearance {
            // Rounded to strip the f32 widening, then spelled through
            // `shortNumbers` so JSONSerialization prints `0.3`, not
            // `0.29999999999999999`.
            let opacity = (appearance.opacity * 10000).rounded() / 10000
            row["opacity"] = AgentServer.shortNumbers(opacity)
            row["blend_mode"] = DrBlendMode.displayName(for: appearance.blend)
            row["style"] = appearance.style.flatMap(AgentServer.layerStyleObject) ?? NSNull()
        }
        return row
    }
}

extension RasterDocument {
    /// Every layer comp in list order, at whichever depth this facade owns.
    func layerComps() throws -> [LayerComp] {
        let text = try layerCompsJSON()
        guard let data = text.data(using: .utf8),
              let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let rows = object["comps"] as? [[String: Any]]
        else { throw DoctoredCoreError(message: "The layer comps could not be read.") }
        return rows.compactMap(LayerComp.init(json:))
    }

    /// The ONE writer for every comp mutation — the window, the menu items
    /// and the six MCP tools all arrive here. nil is the core's exact no-op
    /// (an update that changes nothing, an apply whose every field already
    /// holds); a refusal throws with the core's own sentence.
    ///
    /// The typed branch is the core's `{"op":"layer_comp", …}` edit, which is
    /// the same request the byte export takes, so there is one schema.
    func editingLayerComps(_ request: [String: Any]) throws -> RasterDocument? {
        var fields = request
        fields["op"] = "layer_comp"
        if isHighDepth {
            return try performingPrecisionPixelEdit(.init(fields))
        }
        guard JSONSerialization.isValidJSONObject(fields),
              let data = try? JSONSerialization.data(withJSONObject: fields, options: .sortedKeys),
              let text = String(data: data, encoding: .utf8)
        else { throw DoctoredCoreError(message: "The layer comp request could not be built.") }
        return try layerCompsEditHandle(text)
    }

    /// `self` is the document after the host re-rendered or refitted
    /// `indices` (text, shape) following a whole-canvas geometry
    /// op; `core` is the document the core op returned, whose comps the core
    /// already remapped by its own pixel rule. Each index whose offset differs
    /// between the two gets one `shift` comp request (dx = self − core), so
    /// that layer's recorded offsets follow the host's placement and applying
    /// a comp after the op does not move it. Bookkeeping inside the geometry
    /// edit (no undo step of its own); a refusal throws with the edit.
    func followingHostPlacement(of indices: [Int], placedBy core: RasterDocument) throws
        -> RasterDocument {
        // A CMYK document's comps come only from a PSD and record visibility,
        // never a position, and its table has no `layer_comp` family to shift
        // one, so a re-rendered artwork layer has no offset to follow (#1).
        guard !isCmyk else { return self }
        var result = self
        for index in indices {
            guard let placed = layerInfo(index), let computed = core.layerInfo(index) else {
                continue
            }
            let dx = placed.offsetX - computed.offsetX
            let dy = placed.offsetY - computed.offsetY
            guard dx != 0 || dy != 0 else { continue }
            if let shifted = try result.editingLayerComps(
                ["action": "shift", "layer": index, "dx": dx, "dy": dy]) {
                result = shifted
            }
        }
        return result
    }
}
