import AppKit

/// One element of a `.kys` file's root, in file order: a menu command (`kind` "static" carries an
/// `id`, "dynamic" only a `name`; `shortcuts` empty = the binding was removed), a tool (`type` "1"
/// is a tool proper, keyed by its four-character code as a decimal `key`; `letter` "" = the key
/// was removed), or anything else (a `<taskspace>` block). `name` is what the report shows.
enum KysEntry: Equatable {
    case command(kind: String, id: String?, name: String, shortcuts: [String])
    case tool(type: String, key: String?, name: String, letter: String)
    case other(name: String)
}

/// What an import did, by entry name: `applied` (taken, or equal to this build's default),
/// `unmapped` (nothing here to map it onto) and `refused` ("<entry>: <sentence>").
struct KysImportReport: Equatable {
    var applied: [String] = []
    var unmapped: [String] = []
    var refused: [String] = []
}

/// The `.kys` keyboard-shortcut exchange format, read into a NEW named set (`ShortcutSets.importKys`,
/// `designs/shortcut-sets.md`). Entries map through the explicit tables below and nothing else:
/// static commands by `id` (their names are localized), dynamic commands by `name`, tools by `key`.
/// Nothing is matched by title. Every chord passes `ShortcutChord.validated()` and the same
/// conflict rule `ShortcutMenu.assign` applies, and every tool key `ToolKeys.validated`; whatever
/// does not is reported, never guessed.
enum KysImport {
    static let maximumBytes = 1 << 20
    static let maximumEntries = 4096
    static let readVersion = "4"

    /// Static command `id` → `ShortcutMenu.identity`.
    static let staticCommands: [String: String] = [
        "10": "newDocument:", "20": "openDocument:", "30": "saveDocument:", "32": "saveDocumentAs:",
        "34": "revertDocumentToSaved:", "103": "cut:", "104": "copy:", "105": "paste:",
        "1002": "toggleRulers:", "1004": "zoomInAction:", "1005": "zoomOutAction:",
        "1016": "deselect:", "1017": "selectAll:", "1018": "invertSelection:",
        "1019": "repeatLastFilter:", "1030": "resizeImage:", "1031": "showCanvasSize:",
        "1036": "featherSelection:", "1040": "pasteInto:", "1042": "fillSelection:",
        "1055": "showInfo:", "1098": "toggleLayersPanel:", "1099": "newLayer:",
        "1105": "toggleProofColors:", "1106": "toggleGamutWarning:", "1107": "copyMerged:",
        "1115": "refineSelection:", "1137": "showFileInfo:", "1139": "mergeVisible:",
        "1154": "showFade:", "1166": "mergeDown:", "1170": "showActions:",
        "1190": "zoomActualAction:", "1192": "zoomFitAction:", "1297": "pasteInPlace:",
        "1701": "applyInvert:", "1801": "showLevels:", "1802": "showCurves:",
        "1804": "showAdjustmentSheet:#3", "1805": "showAdjustmentSheet:#2", "1808": "autoTone:",
        "1809": "applyDesaturate:", "1810": "autoContrast:", "1817": "autoColor:",
        "1824": "showAdjustmentSheet:#4", "1943": "reselect:", "2101": "printDocument:",
        "2207": "freeTransform:", "2217": "transformAgain:", "2344": "showColorSettings:",
        "2711": "bringToFront:", "2712": "bringForward:", "2713": "sendBackward:",
        "2714": "sendToBack:", "2940": "toggleLockGuides:", "2957": "lockAll:",
        "2958": "groupLayers:", "2959": "ungroupLayers:", "2970": "layerViaCopy:",
        "2971": "layerViaCut:", "2972": "toggleClippingMask:", "3443": "showExportWorkspace:",
        "3446": "quickExport:", "3503": "toggleGuides:", "3504": "toggleGrid:",
        "3520": "toggleSnap:", "5957": "showCommandPalette:",
    ]

    /// Dynamic command `name` → `ShortcutMenu.identity`.
    static let dynamicCommands: [String: String] = [
        "Liquify": "showLiquify:", "Lens Correction": "showLensCorrection:",
        "Camera Raw Filter": "showDevelopFilter:",
    ]

    /// A type-"1" tool's `key` (its four-character code as a decimal) → `EditorTool.identifier`.
    static let tools: [String: String] = [
        "1819113074": "move", "1919380852": "select", "1701604724": "ellipseSelect",
        "1818325871": "freehandLasso", "1885826926": "lasso", "1835819379": "magneticLasso",
        "1902867308": "quickSelect", "2002873956": "wand", "1835494497": "objectSelect",
        "1702454628": "eyedropper", "1668246643": "colorSampler", "1835360596": "ruler",
        "1668444016": "crop", "1885565552": "perspectiveCrop", "1936746562": "spotHeal",
        "1937010029": "heal", "1886675816": "patch", "1919118704": "contentAwareMove",
        "1919182201": "redEye", "1886286946": "brush", "1885695587": "pencil",
        "1937006957": "clone", "1752396866": "historyBrush", "1634230900": "artHistoryBrush",
        "1701994867": "eraser", "1936028257": "backgroundEraser", "1835364961": "magicEraser",
        "1651401812": "gradient", "1651860331": "fill", "1651275122": "blurBrush",
        "1936220530": "sharpenBrush", "1936553316": "smudge", "1685021799": "dodge",
        "1935766645": "sponge", "1885695572": "pen", "1954038392": "text",
        "1885565780": "pathSelect", "1886677089": "directSelect", "1382376308": "shapeRect",
        "1164734579": "shapeEllipse", "1818848852": "shapeLine", "1751215716": "hand",
        "2054123373": "zoom",
    ]

    private static let notShortcutFile = "This is not a keyboard shortcut (.kys) file."

    // MARK: - Reading

    /// A bounded read of a regular file, then `entries(_:)`.
    static func entries(contentsOf url: URL) throws -> [KysEntry] {
        let file = url.lastPathComponent
        let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard values?.isRegularFile == true else {
            throw DoctoredCoreError(message: "“\(file)” is not a readable file.")
        }
        guard (values?.fileSize ?? 0) <= maximumBytes else { throw tooLarge() }
        let handle: FileHandle
        do {
            handle = try FileHandle(forReadingFrom: url)
        } catch {
            throw DoctoredCoreError(message: "“\(file)” could not be read.")
        }
        defer { try? handle.close() }
        // An empty file reads as nil here and is refused as not a `.kys` file.
        let data = (try? handle.read(upToCount: maximumBytes + 1)) ?? Data()
        return try entries(data)
    }

    private static func tooLarge() -> DoctoredCoreError {
        DoctoredCoreError(message: "This .kys file is larger than 1 MiB.")
    }

    /// The root's child elements in file order. The bounds come first (bytes, then elements);
    /// external entities are never loaded.
    static func entries(_ data: Data) throws -> [KysEntry] {
        guard data.count <= maximumBytes else { throw tooLarge() }
        guard let document = try? XMLDocument(data: data, options: [.nodeLoadExternalEntitiesNever]),
              let root = document.rootElement() else {
            throw DoctoredCoreError(message: notShortcutFile)
        }
        // Counted with a stack, not recursion: the parser bounds the depth, this does not rely on it.
        var stack: [XMLNode] = [root]
        var elements = 0
        while let node = stack.popLast() {
            elements += 1
            guard elements <= maximumEntries else {
                throw DoctoredCoreError(
                    message: "This .kys file holds more than \(maximumEntries) elements.")
            }
            stack.append(contentsOf: (node.children ?? []).filter { $0.kind == .element })
        }
        guard root.name == "photoshop-keyboard-shortcuts" else {
            throw DoctoredCoreError(message: notShortcutFile)
        }
        guard let version = root.attribute(forName: "version")?.stringValue else {
            throw DoctoredCoreError(
                message: "This .kys file names no version; this import reads version \(readVersion).")
        }
        guard version == readVersion else {
            throw DoctoredCoreError(
                message: "This .kys file is version “\(version)”; this import reads version \(readVersion).")
        }
        return (root.children ?? []).compactMap { $0 as? XMLElement }.map(entry)
    }

    private static func entry(_ element: XMLElement) -> KysEntry {
        func attribute(_ name: String) -> String? { element.attribute(forName: name)?.stringValue }
        func trimmed(_ node: XMLNode) -> String {
            (node.stringValue ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        }
        let name = reportName(element)
        switch element.name {
        case "command":
            return .command(kind: attribute("kind") ?? "", id: attribute("id"), name: name,
                            shortcuts: element.elements(forName: "shortcut").map(trimmed))
        case "tool":
            return .tool(type: attribute("type") ?? "", key: attribute("key"), name: name,
                         letter: trimmed(element))
        default:
            return .other(name: name)
        }
    }

    /// The `name` attribute, else the element's name with its `id` or `key`; 200 characters at
    /// most.
    private static func reportName(_ element: XMLElement) -> String {
        if let name = element.attribute(forName: "name")?.stringValue, !name.isEmpty {
            return String(name.prefix(200))
        }
        let tag = element.name ?? "element"
        let code = element.attribute(forName: "id")?.stringValue
            ?? element.attribute(forName: "key")?.stringValue
        return String((code.map { "\(tag) \($0)" } ?? tag).prefix(200))
    }

    /// "Opt+Shift+Cmd+K" → ⌥⇧⌘K, not yet `validated()`. The leading `Cmd+`, `Opt+`, `Shift+` and
    /// `Control+` tokens are modifiers while they match; what remains is the key: one character
    /// (a letter LOWERCASED, since the file spells letters in capitals and writes Shift out) or
    /// F1–F19. `Cmd++` is ⌘ and "+".
    static func chord(_ text: String) throws -> ShortcutChord {
        let tokens: [(token: String, flag: NSEvent.ModifierFlags)] = [
            ("Cmd+", .command), ("Opt+", .option), ("Shift+", .shift), ("Control+", .control),
        ]
        var rest = Substring(text)
        var flags: NSEvent.ModifierFlags = []
        while let match = tokens.first(where: { rest.hasPrefix($0.token) }) {
            flags.insert(match.flag)
            rest = rest.dropFirst(match.token.count)
        }
        if rest.count == 1 {
            return ShortcutChord(key: rest.lowercased(), modifiers: flags)
        }
        let digits = rest.dropFirst()
        if rest.hasPrefix("F"), let number = Int(digits), String(number) == digits,
           (1...19).contains(number), let scalar = Unicode.Scalar(0xF704 + number - 1) {
            return ShortcutChord(key: String(Character(scalar)), modifiers: flags)
        }
        throw DoctoredCoreError(message: "“\(text)” is not a shortcut this import reads.")
    }

    // MARK: - Mapping

    /// The set `entries` make under `name`, and the report. Walked in file order against the
    /// customizable items of `menu` and its fixed chords (`ShortcutMenu.fixedChords`): a chord
    /// another command holds there, or an earlier entry took, is refused with `assign`'s sentence,
    /// and a chord or key equal to this build's default is applied but not stored.
    ///
    /// **Default holders.** `apply` withholds a customizable default that an override holds, so
    /// an imported chord equal to the default of a command the file leaves alone would strip that
    /// command silently. After the walk, each such override is refused instead, which puts its own
    /// identity's default back — so the check repeats until a pass refuses nothing. A command the
    /// file rebinds or removes holds no default, which is why a swap within one file is kept.
    static func bindings(
        _ entries: [KysEntry], name: String, menu: NSMenu
    ) -> (bindings: ShortcutBindings, report: KysImportReport) {
        let items = ShortcutMenu.customizableItems(in: menu)
        var byIdentity: [String: ShortcutMenu.Item] = [:]
        for item in items where byIdentity[item.identity] == nil { byIdentity[item.identity] = item }
        let fixed = ShortcutMenu.fixedChords(in: menu)
        var result = ShortcutBindings(name: name)
        var report = KysImportReport()
        // Applied entries in file order, each with the override identity it stored (nil for a
        // tool, a removal, or a chord equal to the default), so the holder pass can withdraw one.
        var applied: [(name: String, identity: String?)] = []
        var seen: Set<String> = []
        var taken: [ShortcutChord: String] = [:]

        func refuse(_ entry: String, _ error: Error) {
            let sentence = (error as? DoctoredCoreError)?.message ?? error.localizedDescription
            report.refused.append("\(entry): \(sentence)")
        }

        for entry in entries {
            switch entry {
            case .other(let entryName):
                report.unmapped.append(entryName)
            case .command(let kind, let id, let entryName, let shortcuts):
                let identity: String?
                switch kind {
                case "static": identity = id.flatMap { staticCommands[$0] }
                case "dynamic": identity = dynamicCommands[entryName]
                default: identity = nil
                }
                guard let identity, let item = byIdentity[identity] else {
                    report.unmapped.append(entryName)
                    continue
                }
                guard seen.insert("command " + identity).inserted else {
                    refuse(entryName, DoctoredCoreError(message: "listed twice; the first is kept."))
                    continue
                }
                guard let first = shortcuts.first else {
                    let stored = item.defaultChord != nil
                    if stored { result.overrides.updateValue(nil, forKey: identity) }
                    applied.append((entryName, stored ? identity : nil))
                    continue
                }
                do {
                    let parsed = try Self.chord(first).validated()
                    if let holder = fixed[parsed], holder.identity != identity {
                        throw DoctoredCoreError(message: "\(parsed.display) is already \(holder.path).")
                    }
                    if let path = taken[parsed] {
                        throw DoctoredCoreError(message: "\(parsed.display) is already \(path).")
                    }
                    taken[parsed] = item.path
                    let stored = parsed != item.defaultChord
                    if stored { result.overrides.updateValue(parsed, forKey: identity) }
                    applied.append((entryName, stored ? identity : nil))
                } catch {
                    refuse(entryName, error)
                }
                for extra in shortcuts.dropFirst() {
                    refuse(entryName, DoctoredCoreError(
                        message: "“\(extra)” is a second shortcut; a command takes one here."))
                }
            case .tool(let type, let key, let entryName, let letter):
                guard type == "1", let identifier = key.flatMap({ tools[$0] }),
                      let tool = EditorTool.allCases.first(where: { $0.identifier == identifier }) else {
                    report.unmapped.append(entryName)
                    continue
                }
                guard seen.insert("tool " + identifier).inserted else {
                    refuse(entryName, DoctoredCoreError(message: "listed twice; the first is kept."))
                    continue
                }
                do {
                    let value: String? = try letter.isEmpty ? nil : ToolKeys.validated(letter)
                    if value != tool.defaultKeyCharacter {
                        result.toolKeys.updateValue(value, forKey: identifier)
                    }
                    applied.append((entryName, nil))
                } catch {
                    refuse(entryName, error)
                }
            }
        }

        // The default holders, to a fixed point.
        var withdrew = true
        while withdrew {
            withdrew = false
            var holders: [ShortcutChord: ShortcutMenu.Item] = [:]
            for item in items where result.overrides.index(forKey: item.identity) == nil {
                if let chord = item.defaultChord, holders[chord] == nil { holders[chord] = item }
            }
            var kept: [(name: String, identity: String?)] = []
            for entry in applied {
                guard let identity = entry.identity, let stored = result.overrides[identity],
                      let chord = stored, let holder = holders[chord],
                      holder.identity != identity else {
                    kept.append(entry)
                    continue
                }
                result.overrides.removeValue(forKey: identity)
                report.refused.append("\(entry.name): \(chord.display) is already \(holder.path).")
                withdrew = true
            }
            applied = kept
        }
        report.applied = applied.map(\.name)
        return (result, report)
    }
}
