import AppKit

// `Clipboard.writeItem`, the ONE pasteboard write, throws its refusal sentence, which the UI beeps
// or presents and MCP answers as `isError`. A typed RZCB at or under
// `HeldClipboardPayload.inlineLimit` (`systemInlineLimit`, 255 MiB, probed;
// `DR_CLIPBOARD_INLINE_LIMIT_BYTES` overrides it for tests) is written inline under
// `Clipboard.typedType` as before; a larger one, or one the pasteboard refuses, is held in process
// (charged as retained through `PrecisionEditPayload`) and named by a UUID under
// `HeldClipboardPayload.tokenType` beside the public TIFF/PNG — `LayerClipboard`'s token pattern:
// `isAvailable(on:)` is a type query that drops the entry when the token type is gone,
// `current(on:)` reads the token string and drops a stale entry. `TypedClipboardPayload.read` asks
// `current(on:)` FIRST and falls through to the inline type; `Clipboard.typedStore(on:)` is MCP's
// `typed_store` (`"pasteboard"`, `"process"` or null) on `copy`, `copy_merged`, `cut` and
// `get_clipboard`. Another instance sees only the token and the flat image, which pastes as an
// external image at every depth.

/// A typed clipboard payload too large for the system pasteboard is
/// HELD in process, the way the layer clipboard holds its clip
/// (`LayerClipboard`): the pasteboard item carries a UUID token under
/// `tokenType` beside the public TIFF/PNG, and `TypedClipboardPayload.read`
/// resolves the token to the held RZCB. A payload at or under `inlineLimit`
/// is written in full under `Clipboard.typedType`, exactly as before, so it
/// still crosses to other Doctored instances.
///
/// Another Doctored instance — or any instance after the copying app quits —
/// sees only the token and the flat TIFF/PNG, and pastes that as an external
/// image.
///
/// The held bytes are charged as retained (`PrecisionEditPayload`), so
/// `PrecisionWorkContext.otherRetained` sees them. The entry is released by
/// the next copy, by a validation that finds the token type gone, by a stale
/// token read, and at quit.
final class HeldClipboardPayload {
    static let shared = HeldClipboardPayload()

    /// The private pasteboard type naming the held payload.
    static let tokenType = NSPasteboard.PasteboardType("com.kgalligan.doctored.held-clipboard")

    /// Test override of `systemInlineLimit`, in bytes.
    static let environmentLimit = "DR_CLIPBOARD_INLINE_LIMIT_BYTES"

    /// The largest typed blob, in whole MiB, that batch 172's probe wrote
    /// beside two 96 MB public blobs to a named pasteboard: 255 MiB wrote and
    /// 256 MiB was refused, with and without the public blobs (2026-10-05,
    /// macOS 26). 216,000,100 bytes wrote and 384,000,000 were refused, as in
    /// batch 171.
    static let systemInlineLimit = 255 << 20

    /// `environmentLimit` when it parses as a positive Int, else
    /// `systemInlineLimit`.
    static var inlineLimit: Int {
        if let text = ProcessInfo.processInfo.environment[environmentLimit],
           let value = Int(text), value > 0 {
            return value
        }
        return systemInlineLimit
    }

    /// One held payload: the pasteboard token, the RZCB and its charge.
    struct Entry {
        let token: String
        let data: Data
        let charge: PrecisionEditPayload
    }

    private var entry: Entry?

    private init() {}

    /// Validation only: a TYPE query plus the held entry, never a data read.
    /// Drops the entry when `tokenType` is absent (including when another
    /// writer put an inline typed payload there).
    func isAvailable(on pasteboard: NSPasteboard) -> Bool {
        guard pasteboard.availableType(from: [Self.tokenType]) != nil else {
            entry = nil
            return false
        }
        return entry != nil
    }

    /// The held RZCB when the pasteboard's token string equals the entry's;
    /// a stale entry (a missing token string included) is dropped.
    func current(on pasteboard: NSPasteboard) -> Data? {
        guard let held = entry else { return nil }
        guard pasteboard.string(forType: Self.tokenType) == held.token else {
            entry = nil
            return nil
        }
        return held.data
    }

    /// Holds `data` in place of any earlier entry, charged as retained, and
    /// returns the fresh token the pasteboard item names it by.
    func hold(_ data: Data) throws -> String {
        release()
        let charge = try PrecisionEditPayload.admitted(bytes: UInt64(data.count))
        let token = UUID().uuidString
        entry = Entry(token: token, data: data, charge: charge)
        return token
    }

    func release() {
        entry = nil
    }

    /// The sentence the MCP `copy` and `get_clipboard` descriptions append.
    static let catalogSentence = "A typed payload larger than the system pasteboard accepts "
        + "(over \(systemInlineLimit >> 20) MiB) stays in this Doctored instance, named on the "
        + "pasteboard by a token; other apps and other Doctored instances get only the flat "
        + "TIFF/PNG. typed_store reports where the typed payload is: \"pasteboard\", "
        + "\"process\" (held in this instance) or null."
}

extension Clipboard {
    /// The one pasteboard write: Copy, Copy Merged, Cut, the layers copy, UI
    /// and MCP. TIFF + PNG of `cgImage` and the typed payload in ONE item —
    /// the RZCB itself when it fits `HeldClipboardPayload.inlineLimit` and
    /// the pasteboard takes it, otherwise a token naming it held in process.
    /// Throws when nothing could be written; Cut clears only on success.
    /// `source` (`ClipboardSource`) rides in the same item, so
    /// Paste in Place knows the canvas the copy came from; it has no default,
    /// so every writer decides.
    static func writeItem(_ cgImage: CGImage, typed: Data?, layerToken: String? = nil,
                          source: ClipboardSource?, tiff grayTIFF: Data? = nil) throws {
        let rep = NSBitmapImageRep(cgImage: cgImage)
        // #1 phase 4: a Grayscale copy's TIFF is its one-channel gray image
        // (`RasterClipboardPayload.grayTIFF`); the PNG stays the RGB rendering.
        let tiff = grayTIFF ?? rep.representation(using: .tiff, properties: [:])
        let png = rep.representation(using: .png, properties: [:])
        let held = HeldClipboardPayload.shared
        let pasteboard = Clipboard.pasteboard

        func makeItem(typed: Data? = nil, heldToken: String? = nil) -> NSPasteboardItem {
            let item = NSPasteboardItem()
            if let layerToken { item.setString(layerToken, forType: LayerClipboard.tokenType) }
            if let source { item.setData(source.data, forType: ClipboardSource.type) }
            if let typed { item.setData(typed, forType: Clipboard.typedType) }
            if let heldToken { item.setString(heldToken, forType: HeldClipboardPayload.tokenType) }
            if let tiff { item.setData(tiff, forType: .tiff) }
            if let png { item.setData(png, forType: .png) }
            return item
        }
        func write(_ item: NSPasteboardItem) -> Bool {
            pasteboard.clearContents()
            return pasteboard.writeObjects([item])
        }
        func refusal() -> DoctoredCoreError {
            func size(_ data: Data?) -> String {
                ByteCountFormatter.string(fromByteCount: Int64(data?.count ?? 0), countStyle: .file)
            }
            return DoctoredCoreError(message: "The system pasteboard refused the copied "
                + "\(cgImage.width)×\(cgImage.height) image (\(size(tiff)) TIFF, "
                + "\(size(png)) PNG).")
        }

        if let typed, !typed.isEmpty {
            if typed.count <= HeldClipboardPayload.inlineLimit,
               write(makeItem(typed: typed)) {
                held.release()
                return
            }
            // Over the limit, or the inline write was refused: hold it.
            // hold released the earlier entry before admitting this one, so a
            // refused hold clears the pasteboard rather than leave it naming
            // a token nothing backs.
            let token: String
            do { token = try held.hold(typed) } catch {
                pasteboard.clearContents()
                throw error
            }
            guard write(makeItem(heldToken: token)) else {
                held.release()
                throw refusal()
            }
            return
        }
        let item = makeItem()
        guard !item.types.isEmpty else {
            throw DoctoredCoreError(message: "The clipboard could not be written.")
        }
        held.release()
        guard write(item) else { throw refusal() }
    }

    /// Where the clipboard's typed payload is: "pasteboard" (the RZCB
    /// itself), "process" (held in this instance) or nil. The held check
    /// runs first, so a stale entry is dropped either way.
    static func typedStore(on pasteboard: NSPasteboard) -> String? {
        let held = HeldClipboardPayload.shared.current(on: pasteboard) != nil
        if pasteboard.availableType(from: Clipboard.readableTypedTypes) != nil { return "pasteboard" }
        return held ? "process" : nil
    }
}
