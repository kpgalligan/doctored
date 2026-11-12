import AppKit

// The plan run.py writes (`DR_SCREENS_PLAN`): the screens to capture, already read from the screen
// files and checked against the fixture table, and the ones skipped. Every name a screen uses is
// looked up here, before any window exists, in the probe's tables: window kinds, cards and panels
// (windows.swift), sheets and views (editor_tables.swift) and UI actions (actions.swift). A misspelt name
// fails the run at once instead of halfway through a capture, and a new name is one entry in its
// table: nothing in this file changes.

enum Look: String {
    case light, dark

    var appearanceName: NSAppearance.Name { self == .light ? .aqua : .darkAqua }
}

enum Mechanism: String { case server, cache }

/// Where a UI action may run, checked when the plan is parsed.
enum Scope {
    /// On the screen's editor, while no card runs.
    case editor
    /// On the open dialog: the editor window's attached sheet, or the running card.
    case dialog
    /// Inside a running card only.
    case card
    /// Anywhere, the plain windows included.
    case any

    /// Where it runs, for the plan's refusal.
    var rule: String {
        switch self {
        case .editor: return "on an editor, before any card"
        case .dialog: return "on an editor's open sheet or in a card"
        case .card: return "inside a card"
        case .any: return "anywhere"
        }
    }
}

/// One UI action of the `uiActions` table: its scope, its exact argument keys, and the parser that
/// turns a step's arguments into the closure the screen runs.
struct ActionSpec {
    let scope: Scope
    let keys: Set<String>
    let parse: (Arguments) throws -> (Context) throws -> Void

    init(_ scope: Scope, _ keys: Set<String>,
         _ parse: @escaping (Arguments) throws -> (Context) throws -> Void) {
        self.scope = scope
        self.keys = keys
        self.parse = parse
    }
}

enum Step {
    case tool(name: String, arguments: [String: Any])
    case ui(name: String, run: (Context) throws -> Void)

    var label: String {
        switch self {
        case .tool(let name, _): return "tool \(name)"
        case .ui(let name, _): return "ui \(name)"
        }
    }
}

enum CaptureTarget {
    /// The screen's own window: the editor's, the plain window, or a card host's card.
    case window
    /// A view of the editor window (`editorViews`), cut from the window image.
    case view(String)
    /// The editor window's attached sheet, cut from the window image.
    case sheet
    /// A window of the `panels` table, under its own window number.
    case panel(String)
    /// The running card, after selecting `tab` in its tab view when one is named.
    case card(tab: String?)
    /// The NSAlert the steps put up, taken while it runs.
    case alert

    /// The manifest's `of`.
    var of: String {
        switch self {
        case .window: return "window"
        case .view: return "view"
        case .sheet: return "sheet"
        case .panel: return "panel"
        case .card: return "card"
        case .alert: return "alert"
        }
    }
}

struct CaptureSpec {
    let name: String
    let target: CaptureTarget

    var of: String { target.of }
}

struct Fixture {
    /// The fixture table's id, for the manifest's `fixtures` list.
    let id: String
    let path: URL
}

/// A card the screen runs, and the steps that run inside it.
struct CardRun {
    let name: String
    let spec: CardSpec
    let steps: [(index: Int, step: Step)]
}

struct Screen {
    let id: String
    let kindName: String
    let kind: WindowKind
    let fixture: Fixture?
    /// The steps run before any card, each with its 1-based position in the screen file.
    let steps: [(index: Int, step: Step)]
    /// The card `run_modal_card` opens, or a card host's own card.
    let card: CardRun?
    let captures: [CaptureSpec]
}

struct PlanError: Error, CustomStringConvertible { let description: String }

private func object(_ value: Any?, _ context: String) throws -> [String: Any] {
    guard let object = value as? [String: Any] else { throw PlanError(description: "\(context) is not an object") }
    return object
}

/// A step's `args` (or a capture's keys), read under `context` for messages.
struct Arguments {
    let values: [String: Any]
    let context: String

    func fail(_ message: String) -> PlanError { PlanError(description: "\(context): \(message)") }

    /// Refuses keys other than exactly `keys`.
    func exactKeys(_ keys: Set<String>) throws {
        let given = Set(values.keys)
        guard given == keys else { throw fail("keys are \(given.sorted()); expected \(keys.sorted())") }
    }

    func string(_ key: String) throws -> String {
        guard let value = values[key] as? String, !value.isEmpty else {
            throw fail("\"\(key)\" is not a non-empty string")
        }
        return value
    }

    /// A number (never a JSON bool) in `range`.
    func number(_ key: String, in range: ClosedRange<Double>) throws -> Double {
        guard let value = values[key] as? NSNumber, !isBool(value), range.contains(value.doubleValue) else {
            throw fail("\"\(key)\" is not a number in \(range)")
        }
        return value.doubleValue
    }

    /// `[x, y, w, h]` in canvas pixels, with a positive width and height.
    func rect(_ key: String) throws -> CGRect {
        let v = try numbers(key, count: 4)
        guard v[2] > 0, v[3] > 0 else { throw fail("\"\(key)\" has no area") }
        return CGRect(x: v[0], y: v[1], width: v[2], height: v[3])
    }

    /// `[x, y]`.
    func point(_ key: String) throws -> CGPoint {
        let v = try numbers(key, count: 2)
        return CGPoint(x: v[0], y: v[1])
    }

    /// A list of `[x, y]` pairs.
    func pairs(_ key: String) throws -> [CGPoint] {
        guard let list = values[key] as? [Any], !list.isEmpty else { throw fail("\"\(key)\" is not a list of [x, y]") }
        return try list.map { entry in
            guard let pair = entry as? [NSNumber], pair.count == 2, !pair.contains(where: isBool) else {
                throw fail("\"\(key)\" holds an entry that is not [x, y]")
            }
            return CGPoint(x: pair[0].doubleValue, y: pair[1].doubleValue)
        }
    }

    /// A string, number or bool, for a control whose type is known only when it is found.
    func scalar(_ key: String) throws -> ControlValue {
        switch values[key] {
        case let text as String: return .text(text)
        case let number as NSNumber where isBool(number): return .flag(number.boolValue)
        case let number as NSNumber: return .number(number.doubleValue)
        default: throw fail("\"\(key)\" is not a string, number or bool")
        }
    }

    private func numbers(_ key: String, count: Int) throws -> [CGFloat] {
        guard let numbers = values[key] as? [NSNumber], numbers.count == count, !numbers.contains(where: isBool) else {
            throw fail("\"\(key)\" is not a list of \(count) numbers")
        }
        return numbers.map { CGFloat($0.doubleValue) }
    }
}

/// A value `set_control` gives a control.
enum ControlValue: CustomStringConvertible {
    case text(String), number(Double), flag(Bool)

    var description: String {
        switch self {
        case .text(let text): return "\"\(text)\""
        case .number(let number): return "\(number)"
        case .flag(let flag): return "\(flag)"
        }
    }
}

private func isBool(_ number: NSNumber) -> Bool { CFGetTypeID(number) == CFBooleanGetTypeID() }

/// Parses one step. `inCard` says whether a card runs at this point of the screen.
private func step(_ value: Any?, _ context: String, host: Host, inCard: Bool) throws -> Step {
    let entry = try object(value, context)
    let arguments = try object(entry["args"] ?? [String: Any](), "\(context) args")
    switch (entry["tool"], entry["ui"]) {
    case (let tool as String, nil):
        guard Set(entry.keys).isSubset(of: ["tool", "args"]), !tool.isEmpty else {
            throw PlanError(description: "\(context): a tool step holds only \"tool\" and \"args\"")
        }
        guard case .editor = host, !inCard else {
            throw PlanError(description: "\(context): a tool step runs only on an editor, before any card")
        }
        guard arguments["document_id"] == nil else {
            throw PlanError(description: "\(context): \(tool) names document_id; the probe injects it")
        }
        return .tool(name: tool, arguments: arguments)
    case (nil, let name as String):
        guard Set(entry.keys).isSubset(of: ["ui", "args"]) else {
            throw PlanError(description: "\(context): a ui step holds only \"ui\" and \"args\"")
        }
        let context = "\(context) ui \(name)"
        guard let action = uiActions[name] else { throw PlanError(description: "\(context): unknown UI action") }
        let allowed: Bool
        switch (action.scope, host) {
        case (.any, _): allowed = true
        case (.editor, .editor): allowed = !inCard
        case (.dialog, .editor): allowed = true
        case (.dialog, .card), (.card, .card), (.card, .editor): allowed = inCard
        default: allowed = false
        }
        guard allowed else { throw PlanError(description: "\(context): runs only \(action.scope.rule)") }
        let reader = Arguments(values: arguments, context: context)
        try reader.exactKeys(action.keys)
        return .ui(name: name, run: try action.parse(reader))
    default:
        throw PlanError(description: "\(context): a step names exactly one of \"tool\" or \"ui\"")
    }
}

private func capture(_ value: Any?, _ context: String, host: Host, hasCard: Bool) throws -> CaptureSpec {
    let entry = Arguments(values: try object(value, context), context: context)
    let name = try entry.string("name")
    let of = try entry.string("of")
    let target: CaptureTarget
    switch of {
    case "window":
        try entry.exactKeys(["name", "of"])
        target = .window
    case "view":
        try entry.exactKeys(["name", "of", "view"])
        let view = try entry.string("view")
        guard editorViews[view] != nil else { throw entry.fail("no view \"\(view)\"") }
        target = .view(view)
    case "sheet":
        try entry.exactKeys(["name", "of"])
        target = .sheet
    case "panel":
        try entry.exactKeys(["name", "of", "panel"])
        let panel = try entry.string("panel")
        guard panels[panel] != nil else { throw entry.fail("no panel \"\(panel)\"") }
        target = .panel(panel)
    case "card":
        try entry.exactKeys(entry.values["tab"] == nil ? ["name", "of"] : ["name", "of", "tab"])
        target = .card(tab: entry.values["tab"] == nil ? nil : try entry.string("tab"))
    case "alert":
        try entry.exactKeys(["name", "of"])
        target = .alert
    default:
        throw entry.fail("unknown capture kind \"\(of)\"")
    }
    guard host.admits(target, hasCard: hasCard) else {
        throw entry.fail("this window has no \(of) to capture\(of == "card" ? " (no run_modal_card)" : "")")
    }
    return CaptureSpec(name: name, target: target)
}

private func string(_ object: [String: Any], _ key: String, _ context: String) throws -> String {
    try Arguments(values: object, context: context).string(key)
}

/// The plan's screens and skip records. Throws `PlanError` on any name outside the tables, a
/// step outside its scope, a capture its window cannot take, or a fixture its window cannot open.
func parsePlan(_ data: Data) throws -> (screens: [Screen], skipped: [[String: String]]) {
    let root = try object(try JSONSerialization.jsonObject(with: data), "the plan")
    guard let entries = root["screens"] as? [Any], let skippedEntries = root["skipped"] as? [Any] else {
        throw PlanError(description: "the plan has no \"screens\" and \"skipped\" lists")
    }
    let skipped = try skippedEntries.enumerated().map { index, value -> [String: String] in
        let entry = try object(value, "skipped[\(index)]")
        return ["screen": try string(entry, "screen", "skipped[\(index)]"),
                "reason": try string(entry, "reason", "skipped[\(index)]")]
    }
    var seen = Set<String>()
    let screens = try entries.enumerated().map { index, value -> Screen in
        let entry = try object(value, "screens[\(index)]")
        let id = try string(entry, "id", "screens[\(index)]")
        let context = "screen \(id)"
        guard seen.insert(id).inserted else { throw PlanError(description: "\(context) appears twice") }
        let kindName = try string(entry, "window", context)
        guard let kind = windowKinds[kindName] else {
            throw PlanError(description: "\(context): unknown window \"\(kindName)\"")
        }
        let fixture = try parseFixture(entry["fixture"], kind: kind, kindName: kindName, context)
        guard let stepEntries = entry["steps"] as? [Any], let captureEntries = entry["captures"] as? [Any] else {
            throw PlanError(description: "\(context): \"steps\" and \"captures\" are not lists")
        }
        var steps: [(index: Int, step: Step)] = []
        var card: (name: String, spec: CardSpec)?
        var cardSteps: [(index: Int, step: Step)] = []
        if case .card(let spec) = kind.host { card = (kindName, spec) }
        for (offset, value) in stepEntries.enumerated() {
            let stepContext = "\(context) step \(offset + 1)"
            if let entry = value as? [String: Any], entry["ui"] as? String == "run_modal_card" {
                card = try parseCardStep(entry, kind: kind, existing: card, stepContext)
                continue
            }
            let parsed = try step(value, stepContext, host: kind.host, inCard: card != nil)
            if card == nil { steps.append((offset + 1, parsed)) } else { cardSteps.append((offset + 1, parsed)) }
        }
        let captures = try captureEntries.enumerated().map {
            try capture($1, "\(context) capture \($0 + 1)", host: kind.host, hasCard: card != nil)
        }
        guard !captures.isEmpty else { throw PlanError(description: "\(context) has no captures") }
        guard Set(captures.map(\.name)).count == captures.count else {
            throw PlanError(description: "\(context): two captures share a name")
        }
        return Screen(id: id, kindName: kindName, kind: kind, fixture: fixture, steps: steps,
                      card: card.map { CardRun(name: $0.name, spec: $0.spec, steps: cardSteps) },
                      captures: captures)
    }
    return (screens, skipped)
}

/// `{"ui": "run_modal_card", "args": {"card": <name>}}`: once per screen, on an editor.
private func parseCardStep(_ entry: [String: Any], kind: WindowKind, existing: (name: String, spec: CardSpec)?,
                           _ context: String) throws -> (name: String, spec: CardSpec) {
    let context = "\(context) ui run_modal_card"
    guard Set(entry.keys).isSubset(of: ["ui", "args"]) else {
        throw PlanError(description: "\(context): a ui step holds only \"ui\" and \"args\"")
    }
    guard case .editor = kind.host, existing == nil else {
        throw PlanError(description: "\(context): a card opens once, from an editor")
    }
    let arguments = Arguments(values: try object(entry["args"] ?? [String: Any](), "\(context) args"),
                              context: context)
    try arguments.exactKeys(["card"])
    let name = try arguments.string("card")
    guard let spec = cards[name] else { throw arguments.fail("no card \"\(name)\"") }
    return (name, spec)
}

/// The screen's fixture: `{"id", "path", "kind"}` of the kind its window opens, or null for a
/// window that opens none.
private func parseFixture(_ value: Any?, kind: WindowKind, kindName: String, _ context: String) throws -> Fixture? {
    guard let wanted = kind.fixture else {
        guard value == nil || value is NSNull else {
            throw PlanError(description: "\(context): a \(kindName) window opens no fixture")
        }
        return nil
    }
    let fixture = try object(value, "\(context) fixture")
    let fixtureID = try string(fixture, "id", "\(context) fixture")
    let path = try string(fixture, "path", "\(context) fixture")
    let fixtureKind = try string(fixture, "kind", "\(context) fixture")
    guard path.hasPrefix("/") else { throw PlanError(description: "\(context): the fixture path is not absolute") }
    guard fixtureKind == wanted else {
        throw PlanError(description: "\(context): a \(kindName) window opens a \(wanted) fixture, not \(fixtureKind)")
    }
    return Fixture(id: fixtureID, path: URL(fileURLWithPath: path))
}
