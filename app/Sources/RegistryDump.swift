#if DEBUG
import AppKit

/// Debug-only capture of the installed menu tree and the nine tool-name sets,
/// so a refactor that moves menu builders or set tables can be compared
/// before and after. With `DR_REGISTRY_DUMP=<path>` the app writes the text to
/// that path at launch and exits 0.
enum RegistryDump {
    static let environmentKey = "DR_REGISTRY_DUMP"

    static func writeIfRequested() {
        guard let path = ProcessInfo.processInfo.environment[environmentKey], !path.isEmpty else {
            return
        }
        do {
            try Data(text().utf8).write(to: URL(fileURLWithPath: path), options: .atomic)
        } catch {
            FileHandle.standardError.write(Data("[RegistryDump] \(error)\n".utf8))
            exit(EXIT_FAILURE)
        }
        exit(0)
    }

    static func text() -> String {
        var lines: [String] = ["# menu"]
        if let mainMenu = NSApp.mainMenu {
            walk(mainMenu, depth: 0, into: &lines)
        }
        let sets: [(String, Set<String>)] = [
            ("ActionCatalogFacts.repeatableTools", ActionCatalogFacts.repeatableTools),
            (
                "ActionCatalogFacts.targetedRepeatableTools",
                ActionCatalogFacts.targetedRepeatableTools
            ),
            ("ActionCatalogFacts.nonRecordable", ActionCatalogFacts.nonRecordable),
            ("ActionCatalogFacts.selectionSensitive", ActionCatalogFacts.selectionSensitive),
            ("DocumentPrecision.commonTools", DocumentPrecision.commonTools),
            ("DocumentPrecision.documentFreeTools", DocumentPrecision.documentFreeTools),
            ("DocumentPrecision.typedTools", DocumentPrecision.typedTools),
            ("DocumentPrecision.rasterTools", DocumentPrecision.rasterTools),
            ("FloatDocument.tools", FloatDocument.tools),
        ]
        for (name, members) in sets {
            lines.append("# set \(name) count=\(members.count)")
            for member in members.sorted() {
                lines.append("  \(member)")
            }
        }
        return lines.joined(separator: "\n") + "\n"
    }

    private static func walk(_ menu: NSMenu, depth: Int, into lines: inout [String]) {
        let delegate = menu.delegate.map { String(describing: type(of: $0)) } ?? "-"
        lines.append(
            indent(depth) + "menu \(quoted(menu.title)) delegate=\(delegate)"
                + " autoenables=\(flag(menu.autoenablesItems))"
                + " windows=\(flag(menu === NSApp.windowsMenu))"
                + " help=\(flag(menu === NSApp.helpMenu))")
        for item in menu.items {
            if item.isSeparatorItem {
                lines.append(indent(depth + 1) + "--- hidden=\(flag(item.isHidden))")
                continue
            }
            let action = item.action.map { NSStringFromSelector($0) } ?? "-"
            let target = item.target.map { String(describing: type(of: $0)) } ?? "-"
            let tip = item.toolTip.map { quoted($0) } ?? "-"
            lines.append(
                indent(depth + 1) + "item \(quoted(item.title))"
                    + " key=\(quoted(item.keyEquivalent))"
                    + " mods=\(item.keyEquivalentModifierMask.rawValue)"
                    + " action=\(action) tag=\(item.tag) target=\(target)"
                    + " hidden=\(flag(item.isHidden)) alt=\(flag(item.isAlternate))"
                    + " whenHidden=\(flag(item.allowsKeyEquivalentWhenHidden))"
                    + " rep=\(flag(item.representedObject != nil))"
                    + " state=\(item.state.rawValue) enabled=\(flag(item.isEnabled))"
                    + " tip=\(tip)")
            if let submenu = item.submenu {
                walk(submenu, depth: depth + 2, into: &lines)
            }
        }
    }

    private static func indent(_ depth: Int) -> String {
        String(repeating: "  ", count: depth)
    }

    private static func flag(_ value: Bool) -> String {
        value ? "1" : "0"
    }

    private static func quoted(_ value: String) -> String {
        var out = "\""
        for scalar in value.unicodeScalars {
            switch scalar {
            case "\\": out += "\\\\"
            case "\"": out += "\\\""
            default:
                let v = scalar.value
                if v < 0x20 || v == 0x7F || (0xF700...0xF8FF).contains(v) {
                    out += String(format: "\\u{%04X}", v)
                } else {
                    out.unicodeScalars.append(scalar)
                }
            }
        }
        return out + "\""
    }
}
#endif
