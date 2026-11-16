import AppKit

// "action parameter rows" (#20): the Actions window's Parameters… sheet reads each row through
// `ActionParametersEditorSheetController.edits(for:)` on Save. A row left as the sheet filled it
// asks for nothing, a string default with outer spaces included; a string default is kept as
// typed and the other types parse their trimmed text; a changed default, type, description or
// name asks for its own edit, a removed row for its removal alone and the add row for one add
// when named; a default that is not of its type is refused by sentence.

func verifyActionParameterRows() throws {
    let tag = "[action parameter rows]"
    typealias Sheet = ActionParametersEditorSheetController
    let label = ActionParameter(name: "label", type: .string, default: " a", description: "Label")
    let count = ActionParameter(name: "count", type: .integer, default: 5, description: nil)
    let amount = ActionParameter(name: "amount", type: .number, default: 0.5, description: nil)
    let title = ActionParameter(name: "title", type: .string, default: nil, description: nil)

    func describe(_ edit: ActionParameterEdit) -> String {
        switch edit {
        case .add(let name, let type, let value, _):
            return "add \(name) \(type.rawValue) [\(value.map { "\($0)" } ?? "nil")]"
        case .rename(let from, let to): return "rename \(from) \(to)"
        case .retype(let name, let type, _): return "retype \(name) \(type.rawValue)"
        case .setDefault(let name, let value):
            return "setDefault \(name) [\(value.map { "\($0)" } ?? "nil")]"
        case .setDescription(let name, let text): return "setDescription \(name) \(text ?? "nil")"
        case .remove(let name): return "remove \(name)"
        }
    }
    // A row as Save reads it, every field as the sheet filled it unless named.
    func edits(_ original: ActionParameter?, name: String? = nil,
               type: ActionParameter.ParameterType? = nil, value: String,
               description: String? = nil, remove: Bool = false) -> [String] {
        let fields = Sheet.Fields(
            original: original, name: name ?? original?.name ?? "",
            type: type ?? original?.type ?? .string, value: value,
            description: description ?? original?.description ?? "", remove: remove)
        switch Sheet.edits(for: fields) {
        case .success(let list): return list.map(describe)
        case .failure(let error): return ["refused: \(error.message)"]
        }
    }

    let rows: [(String, [String], [String])] = [
        ("an untouched spaced string default", [], edits(label, value: " a")),
        ("a description edit beside a spaced default", ["setDescription label Name"],
         edits(label, value: " a", description: "Name")),
        ("a string default typed with spaces", ["setDefault label [ b ]"],
         edits(label, value: " b ")),
        ("an untouched required string", [], edits(title, value: "")),
        ("an integer typed with spaces", [], edits(count, value: " 5 ")),
        ("a changed integer", ["setDefault count [6]"], edits(count, value: "6")),
        ("a cleared integer", ["setDefault count [nil]"], edits(count, value: " ")),
        ("an untouched number", [], edits(amount, value: "0.5")),
        ("a malformed integer", ["refused: “5.5” is not an integer."], edits(count, value: "5.5")),
        ("a retype and a rename", ["retype label integer", "rename label caption"],
         edits(label, name: "caption", type: .integer, value: "3")),
        ("a removed row", ["remove label"],
         edits(label, value: "x", description: "y", remove: true)),
        ("an empty add row", [], edits(nil, value: "")),
        ("a named add row", ["add extra boolean [true]"],
         edits(nil, name: " extra ", type: .boolean, value: "TRUE")),
    ]
    for (what, expected, got) in rows {
        try require(got == expected, "\(tag) \(what): \(got), expected \(expected)")
    }
    print("PASS: \(tag) an untouched row asks for nothing (a spaced string default too), a string "
          + "default keeps its spaces, the other types parse trimmed text, each changed field asks "
          + "for its own edit and a malformed default is refused")
}
