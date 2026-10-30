import AppKit

// A RAW local whose coverage a model generated: Select Subject, Select People or the sky
// model (`SkySelection.coverage`, the M1 matte Select ▸ Sky reads, unrefined) run on the
// BASE development (the camera-only global render before any local, vignette, optics and grain,
// Lensfun-corrected when the recipe's profile applies, the image a range mask already reads), so
// the coverage is in canonical source coordinates, the corrected frame when Lensfun applies. The
// result is an ordinary native u16 RZLM mask; `RawLocalNode.generated` only records which
// request made it.
//
// Recompute is on request only. Stored coverage never changes by itself: not on a global edit, a
// preset, Sync Settings, Redevelop RAW, a decoder migration or a reopen. A generating operation
// reads the base of the request it is in, so inside one open Develop session a not-yet-applied
// generated mask follows that session's global settings; after Apply it is stored. Recompute
// replaces the whole plane: brush strokes and Invert made since are not replayed.
//
// No Vision or Core ML code lives here: the subject request is `SubjectSelection.mask(in:)` (the
// call Select ▸ Subject makes), the people request `SubjectTargeting.people(in:)` (the call
// Select ▸ All People makes, its human/face gate included) and the sky request
// `SkySelection.coverage` (a missing model refuses with `ModelGate.notInstalled("sky")`). `.sky`
// rides the existing `generated` key with no recipe or DRDC version bump: an older build refuses
// the recipe and keeps the rendering, the documented rule.

enum RawLocalGeneratedKind: String, Codable, CaseIterable {
    case subject, people, sky

    var title: String {
        switch self {
        case .subject: return "Subject"
        case .people: return "People"
        case .sky: return "Sky"
        }
    }
    /// The sentence a request that found nothing refuses with: the selection commands' own.
    var nothingFound: String {
        switch self {
        case .subject: return SubjectSelection.Failure.noSubject.message
        case .people: return SubjectTargeting.noPeopleMessage
        case .sky: return SkySelection.noSky
        }
    }
}

/// A generating operation that produced no mask. `operation` is its index in the request's
/// operation list, which is what lets a Develop session drop exactly that operation.
struct RawLocalGenerationRefusal: LocalizedError {
    let operation: Int
    let message: String
    var errorDescription: String? { message }
}

enum RawLocalGenerated {
    /// nil unless `object["op"]` is subject, people or sky; such an object may hold only `op`.
    static func kind(of object: [String: Any]) throws -> RawLocalGeneratedKind? {
        guard let op = object["op"] as? String, let kind = RawLocalGeneratedKind(rawValue: op) else {
            return nil
        }
        guard object.count == 1 else {
            throw DoctoredCoreError(message: "A generated RAW mask (subject, people or sky) takes op and no other field.")
        }
        return kind
    }

    /// Worker thread. `base` is the 16-bit base development.
    static func mask(_ kind: RawLocalGeneratedKind, base: RasterDocument,
                     control: RasterJobControl) throws -> RasterRawLocalMask {
        guard let source = base.precision?.layerSource(0) else {
            throw DoctoredCoreError(message: "A generated RAW mask is read from a 16-bit base development.")
        }
        let context = try PrecisionWorkContext(image: source)
        let display = try control.withChildOperation { child in
            try source.previewRGBA8(workingLimit: context.limit, otherRetained: context.otherRetained,
                control: child)
        }
        // The base's own profile, as the Develop preview displays it.
        guard let space = CGColorSpace(iccData: source.profileData as CFData),
              let image = display.makeCGImage(in: space) else {
            throw DoctoredCoreError(message: "The RAW base development could not be read for mask generation.")
        }
        guard !control.isCancelled else { throw JobCancelled() }
        let coverage: [UInt8]
        switch kind {
        case .subject: coverage = try SubjectSelection.mask(in: image).mask
        case .people: coverage = try SubjectTargeting.people(in: image)
        case .sky:
            // The model's own term, admitted before it runs (`SkySelection.coverage`).
            guard let model = ModelGate.ready("sky") else {
                throw DoctoredCoreError(message: ModelGate.notInstalled("sky"))
            }
            let pixels = UInt64(image.width) * UInt64(image.height)
            _ = try RasterResources.shared.admit(scratch: model.entry.inferenceBytes
                + ModelMaskRun.coverageScratch(width: image.width, height: image.height)
                + ModelMaskRun.captureBytesPerPixel * pixels)
            coverage = try control.withChildOperation { child in
                try SkySelection.coverage(in: image, limit: context.limit, other: context.otherRetained,
                                          control: child)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        let stored = try control.withChildOperation { child in
            try RasterRawLocalMask.fromCoverage(coverage, width: image.width, height: image.height,
                control: child)
        }
        guard let stored else { throw DoctoredCoreError(message: kind.nothingFound) }
        return stored
    }

    /// The list without the refused operation (for a `create`, also without every later operation
    /// carrying its id), or nil to keep the list: nil when `refusal.operation < previewed` (an
    /// evaluation already succeeded with that operation in the list) or is not an index of
    /// `operations`. A refusal therefore never deletes work the user has already seen previewed.
    static func dropping(_ refusal: RawLocalGenerationRefusal, from operations: [[String: Any]],
                         previewed: Int) -> [[String: Any]]? {
        let refused = refusal.operation
        guard operations.indices.contains(refused), refused >= previewed else { return nil }
        let created = operations[refused]["op"] as? String == "create"
            ? operations[refused]["id"] as? String : nil
        var kept: [[String: Any]] = []
        for (index, operation) in operations.enumerated() where index != refused {
            if index > refused, let created, operation["id"] as? String == created { continue }
            kept.append(operation)
        }
        return kept
    }

    /// Appended to a kept refusal's message in the Develop status line.
    static let keptSuffix = " · The local is kept; restore the setting or Reset."
}
