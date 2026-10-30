import AppKit

// Synchronization prepares all selected results and projections before publishing, rechecks every
// captured target and records one Undo per changed document. As-shot settings resolve against each
// target's captured camera state.
//
// Global synchronization preserves target-local IDs/coverage and camera baseline references.
// Masking (`locals`; `RawLocalSync.swift`) instead REPLACES each target's locals with the
// source's, in order: drawn coverage resampled in normalised coordinates, feather scaled to the
// target's size, Subject/People/Sky detected again on the target. A generated local that finds
// nothing (or whose model is not installed) is skipped and named in `Result.note`.
// Explicit local copy transfers exposure offset and white-balance intent only, retaining target
// mask, name, density, feather and enabled state.

struct RawSettingsTarget {
    let document: ImageDocument
    let layer: Int
}

/// Every requested result and projection is ready before any document changes.
/// One immutable edit publishes all target layers in each document. Separate
/// documents keep their own independent Undo histories.
enum RawSettingsSynchronization {
    static let maximumTargets = 32
    private struct CapturedDocument {
        let document: ImageDocument
        let capture: ImageSourceCapture
        var layers: [Int]
    }
    struct Result {
        let changedDocuments: Int
        let changedLayers: Int
        /// `"<document> — <local name> (<kind>): <message>"` per generated local Masking skipped.
        let skipped: [String]
        var note: String?
        var json: [String: Any] {
            var json: [String: Any] = ["changed_documents": changedDocuments,
                                       "changed_layers": changedLayers, "skipped": skipped]
            if let note { json["note"] = note }
            return json
        }
    }

    static func settings(from source: RawSettingsTarget, fields: [RawSettingsField]) throws -> RawDevelopSettings {
        guard let document = source.document.doc,
              let info = try document.retainedSourceInfo(source.layer) else {
            throw DoctoredCoreError(message: "Choose a retained RAW source layer.")
        }
        let descriptor = try RawSourceDescriptor.read(info)
        guard let output = RawOutputSpace(policy: descriptor.recipe.outputSpace) else {
            throw DoctoredCoreError(message: "The source RAW colour encoding is unavailable.")
        }
        return try RawSettingsPatch.capture(values: descriptor.recipe.values,
            capabilities: descriptor.recipe.capabilities, outputSpace: output, fields: fields,
            recipe: descriptor.recipe)
    }

    static func apply(settings: RawDevelopSettings, locals: RawLocalSyncPayload? = nil,
                      targets: [RawSettingsTarget], title: String = "Apply RAW Settings",
                      validateSource: (() throws -> Void)? = nil) throws -> Result {
        // Masking alone carries no global control.
        let settings = locals != nil && settings.jsonObject().isEmpty
            ? settings : try RawSettingsPatch.validated(settings)
        guard !targets.isEmpty, targets.count <= maximumTargets else {
            throw DoctoredCoreError(message: "Choose 1–32 retained RAW target layers.")
        }
        var captured: [CapturedDocument] = []
        for target in targets {
            guard NSDocumentController.shared.documents.contains(where: { $0 === target.document }) else {
                throw DoctoredCoreError(message: "A RAW target document has closed.")
            }
            if let index = captured.firstIndex(where: { $0.document === target.document }) {
                guard !captured[index].layers.contains(target.layer) else {
                    throw DoctoredCoreError(message: "A RAW synchronization target was listed more than once.")
                }
                captured[index].layers.append(target.layer)
            } else {
                captured.append(CapturedDocument(document: target.document,
                    capture: try ImageSourceCapture(target.document), layers: [target.layer]))
            }
        }
        func requireCurrent() throws {
            try validateSource?()
            for item in captured {
                guard NSDocumentController.shared.documents.contains(where: { $0 === item.document }) else {
                    throw DoctoredCoreError(message: "A RAW target document closed. No settings were applied.")
                }
                try item.capture.requireCurrent(item.document)
            }
        }
        // Validate all layer/lock/recipe/control contracts before the first
        // decode. File-specific decoder support is checked during preparation,
        // still before publication of any target.
        for item in captured {
            for layer in item.layers {
                let info = try LinkedImageWork.requireSource(item.capture.source, layer: layer, editing: true)
                let descriptor = try RawSourceDescriptor.read(info)
                let desired = try descriptor.recipe.updating(settings)
                try desired.validate()
                if locals != nil || descriptor.recipe.enhancements != nil
                    || item.capture.source.hasSourceAttachment(layer) {
                    _ = try RawLocalWork.state(item.capture.source, layer: layer)
                }
                guard desired == descriptor.recipe || descriptor.compatible else {
                    throw DoctoredCoreError(message: "A RAW target's captured decoder is unavailable. No settings were applied.")
                }
            }
        }
        try requireCurrent()
        var results: [(Int, RasterDocument, [Int])] = []
        var skipped: [String] = []
        for (index, item) in captured.enumerated() {
            var source = item.capture.source
            var changed: [Int] = []
            for layer in item.layers.sorted() {
                let result: RasterDocument?
                if let locals {
                    result = try prepare(locals, source: source, layer: layer, settings: settings,
                                         document: item.document, skipped: &skipped)
                } else {
                    result = try RawSourceWork.prepare(source: source, layer: layer, settings: settings)
                }
                if let result, result !== source {
                    source = result
                    changed.append(layer)
                }
                try requireCurrent()
            }
            if !changed.isEmpty { results.append((index, source, changed)) }
        }
        var prepared: [(Int, PreparedDocumentEdit, [Int])] = []
        for (index, result, layers) in results {
            let ready = try BackgroundWork.wait(try WorkerTask { PreparedDocumentEdit(result) },
                                                title: "Preparing RAW Settings")
            if let error = ready.error { throw DoctoredCoreError(message: error) }
            prepared.append((index, ready, layers))
            try requireCurrent()
        }
        // Computed once for the whole call, and only when something publishes: while a recording
        // runs it writes the drawn masks the step names.
        let localArguments = prepared.isEmpty ? nil : try locals?.recordedArguments()
        try requireCurrent()
        let raw = settings.jsonObject()
        let record = { (layers: [Int]) -> [ActionStep] in
            var arguments: [String: Any] = ["layers": layers]
            if locals == nil || !raw.isEmpty { arguments["raw"] = raw }
            if let localArguments { arguments.merge(localArguments) { $1 } }
            return .step("apply_raw_settings", arguments, note: title)
        }
        // Every target's applyEdit refusals are answered before ANY target
        // publishes and before any undo group opens: a refusal throws
        // in band and nothing changes anywhere.
        do {
            for (index, ready, layers) in prepared {
                let item = captured[index]
                item.document.preparedEdit = ready
                try item.document.preflightPublication(ready.document, record: record(layers))
            }
        } catch {
            for (index, ready, _) in prepared
            where captured[index].document.preparedEdit?.document === ready.document {
                captured[index].document.preparedEdit = nil
            }
            throw error
        }
        // Synchronous notifications from one publication must not let UI/MCP
        // edits alter another prepared target. Each applyEdit owns its normal
        // central publication guard; the other documents stay guarded here.
        for item in captured { item.document.isPublishingEditingState = true }
        defer {
            for item in captured {
                item.document.isPublishingEditingState = false
                item.document.preparedEdit = nil
            }
        }
        var changedDocuments = 0
        var changedLayers = 0
        for (index, ready, layers) in prepared {
            let item = captured[index]
            item.document.preparedEdit = ready
            item.document.isPublishingEditingState = false
            withUndoGroup(item.document.undoManager) {
                item.document.applyEdit(title, record: record(layers)) { current in
                    current === item.capture.source ? ready.document : nil
                }
            }
            item.document.isPublishingEditingState = true
            if item.document.doc === ready.document {
                changedDocuments += 1
                changedLayers += layers.count
            }
        }
        let note = skipped.isEmpty ? nil : "Skipped \(skipped.count) local(s): "
            + skipped.map { $0.hasSuffix(".") ? String($0.dropLast()) : $0 }.joined(separator: "; ") + "."
        return Result(changedDocuments: changedDocuments, changedLayers: changedLayers,
                      skipped: skipped, note: note)
    }

    /// One target layer's Masking development: the payload's operations against the layer's
    /// current state. A generated local that refuses is dropped, named in `skipped`, and the
    /// rest prepared again. nil when the layer would not change.
    private static func prepare(_ locals: RawLocalSyncPayload, source: RasterDocument, layer: Int,
                                settings: RawDevelopSettings, document: ImageDocument,
                                skipped: inout [String]) throws -> RasterDocument? {
        let state = try RawLocalWork.state(source, layer: layer)
        let replacing = locals.operations(replacing: state)
        let provided = replacing.provided
        var operations = replacing.operations
        while true {
            do {
                return try RawLocalWork.prepare(source: source, layer: layer, settings: settings,
                                                operations: operations, provided: provided)
            } catch let refusal as RawLocalGenerationRefusal {
                guard let kept = RawLocalGenerated.dropping(refusal, from: operations, previewed: 0) else {
                    throw refusal
                }
                let object = operations[refusal.operation]
                let name = object["name"] as? String ?? object["id"] as? String ?? "Local"
                let op = (object["mask"] as? [String: Any])?["op"] as? String ?? ""
                let kind = RawLocalGeneratedKind(rawValue: op)?.title ?? op
                skipped.append("\(document.displayName ?? "Untitled") — \(name) (\(kind)): \(refusal.message)")
                operations = kept
            }
        }
    }

    static func availableTargets() -> [RawSettingsTarget] {
        let documents: [ImageDocument] = NSDocumentController.shared.documents.compactMap { $0 as? ImageDocument }
        return documents.flatMap { document in
            guard let source = document.doc else { return [RawSettingsTarget]() }
            return (0..<source.layerCount).filter { source.hasRetainedRawSource($0) }
                .map { RawSettingsTarget(document: document, layer: $0) }
        }
    }
}
