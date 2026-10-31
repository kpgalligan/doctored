import AppKit
import CryptoKit

// `SoftProof` is VIEW state: `ProofDisplay` lives on `EditorViewController`, never in
// `EditingState`, never in a `.rz`, never an undo step and never an Action step — the ruler/grid
// exemption, for the same four reasons, which is why the five soft-proof tools are `nonRecordable`.
// The proof is delivered through `ImageCanvasView.proofImage`, a display-only slot drawn as
// `previewImage ?? proofImage ?? image`; it must NEVER be assigned to `canvas.image`, whose
// `didSet` clears the overlay, resets the selection and the Quick Mask, cancels the subject session
// and cancels an in-progress selection stroke.
//
// A sheet's live preview wins over the proof and is UNPROOFED — a recorded decision, because
// proofing a preview would re-run the whole chain per slider tick.
//
// `ProofRenderer` gives each request one compound `RasterJobControl` with a child per core
// call: the next request, `cancel()` and deinit cancel it, and a cancelled render delivers nothing,
// so the held proof stays up; a request only ever delivers its OWN landing. At 16 bits the
// canvas and the default `render_proof` proof the RETAINED projection's 8-bit display codes
// (`ProofSource.display`, `ProofRenderer.displayCodes`) through the byte proof — never a fresh
// composite; while `ImageDocument.retainedPrecisionProjection` is nil and a projection refresh is
// scheduled no request is made (the refresh re-enters), and with none scheduled the proof fails.
// `render_proof exact:true` is the full-depth typed proof on `BackgroundWork`. At 8 bits the canvas
// and `render_proof` proof the RETAINED projection (`ImageDocument.retainedByteProjection`, read on
// main and carried in `ProofSource.flat(doc, retained)`) and render the document only when none is
// retained — `render_proof` as `retainedByteProjection ?? doc.render()`, never
// `currentProjection()`, whose stored `projectionError` may be stale.
//
// `ProofSetup`'s profile fields are `private(set)` behind `setProfile`, which recomputes
// `profileSHA256` once per profile change.

/// Soft proofing, host side: the setup a window proofs through, the per-window
/// view state the two View menu toggles write, and the coalescing background
/// renderer that turns a projection into a proofed `CGImage`.
///
/// **This is VIEW state.** Nothing here is persisted in a document, registered
/// with undo, counted as a change, or recorded as an Action step. A proof
/// never reaches `canvas.image`, the master, a render or an export — it is
/// delivered through `ImageCanvasView.proofImage`, a display-only slot.

/// The proof profile and how the simulation runs through it. Every default is
/// the core's (`soft_proof::ProofSetup`).
struct ProofSetup: Equatable {
    /// The proof profile's bytes. Kept rather than a path, because the file
    /// may be replaced under a long-lived window and a proof must not change
    /// silently. Every change goes through `setProfile`, which keeps
    /// `profileSHA256` in step with the bytes.
    private(set) var profile: Data
    private(set) var profileName: String
    var intent: RenderingIntent = .relative
    var blackPointCompensation: Bool = true
    var simulatePaperWhite: Bool = false
    var simulateInkBlack: Bool = false
    var gamutColor: NSColor = NSColor(deviceRed: 0.5, green: 0.5, blue: 0.5, alpha: 1)
    /// Preserve CMYK Numbers (#1 phase 5): a CMYK document's ink goes to the proof profile
    /// unconverted. Meaningless (and dropped, `reconciled`) on an RGB document.
    var preserveNumbers: Bool = false
    /// One plate of the proof device instead of the colour (View ▸ Proof Setup ▸ Working Cyan
    /// Plate, …); needs a CMYK proof profile.
    var plate: CmykPlane? = nil

    /// The originating file, when the setup came from one. Display only — the
    /// bytes above are the authority.
    private(set) var path: String?
    /// The lowercase hex SHA-256 of `profile`, computed once per profile
    /// change: what a saved proof preset matches against without I/O.
    private(set) var profileSHA256: String

    init(profile: Data, profileName: String, path: String? = nil) {
        self.profile = profile
        self.profileName = profileName
        self.path = path
        self.profileSHA256 = ProofSetup.sha256Hex(profile)
    }

    /// The one way the profile changes: bytes, name, path and digest together.
    mutating func setProfile(_ data: Data, name: String, path: String?) {
        profile = data
        profileName = name
        self.path = path
        profileSHA256 = ProofSetup.sha256Hex(data)
    }

    static func sha256Hex(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    /// The `setup_json` the core takes. `gamut_warning` is carried by
    /// `ProofDisplay`, so the caller supplies it.
    func json(gamutWarning: Bool) -> [String: Any] {
        [
            "intent": intent.rawValue,
            "black_point_compensation": blackPointCompensation,
            "simulate_paper_white": simulatePaperWhite,
            "simulate_ink_black": simulateInkBlack,
            "gamut_warning": gamutWarning,
            "gamut_color": ProofSetup.hex(gamutColor),
            "preserve_numbers": preserveNumbers,
            "plate": plate.map { $0.key as Any } ?? NSNull(),
        ]
    }

    func jsonText(gamutWarning: Bool) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: json(gamutWarning: gamutWarning)),
              let text = String(data: data, encoding: .utf8)
        else { return "{}" }
        return text
    }

    /// `#RRGGBB` in the device RGB space the colour well edits in.
    static func hex(_ color: NSColor) -> String {
        let rgb = color.usingColorSpace(.deviceRGB) ?? color
        let r = Int((rgb.redComponent * 255).rounded())
        let g = Int((rgb.greenComponent * 255).rounded())
        let b = Int((rgb.blueComponent * 255).rounded())
        return String(format: "#%02X%02X%02X", r, g, b)
    }

    /// The inverse, for an MCP `gamut_color` argument. nil for anything that
    /// is not `#RRGGBB`.
    static func color(hex text: String) -> NSColor? {
        var body = text
        if body.hasPrefix("#") { body.removeFirst() }
        guard body.count == 6, let value = UInt32(body, radix: 16) else { return nil }
        return NSColor(
            deviceRed: CGFloat((value >> 16) & 0xFF) / 255,
            green: CGFloat((value >> 8) & 0xFF) / 255,
            blue: CGFloat(value & 0xFF) / 255,
            alpha: 1)
    }
}

/// One window's proofing state. Lives on the editor, never in `EditingState`,
/// never in a `.rz`, never in an Action.
struct ProofDisplay {
    /// nil until the user chooses a profile in View ▸ Proof Setup….
    var setup: ProofSetup?
    /// View ▸ Proof Colors (⌘Y).
    var proofColors: Bool = false
    /// View ▸ Gamut Warning (⇧⌘Y).
    var gamutWarning: Bool = false
    /// The refusal that last turned proofing off: a failed proof
    /// turns itself off VISIBLY. Cleared by the next toggle or setup.
    var failure: String?

    /// True when a proof should actually be drawn.
    var isActive: Bool { proofColors && setup != nil }

    /// The one-line status a host shows beside the zoom: the profile's name
    /// and, when it is on, the warning — or why proofing turned itself off.
    var statusLine: String? {
        if let failure { return "Proof off — \(failure)" }
        guard proofColors, let setup else { return nil }
        return gamutWarning ? "Proof: \(setup.profileName) + gamut" : "Proof: \(setup.profileName)"
    }

    /// The MCP `get_proof_setup` payload.
    var json: [String: Any] {
        var out: [String: Any] = [
            "proof_colors": proofColors,
            "gamut_warning": gamutWarning,
            "failure": failure as Any? ?? NSNull(),
        ]
        if let setup {
            out["profile_name"] = setup.profileName
            out["path"] = setup.path as Any? ?? NSNull()
            out["intent"] = setup.intent.rawValue
            out["black_point_compensation"] = setup.blackPointCompensation
            out["simulate_paper_white"] = setup.simulatePaperWhite
            out["simulate_ink_black"] = setup.simulateInkBlack
            out["gamut_color"] = ProofSetup.hex(setup.gamutColor)
            out["preserve_numbers"] = setup.preserveNumbers
            out["plate"] = setup.plate.map { $0.key as Any } ?? NSNull()
        } else {
            out["profile_name"] = NSNull()
        }
        return out
    }
}

/// What one proof render produced.
struct ProofResult {
    /// The proofed pixels, already tagged with the space the core reported.
    let image: CGImage
    /// `"document"`, `"srgb"` or (a CMYK document's proof) `"display-p3"` — which profile
    /// `image` is tagged with.
    let space: String
    let outOfGamut: Int
    /// The size the proof was rendered AT. A landing result whose size no
    /// longer matches the canvas's own image is stale and is dropped.
    let pixelSize: NSSize
}

/// What a canvas proof is rendered FROM.
enum ProofSource {
    /// An 8-bit document and its RETAINED projection — what the canvas shows;
    /// nil when none is retained, and then this request renders the
    /// document.
    case flat(RasterDocument, RasterImage?)
    /// A 16-bit document and its RETAINED projection: the projection's 8-bit
    /// display codes — exactly what the canvas shows — are proofed, never a
    /// fresh composite. The full-depth proof is `render_proof exact:true`.
    case display(RasterDocument, RasterPrecisionImage)
    /// A CMYK document (#1 phase 5): its composite's ink proofed by the core
    /// (`dr_precision_doc_cmyk_proof`) into Display P3.
    case cmyk(RasterDocument)
}

/// How one delivered request ended. A cancelled or coalesced-away request
/// delivers nothing at all.
enum ProofOutcome {
    case rendered(ProofResult)
    /// The refusal or error, worded for the status bar and `failure`.
    case failed(String)
}

/// Renders proofs through the shared `PreviewRenderer` seam: coalescing, one
/// at a time, off the main thread, delivered back on it. Deliberately NOT a
/// queue of its own — `app/CLAUDE.md` names the `DispatchQueue` seams this
/// app has, and a proof is exactly what the preview one is for.
///
/// The renderer knows nothing about the canvas. It takes immutable values — a
/// document handle and its retained projection (at 8 bits, when one is
/// retained), the setup — and calls back with a `ProofOutcome`.
final class ProofRenderer {
    private let renderer = PreviewRenderer()
    /// What the last finished compute left for `onRender`, read and consumed
    /// on the main thread: the size and space a delivered render came back
    /// with, the failure it met, or `.cancelled` for a render the next
    /// request (or `cancel()`) stopped, which delivers nothing so the held
    /// proof stays up.
    private enum Landing {
        case rendered(ProofResult)
        case failed(String)
        case cancelled
    }
    /// The landing and the request control it belongs to: a request only
    /// ever delivers its OWN landing, never one a superseded request left.
    private var landed: (control: RasterJobControl, landing: Landing)?
    private let lock = NSLock()

    deinit { renderer.cancel() }

    /// Cancels whatever is in flight and drops any pending request.
    func cancel() {
        renderer.cancel()
    }

    /// Requests a proof of `source`. `completion` runs on the main thread; a
    /// coalesced-away or cancelled request never calls back. Each request
    /// carries one compound job control, and the next request cancels
    /// the one before it. The working budget is taken inside the compute.
    func request(source: ProofSource,
                 setup: ProofSetup,
                 gamutWarning: Bool,
                 completion: @escaping (ProofOutcome) -> Void) {
        let setupJSON = setup.jsonText(gamutWarning: gamutWarning)
        let proofBytes = setup.profile
        renderer.cancelCompute?()
        let control: RasterJobControl
        do {
            control = try RasterJobControl()
            try control.beginCompoundOperation()
        } catch {
            completion(.failed(error.localizedDescription))
            return
        }
        renderer.cancelCompute = { control.cancel() }
        renderer.onRender = { [weak self] _ in
            guard let self else { return }
            self.lock.lock()
            let landed = self.landed
            self.landed = nil
            self.lock.unlock()
            guard let landed, landed.control === control else { return }
            switch landed.landing {
            case .cancelled:
                return
            case .failed(let reason):
                completion(.failed(reason))
            case .rendered(let result):
                completion(.rendered(result))
            }
        }
        renderer.request { [weak self] in
            var image: CGImage?
            var landing: Landing
            do {
                let rendered = try ProofRenderer.render(
                    source: source, proof: proofBytes, setupJSON: setupJSON, control: control)
                image = rendered.image
                landing = .rendered(rendered)
            } catch {
                landing = .failed(error.localizedDescription)
            }
            if control.isCancelled {
                landing = .cancelled
                image = nil
            }
            self?.lock.lock()
            self?.landed = (control, landing)
            self?.lock.unlock()
            return image
        }
    }

    /// The CGColorSpace a proof the core reported in `space` is tagged with: sRGB, the built-in
    /// Display P3 (a CMYK document's proof), else the document's own profile.
    static func space(_ space: String, profile: Data) -> CGColorSpace {
        switch space {
        case "srgb": return ColorProfile.sRGB
        case "display-p3": return ColorProfile.displayP3
        default: return ColorProfile.space(for: profile)
        }
    }

    /// A 16-bit projection's 8-bit DISPLAY codes — what the canvas draws, and
    /// what the default 16-bit proof runs through. A float document has no
    /// soft proof and refuses by name.
    static func displayCodes(of projection: RasterPrecisionImage, in document: RasterDocument,
                             control: RasterJobControl? = nil) throws -> RasterImage {
        guard !document.isFloat else {
            throw DoctoredCoreError(message: FloatDocument.refusal(tool: "render_proof"))
        }
        return try document.display(of: projection, context: PrecisionWorkContext(image: projection),
                                    control: control)
    }

    /// The actual core calls. Static and value-only: it runs off the main
    /// thread and must capture no AppKit state (`BackgroundWork.swift`).
    /// Each core call runs under its own child of the request's compound
    /// `control`.
    private static func render(source: ProofSource,
                               proof: Data,
                               setupJSON: String,
                               control: RasterJobControl) throws -> ProofResult {
        let document: RasterDocument
        let flat: RasterImage
        let start: Double
        if case .cmyk(let doc) = source {
            let rendered = try control.withChildOperation(from: 0, to: 1) {
                try CmykProof.render(doc, proof: proof, setupJSON: setupJSON, control: $0)
            }
            guard let result = finish(rendered, profile: Data()) else {
                throw DoctoredCoreError(message: "The proof could not be drawn.")
            }
            return result
        }
        switch source {
        case .flat(let doc, let retained):
            document = doc
            if let retained {
                flat = retained
                start = 0
            } else {
                // The flatten runs under the request's control.
                flat = try control.withChildOperation(from: 0, to: 0.5) {
                    try doc.render(control: $0)
                }
                start = 0.5
            }
        case .display(let doc, let projection):
            document = doc
            flat = try control.withChildOperation(from: 0, to: 0.2) {
                try displayCodes(of: projection, in: doc, control: $0)
            }
            start = 0.2
        case .cmyk:
            throw DoctoredCoreError(message: "The proof could not be drawn.")
        }
        // #1 Lab phase 5b: a Lab document's display codes are its Display P3 view.
        let profile = document.proofSourceProfile
        let context = try PrecisionWorkContext(byteImage: flat)
        var rendered = try control.withChildOperation(from: start, to: 1) {
            try flat.proofRender(
                documentProfile: profile, proof: proof, setup: setupJSON,
                workingLimit: context.limit, otherRetained: context.otherRetained, control: $0)
        }
        rendered.space = LabDocument.reportedProofSpace(rendered.space, isLab: document.isLab)
        guard let result = finish(rendered, profile: profile) else {
            throw DoctoredCoreError(message: "The proof could not be drawn.")
        }
        return result
    }

    private static func finish(
        _ rendered: (image: RasterImage, space: String, outOfGamut: Int),
        profile: Data
    ) -> ProofResult? {
        // The proof is TAGGED with the space the core reported, so
        // CoreGraphics applies the monitor profile from there — there is
        // deliberately no monitor compensation in the core.
        let space = ProofRenderer.space(rendered.space, profile: profile)
        guard let image = rendered.image.makeCGImage(in: space) else { return nil }
        return ProofResult(
            image: image,
            space: rendered.space,
            outOfGamut: rendered.outOfGamut,
            pixelSize: NSSize(width: rendered.image.width, height: rendered.image.height))
    }
}
