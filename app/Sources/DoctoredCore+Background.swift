import AppKit

/// The role a document's Background mark gives its bottom layer (the core's
/// `doc_background`): set only by a PSD import, from the file's own record 0.
enum BackgroundRole: Int32 {
    /// The file's Background layer.
    case background = 1
    /// An ordinary bottom layer: never the Background a Deep knockout punches
    /// to, whatever its shape.
    case ordinary = 2

    /// The `get_document` spelling.
    var name: String {
        switch self {
        case .background: return "background"
        case .ordinary: return "ordinary"
        }
    }
}

extension RasterDocument {
    /// The role of entry 0, or nil when no mark applies to it and D5's
    /// structural rule decides (`hasQualifyingBackground`).
    var backgroundRole: BackgroundRole? {
        let code = precision.map { dr_precision_doc_background_role($0.ptr) }
            ?? dr_doc_background_role(ptr)
        return BackgroundRole(rawValue: code)
    }
}
