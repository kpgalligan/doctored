import AppKit

// Migration shares the complete local peak plan and configured parent/child cancellation owner,
// with an object-scoped owner-close cancellation observer.
//
// A registered EXIF/crop/pad change remaps canonical u16 masks; an unregistered legacy extent needs
// an explicit reviewed inverse affine map. Existing optical and vignette references compose with
// that map independently. Acceptance publishes the exact reviewed source/masks through central
// Undo; saved rendering remains usable without a compatible decoder.

/// A migration is a reviewed new interpretation, never a native-open side effect.
/// The candidate owns the exact pixels that will be committed after review.
final class RawDecoderMigration {
    let capture: ImageSourceCapture
    let layer: Int
    let previous: RawSourceDescriptor
    let descriptor: RawSourceDescriptor
    let asset: RasterDocument
    let prepared: RasterDocument
    let geometry: RawSourceMap
    let beforeMasks: [String]
    let afterMasks: [String]
    let changes: [String]
    private let payload: PrecisionEditPayload

    private init(capture: ImageSourceCapture, layer: Int, previous: RawSourceDescriptor,
                 descriptor: RawSourceDescriptor, asset: RasterDocument,
                 prepared: RasterDocument, geometry: RawSourceMap,
                 beforeMasks: [String], afterMasks: [String], changes: [String],
                 payload: PrecisionEditPayload) {
        self.capture = capture
        self.layer = layer
        self.previous = previous
        self.descriptor = descriptor
        self.asset = asset
        self.prepared = prepared
        self.geometry = geometry
        self.beforeMasks = beforeMasks
        self.afterMasks = afterMasks
        self.changes = changes
        self.payload = payload
    }

    static func proposedFrame(in document: ImageDocument, layer: Int) throws -> RawDecoderFrame {
        let capture = try ImageSourceCapture(document)
        let info = try LinkedImageWork.requireSource(capture.source, layer: layer, editing: true)
        let previous = try RawSourceDescriptor.read(info)
        let snapshot = try RawSourceSnapshot.restore(capture.source, layer: layer, descriptor: previous)
        let current = try RetainedRawDecoder.capture(url: snapshot.url,
            filename: previous.original_filename, sha256: previous.content_sha256)
        let frame = try RetainedRawDecoder.decoderFrame(url: snapshot.url, descriptor: current)
        try capture.requireCurrent(document)
        return frame
    }

    static func prepare(in document: ImageDocument, layer: Int,
                        geometry: RawSourceMap? = nil,
                        control supplied: RasterJobControl? = nil) throws -> RawDecoderMigration {
        let capture = try ImageSourceCapture(document)
        let source = capture.source
        let info = try LinkedImageWork.requireSource(source, layer: layer, editing: true)
        let previous = try RawSourceDescriptor.read(info)
        guard previous.schema_version == 1,
              previous.decoder.identifier == RawSourceDescriptor.decoderIdentifier,
              previous.recipe.hasSupportedOutputPolicy else {
            throw DoctoredCoreError(message: "This saved recipe cannot be migrated by this build. Keep its saved rendering.")
        }
        let payload = try PrecisionEditPayload.admitted(bytes: 2 * 1024 * 1024)
        let localState = try RawLocalWork.state(source, layer: layer)
        let encoded = try PrecisionDocumentDescriptor.integer(info["raw"] as? [String: Any] ?? [:], "encoded_bytes")
        let width = Int(try PrecisionDocumentDescriptor.integer(info, "width"))
        let height = Int(try PrecisionDocumentDescriptor.integer(info, "height"))
        let control = try supplied ?? RasterJobControl()
        let closeObserver = NotificationCenter.default.addObserver(forName: .imageDocumentDidClose,
            object: document, queue: nil) { _ in control.cancel() }
        defer { NotificationCenter.default.removeObserver(closeObserver) }
        guard !control.isCancelled else { throw JobCancelled() }
        let snapshot = try RawSourceSnapshot.restore(source, layer: layer, descriptor: previous)
        let (current, proposedFrame) = try autoreleasepool { () throws -> (RawSourceDescriptor, RawDecoderFrame) in
            let current = try RetainedRawDecoder.capture(url: snapshot.url,
                filename: previous.original_filename, sha256: previous.content_sha256)
            let frame = try RetainedRawDecoder.decoderFrame(url: snapshot.url, descriptor: current)
            return (current, frame)
        }
        guard !control.isCancelled else { throw JobCancelled() }
        guard current.decoder != previous.decoder else {
            throw DoctoredCoreError(message: "This RAW already uses the current decoder. Use Redevelop RAW to change its settings.")
        }
        let proposedSize = try RawMigrationGeometry.dimensions(proposedFrame)
        let registration = try RawMigrationGeometry.resolve(saved: previous.recipe,
            savedSize: (width, height), proposed: proposedFrame, proposedSize: proposedSize,
            explicit: geometry)
        let beforeMasks = try RawLocalWork.maskFingerprints(source, layer: layer)
        let float = previous.recipe.isLinearFloat
        // The tone path is outside the `scratch` envelope: its share is added to both.
        let toned = previous.recipe.enhancements?.locals ?? []
        let oldBudget = try previous.recipe.enhancements == nil
            ? RawSourceWork.scratch(width: width, height: height, encoded: encoded, float: float)
            : PrecisionDocumentDescriptor.sum(
                RawLocalWork.scratch(width: width, height: height, encoded: encoded, float: float),
                RawLocalWork.toneScratch(width: width, height: height, nodes: toned))
        let newBudget = try previous.recipe.enhancements == nil
            ? RawSourceWork.scratch(width: proposedSize.0, height: proposedSize.1, encoded: encoded,
                                    float: float)
            : PrecisionDocumentDescriptor.sum(
                RawLocalWork.scratch(width: proposedSize.0, height: proposedSize.1, encoded: encoded,
                                     float: float),
                RawLocalWork.toneScratch(width: proposedSize.0, height: proposedSize.1, nodes: toned))
        let maskScratch = UInt64(proposedSize.0) * UInt64(proposedSize.1) * 8
        let budget = try PrecisionDocumentDescriptor.sum(max(oldBudget, newBudget), maskScratch)
        guard control.setScratchLimit(budget) else {
            throw DoctoredCoreError(message: "RAW decoder migration could not configure its work budget.")
        }
        guard !control.isCancelled else { throw JobCancelled() }
        _ = try RasterResources.shared.admit(scratch: budget)
        let result = try BackgroundWork.wait(try WorkerTask(scratchEstimate: budget) {
            guard !control.isCancelled else { throw JobCancelled() }
            var migrated = try recipe(previous: previous, current: current)
            if registration.changed || previous.recipe.version >= 4 {
                migrated.descriptor.recipe.version = max(migrated.descriptor.recipe.version, 4)
                migrated.descriptor.recipe.decoderFrame = proposedFrame
            }
            if var optical = previous.recipe.sourceGeometry {
                if optical.referenceSize == nil {
                    optical.referenceSize = [UInt32(width), UInt32(height)]
                    optical.referenceFromSource = RawMigrationGeometry.identity
                }
                optical.referenceFromSource = RawMigrationGeometry.compose(
                    optical.referenceFromSource ?? RawMigrationGeometry.identity, registration.matrix)
                migrated.descriptor.recipe.sourceGeometry = optical
            }
            if let vignette = previous.recipe.enhancements?.vignette,
               (registration.changed && vignette.edgeExposure != 0)
                    || previous.recipe.vignetteReference != nil {
                var reference = previous.recipe.vignetteReference ?? RawVignetteReference(
                    referenceSize: [UInt32(width), UInt32(height)],
                    referenceFromSource: RawMigrationGeometry.identity)
                reference.referenceFromSource = RawMigrationGeometry.compose(
                    reference.referenceFromSource, registration.matrix)
                migrated.descriptor.recipe.vignetteReference = reference
            }
            try migrated.descriptor.recipe.validate()
            let mappedMasks = try localState.masks.map { mask in
                try control.withChildOperation { child in
                    try mask.remapped(registration.map.json(), control: child)
                }
            }
            let decoded: (RasterDocument, RawSourceDescriptor)
            if migrated.descriptor.recipe.enhancements != nil {
                decoded = try RawLocalEvaluation.develop(snapshot: snapshot, requested: migrated.descriptor,
                    previous: previous, previousSource: nil, layer: layer, masks: mappedMasks,
                    cache: RawLocalVariantCache(), maximumScratch: budget, control: control, migrating: true)
            } else {
                decoded = try RetainedRawDecoder.develop(url: snapshot.url, descriptor: migrated.descriptor,
                    maximumScratch: budget, whiteBalanceChanged: previous.recipe.values.customWhiteBalance)
            }
            guard !control.isCancelled else { throw JobCancelled() }
            guard (decoded.0.width, decoded.0.height) == proposedSize else {
                throw DoctoredCoreError(message: "The decoder output differs from its reviewed frame.")
            }
            let prepared = try RawLocalEvaluation.replacing(source: source, layer: layer,
                asset: decoded.0, descriptor: decoded.1, masks: mappedMasks, control: control,
                sourceCoordinateMap: registration.changed ? registration.matrix : nil)
            let afterMasks = try RawLocalWork.maskFingerprints(prepared, layer: layer)
            return (decoded.0, decoded.1, prepared, afterMasks, migrated.changes)
        }, title: "Previewing Current RAW Decoder", control: control)
        try capture.requireCurrent(document)
        return RawDecoderMigration(capture: capture, layer: layer, previous: previous,
            descriptor: result.1, asset: result.0, prepared: result.2,
            geometry: registration.map, beforeMasks: beforeMasks,
            afterMasks: result.3, changes: result.4, payload: payload)
    }

    /// Transfer exposed controls that both implementations support. A formerly
    /// supported control with an authored nondefault value cannot disappear.
    static func recipe(previous: RawSourceDescriptor, current: RawSourceDescriptor)
        throws -> (descriptor: RawSourceDescriptor, changes: [String]) {
        let old = previous.recipe
        let capabilities = current.recipe.capabilities
        var settings = RawDevelopSettings()
        if let enhancements = old.enhancements {
            settings.baselineCompensation = enhancements.baselineCompensation
            settings.vignette = enhancements.vignette
            settings.grain = enhancements.grain
            if let presence = enhancements.presence {
                settings.texture = presence.texture
                settings.clarity = presence.clarity
                settings.dehaze = presence.dehaze
            }
            if old.moireSupported && current.recipe.moireSupported { settings.moire = Double(old.moire) }
            else if old.moireSupported && old.moire != 0 {
                throw DoctoredCoreError(message: "The current decoder cannot preserve saved moiré reduction. Migration was refused.")
            }
        }
        settings.outputSpace = old.outputColorSpace
        // A float development migrates as a float development.
        settings.outputEncoding = old.outputEncoding
        settings.exposure = old.values.exposure
        settings.toneCurve = old.values.toneCurve
        settings.shadows = old.values.shadows
        if old.values.customWhiteBalance {
            settings.temperature = old.values.temperature
            settings.tint = old.values.tint
        } else { settings.asShotWhiteBalance = true }
        var changes = ["The new decoder's camera calibration and hidden defaults replace the captured implementation's defaults.",
                       old.isLinearFloat
                           ? "Supported development controls and the 32-bit float (linear) output space are retained."
                           : "Supported development controls and the SDR output space are retained."]
        if !old.values.customWhiteBalance {
            changes.append("As Shot uses the new decoder's camera white balance; compare it with the saved rendering.")
        }
        let numbers: [(String, Bool, Bool, Double, Double, WritableKeyPath<RawDevelopSettings, Double?>)] = [
            ("contrast", old.capabilities.contrast, capabilities.contrast, old.values.contrast, old.capabilities.defaults.contrast, \.contrast),
            ("sharpness", old.capabilities.sharpness, capabilities.sharpness, old.values.sharpness, old.capabilities.defaults.sharpness, \.sharpness),
            ("detail", old.capabilities.detail, capabilities.detail, old.values.detail, old.capabilities.defaults.detail, \.detail),
            ("luminance noise", old.capabilities.luminanceNoise, capabilities.luminanceNoise, old.values.luminanceNoise, old.capabilities.defaults.luminanceNoise, \.luminanceNoise),
            ("colour noise", old.capabilities.colorNoise, capabilities.colorNoise, old.values.colorNoise, old.capabilities.defaults.colorNoise, \.colorNoise)]
        for (name, wasSupported, supported, value, initial, key) in numbers {
            if wasSupported && supported { settings[keyPath: key] = value }
            else if wasSupported && value != initial {
                throw DoctoredCoreError(message: "The current RAW decoder cannot preserve the saved \(name) setting. Migration was refused.")
            } else if wasSupported != supported { changes.append("\(name.capitalized) support changed; the new decoder's default is used.") }
        }
        let flags: [(String, Bool, Bool, Bool, Bool, WritableKeyPath<RawDevelopSettings, Bool?>)] = [
            ("lens correction", old.capabilities.lensCorrection, capabilities.lensCorrection, old.values.lensCorrection, old.capabilities.defaults.lensCorrection, \.lensCorrection),
            ("highlight recovery", old.capabilities.highlightRecovery, capabilities.highlightRecovery, old.values.highlightRecovery, old.capabilities.defaults.highlightRecovery, \.highlightRecovery)]
        for (name, wasSupported, supported, value, initial, key) in flags {
            if wasSupported && supported { settings[keyPath: key] = value }
            else if wasSupported && value != initial {
                throw DoctoredCoreError(message: "The current RAW decoder cannot preserve the saved \(name) setting. Migration was refused.")
            } else if wasSupported != supported { changes.append("\(name.capitalized) support changed; the new decoder's default is used.") }
        }
        var descriptor = current
        // v1-lensfun: a Lensfun profile is carried as it was saved, like the source geometry; a
        // new decoder lens profile replaces it (the decoder's own always wins), and a recipe saved
        // without one never gains one here: the fresh capture's own match is set aside first.
        var fresh = current.recipe
        if fresh.lensProfile != nil {
            fresh.lensProfile = nil
            fresh.values.lensCorrection = capabilities.defaults.lensCorrection
            fresh.version = 1
        }
        descriptor.recipe = try fresh.updating(settings)
        if !capabilities.lensCorrection, let saved = old.lensProfile {
            descriptor.recipe.lensProfile = saved
            descriptor.recipe.values.lensCorrection = old.values.lensCorrection
            descriptor.recipe.version = 5
        } else if old.lensProfile != nil {
            changes.append("The new decoder carries its own lens profile, which replaces the saved Lensfun profile.")
        }
        if let enhancements = old.enhancements {
            descriptor.recipe.enableEnhancements()
            descriptor.recipe.enhancements?.locals = enhancements.locals
            changes.append("Camera-relative baseline compensation, manual vignette, develop Texture, Clarity and Dehaze, develop grain and ordered local controls are retained. Local native coverage remains exact; every variant is reviewed under the new decoder.")
        }
        return (descriptor, changes)
    }

    func apply(in document: ImageDocument) throws {
        try capture.requireCurrent(document)
        let result = prepared
        var migration: [String: Any] = ["before": try previous.json(), "after": try descriptor.json()]
        if !beforeMasks.isEmpty { migration["masks"] = beforeMasks }
        migration["geometry"] = try JSONSerialization.jsonObject(with: Data(geometry.json().utf8))
        migration["after_masks"] = afterMasks
        let arguments: [String: Any] = ["layer": layer, "migration": migration]
        try RawSourceWork.publish(result, in: document, capture: capture, title: "Migrate RAW Decoder",
            record: .step("migrate_raw_decoder", arguments, note: "Layer ▸ Migrate RAW Decoder"))
    }
}

/// One app-wide review ticket bounds retained preview work. A replacement,
/// expiry, closed document or any changed editing state invalidates it.
enum RawMigrationReview {
    private final class Ticket {
        let id = UUID().uuidString
        weak var document: ImageDocument?
        let candidate: RawDecoderMigration
        init(document: ImageDocument, candidate: RawDecoderMigration) {
            self.document = document
            self.candidate = candidate
        }
    }
    private static var ticket: Ticket?
    private static var expiration: Timer?

    static func retain(_ candidate: RawDecoderMigration, document: ImageDocument) -> String {
        clear()
        let value = Ticket(document: document, candidate: candidate)
        ticket = value
        let timer = Timer(timeInterval: 300, repeats: false) { _ in clear() }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
        expiration = timer
        return value.id
    }
    static func take(_ id: String, document: ImageDocument) throws -> RawDecoderMigration {
        guard let value = ticket, value.id == id, value.document === document else {
            throw DoctoredCoreError(message: "This RAW migration preview expired or belongs to another document. Preview the migration again.")
        }
        clear()
        try value.candidate.capture.requireCurrent(document)
        return value.candidate
    }
    static func clear() {
        expiration?.invalidate()
        expiration = nil
        ticket = nil
    }
    static func discard(for document: ImageDocument) {
        if ticket?.document === document { clear() }
    }
}
