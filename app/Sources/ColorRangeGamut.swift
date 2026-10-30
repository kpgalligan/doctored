import AppKit

// Select ▸ Color Range ▸ Out Of Gamut. There is no second gamut test: the core's
// `dr_selection_out_of_gamut` turns the gamut warning's one rule (`soft_proof::GAMUT_DELTA_E`, the
// relative round trip) into coverage over the SAME 8-bit codes the window's gamut warning proofs —
// the captured byte composite at 8 bits, the projection's display codes
// (`ProofRenderer.displayCodes`) at 16 bits, the R1 display codes Color Range already compares
// (`edgeTraceComposite`) at 32 bits. A CMYK document is all in gamut: the result
// is empty (all selected with Invert) with no core call.
//
// The proof setup is VIEW state: Out Of Gamut reads the window's (`proofDisplay.setup`) and never
// writes it. With none it uses the default proof preset, then Working CMYK, the order
// `AgentServer.baseProofSetup(seedWorkingCMYK:)` falls back through.

enum ColorRangeGamut {
    /// The `ColorRangeParameters.range` value, the MCP `range` and the recorded Action key.
    static let range = "out_of_gamut"

    /// The setup Out Of Gamut proofs through (file comment), made valid for an RGB proof: Preserve
    /// Numbers dropped and no plate (neither changes the gamut rule).
    static func setup(window: ProofSetup?) throws -> ProofSetup {
        let chosen = try window ?? ProofPresetLibrary.defaultPreset()?.resolvedSetup()
            ?? ProofSetup.workingCMYK(plate: nil)
        guard var setup = chosen?.reconciled(isCmykDocument: false) else {
            throw DoctoredCoreError(message: "Out Of Gamut needs a proof profile: choose View ▸ Proof "
                + "Setup, or install a working CMYK profile.")
        }
        setup.plate = nil
        return setup
    }
}

/// Out Of Gamut's captured input, built on the main thread; `coverage` is value-only and runs on
/// the background worker or the sheet's `PreviewRenderer`.
struct ColorRangeGamutInput {
    private enum Source {
        /// The 8-bit codes themselves (8 bits: the captured composite; 32 bits: R1 display codes).
        case flat(RasterImage)
        /// A 16-bit projection, turned into its display codes on the worker.
        case display(RasterPrecisionImage, RasterDocument)
        /// A CMYK document: all in gamut.
        case cmyk
    }

    let setup: ProofSetup
    /// The export's scratch plus the output, plus 4·w·h at 16 bits for the display codes the
    /// worker holds. A CMYK document reserves its coverage and the preview's grey image.
    let reservationBytes: UInt64
    private let source: Source
    private let width: Int
    private let height: Int
    private let profile: Data
    private let setupJSON: String

    var isCmyk: Bool {
        if case .cmyk = source { return true }
        return false
    }

    init(capture: SelectionCapture, document: ImageDocument, setup: ProofSetup) throws {
        let doc = capture.doc
        self.setup = setup
        width = doc.width
        height = doc.height
        // #547 Lab phase 6b-i: a Lab document proofs its Display P3 view (5b's rule).
        profile = doc.proofSourceProfile
        setupJSON = setup.jsonText(gamutWarning: false)
        let pixels = UInt64(width * height)
        if doc.isCmyk {
            source = .cmyk
            reservationBytes = pixels * 2
            return
        }
        if doc.isFloat {
            source = .flat(try document.edgeTraceComposite(of: doc))
        } else if doc.isHighDepth {
            source = .display(try document.currentPrecisionProjection(), doc)
        } else {
            guard let flat = capture.source else {
                throw DoctoredCoreError(message: "The image has no captured projection.")
            }
            source = .flat(flat)
        }
        let scratch = RasterImage.outOfGamutScratch(width: width, height: height,
                                                    documentProfileBytes: profile.count,
                                                    proof: setup.profile)
        guard scratch > 0 else {
            throw DoctoredCoreError(message: "The proof profile “\(setup.profileName)” cannot be read.")
        }
        var bytes = scratch + pixels
        if case .display = source { bytes += pixels * 4 }
        reservationBytes = bytes
    }

    /// The coverage, a byte `.mask` at every depth. At 16 bits `control` (when given) becomes a
    /// compound control: one child converts to display codes, the next runs the export.
    func coverage(invert: Bool, control: RasterJobControl?) throws -> CanvasSelection.Shape {
        switch source {
        case .cmyk:
            return .mask([UInt8](repeating: invert ? 255 : 0, count: width * height))
        case .flat(let flat):
            return .mask(try select(flat, invert: invert, control: control))
        case .display(let projection, let doc):
            guard let control else {
                let display = try ProofRenderer.displayCodes(of: projection, in: doc)
                return .mask(try select(display, invert: invert, control: nil))
            }
            try control.beginCompoundOperation()
            let display = try control.withChildOperation(from: 0, to: 0.3) {
                try ProofRenderer.displayCodes(of: projection, in: doc, control: $0)
            }
            return .mask(try control.withChildOperation(from: 0.3, to: 1) {
                try select(display, invert: invert, control: $0)
            })
        }
    }

    private func select(_ flat: RasterImage, invert: Bool,
                        control: RasterJobControl?) throws -> [UInt8] {
        let context = try PrecisionWorkContext(byteImage: flat)
        return try flat.outOfGamutCoverage(
            documentProfile: profile, proof: setup.profile, setup: setupJSON, invert: invert,
            workingLimit: context.limit, otherRetained: context.otherRetained, control: control)
    }
}

extension SelectionCapture {
    /// Out Of Gamut's final coverage, on the background worker with progress and Cancel (the
    /// `render_proof exact:true` pattern), checked current before and after.
    func outOfGamutShape(invert: Bool, setup: ProofSetup,
                         in document: ImageDocument) throws -> CanvasSelection.Shape {
        try requireCurrent(in: document)
        let input = try ColorRangeGamutInput(capture: self, document: document, setup: setup)
        if input.isCmyk { return try input.coverage(invert: invert, control: nil) }
        let control = try RasterJobControl()
        let task = try WorkerTask(scratchEstimate: input.reservationBytes) {
            try input.coverage(invert: invert, control: control)
        }
        let shape: CanvasSelection.Shape
        do {
            shape = try BackgroundWork.wait(task, title: "Color Range", control: control)
        } catch {
            if control.isCancelled { throw JobCancelled() }
            throw error
        }
        try requireCurrent(in: document)
        guard !control.isCancelled else { throw JobCancelled() }
        return shape
    }
}
