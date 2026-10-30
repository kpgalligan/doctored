import Foundation

// `ActionParameters.bind` is the ONE validator every Play path shares (the window, Play Last,
// `run_action`, `run_batch`, the Batch window), and it refuses at SUBMISSION so a batch never turns
// one parameter error into 200 rows.

/// An action's declared parameters: what a caller may supply, and the one
/// validator every path that supplies them goes through.
///
/// **Why an action needs them.** A recorded step is literal — a Gaussian Blur
/// at sigma 4, a fill in `#204080`. Reusing that action at sigma 8 meant
/// copying the file and editing it, which forks the action and then the two
/// drift. A parameter names one value the caller chooses; `{"$param": "sigma"}`
/// stands in the step where the literal was.
///
/// **Declaring one is an EDIT, never a recording.** The recorder writes what
/// happened, in the values it happened at; deciding that sigma is the knob and
/// the colour is not is a statement about how the action will be reused. So a
/// recording always declares none, and the JSON editor (or `save_action`) is
/// where they arrive.
///
/// **`bind` is the single validator, and it runs BEFORE anything runs.** Every
/// path that plays an action with parameters — the Actions window's Play, File
/// ▸ Automate ▸ Play Last, `run_action`, `run_batch` and the Batch window —
/// calls it and refuses as a whole rather than discovering a missing value at
/// step 7 with the picture already changed. In a batch that matters most: a
/// parameter error is an ACTION-level refusal at submission, so it is one
/// message before the first file is opened rather than 200 identical rows.
struct ActionParameter {
    /// What a supplied value must be. Four types, deliberately: they are the
    /// JSON primitives a tool argument can be, and an "object" or "array"
    /// parameter would be a way to smuggle a whole unvalidated argument tree
    /// into a step.
    enum ParameterType: String, CaseIterable {
        case number
        case integer
        case string
        case boolean
    }

    /// `^[A-Za-z_][A-Za-z0-9_]*$`, unique within the action.
    let name: String
    let type: ParameterType
    /// The value used when the caller supplies none. Its absence is what
    /// makes a parameter REQUIRED.
    let `default`: Any?
    /// One line for the person being asked — the prompt sheet's field label
    /// help, and part of `list_actions`' reply.
    let description: String?

    init(name: String, type: ParameterType, default defaultValue: Any? = nil,
         description: String? = nil) {
        self.name = name
        self.type = type
        self.default = defaultValue
        self.description = description
    }

    /// The file's form.
    func jsonObject() -> [String: Any] {
        var object: [String: Any] = ["name": name, "type": type.rawValue]
        if let value = self.default { object["default"] = value }
        if let description = description { object["description"] = description }
        return object
    }

    /// A name a `{"$param": …}` symbol may carry: a letter or underscore, then
    /// letters, digits and underscores. Narrow on purpose — the name appears
    /// in a symbol, in a prompt's field label and in an MCP argument object,
    /// and anything looser would need three different escapes.
    static func isValidName(_ name: String) -> Bool {
        guard let first = name.unicodeScalars.first else { return false }
        guard CharacterSet.letters.contains(first) || first == "_" else { return false }
        return name.unicodeScalars.allSatisfy {
            CharacterSet.alphanumerics.contains($0) || $0 == "_"
        }
    }

    /// The whole `parameters` array, or the reason it is not one. Absent is
    /// success with none — every schema-1 action takes this branch.
    static func decodeAll(_ value: Any?) -> Result<[ActionParameter], ActionError> {
        guard let value = value, !(value is NSNull) else { return .success([]) }
        guard let rows = value as? [Any] else {
            return .failure(ActionError("parameters must be an array of parameter objects"))
        }
        var out: [ActionParameter] = []
        var seen: Set<String> = []
        for (index, row) in rows.enumerated() {
            let label = "parameters[\(index)]"
            guard let fields = row as? [String: Any] else {
                return .failure(ActionError("\(label) is not an object"))
            }
            guard let name = fields["name"] as? String, isValidName(name) else {
                return .failure(
                    ActionError(
                        "\(label).name must be a letter or underscore followed by letters, "
                            + "digits or underscores"))
            }
            guard seen.insert(name).inserted else {
                return .failure(ActionError("\(label): \"\(name)\" is declared twice"))
            }
            guard let rawType = fields["type"] as? String,
                  let type = ParameterType(rawValue: rawType)
            else {
                return .failure(
                    ActionError(
                        "\(label).type must be one of "
                            + ParameterType.allCases.map { $0.rawValue }
                            .joined(separator: ", ")))
            }
            var defaultValue: Any?
            if let raw = fields["default"], !(raw is NSNull) {
                guard matches(raw, type) else {
                    return .failure(
                        ActionError(
                            "\(label).default is not \(article(type)) \(type.rawValue)"))
                }
                defaultValue = raw
            }
            guard fields["description"] == nil || fields["description"] is NSNull
                    || fields["description"] is String
            else {
                return .failure(ActionError("\(label).description must be a string"))
            }
            out.append(
                ActionParameter(
                    name: name, type: type, default: defaultValue,
                    description: fields["description"] as? String))
        }
        return .success(out)
    }

    /// True when `value` is of this type.
    ///
    /// Booleans and numbers are told apart by `ActionJSON`, never by
    /// `is Bool`: Swift bridges an `NSNumber` holding 0 or 1 to `Bool`, so
    /// `is Bool` would call a supplied `0` a boolean and refuse it as a
    /// number (the trap that helper's own comment measures).
    static func matches(_ value: Any, _ type: ParameterType) -> Bool {
        switch type {
        case .string: return value is String
        case .boolean: return ActionJSON.isBoolean(value)
        case .number:
            guard let number = ActionJSON.number(value) else { return false }
            return number.doubleValue.isFinite
        case .integer:
            guard let number = ActionJSON.number(value) else { return false }
            return Int(exactly: number.doubleValue) != nil
        }
    }

    /// "a" or "an", so a refusal reads like a sentence.
    static func article(_ type: ParameterType) -> String {
        type == .integer ? "an" : "a"
    }
}

/// The one place a supplied set of values is checked against an action's
/// declarations. Every Play path calls it; nothing else validates parameters.
enum ActionParameters {
    /// `supplied` checked against `action`'s declarations, or the ONE sentence
    /// saying what is wrong with it.
    ///
    /// On success the dictionary holds EVERY declared name, defaults filled
    /// in — so a caller downstream (`BatchSettings.parameters`, the player's
    /// `$param` resolution) never has to ask whether a value is present.
    ///
    /// Four refusals, each naming the parameter: an unknown name (a typo that
    /// would otherwise be silently dropped and the default used), a required
    /// one with no value, a value of the wrong type, and — reported first,
    /// because it is the one the caller can act on without reading the
    /// declarations — a value that is not JSON at all.
    /// The failure is an `ActionError` rather than a bare `String` because
    /// `Result` requires its failure to conform to `Error` and this feature
    /// already has exactly one sentence-carrying error type; `.message` is the
    /// sentence, unchanged.
    static func bind(
        _ supplied: [String: Any], to action: Action
    ) -> Result<[String: Any], ActionError> {
        let declared = Dictionary(uniqueKeysWithValues: action.parameters.map { ($0.name, $0) })
        for name in supplied.keys.sorted() where declared[name] == nil {
            guard !action.parameters.isEmpty else {
                return .failure(
                    ActionError(
                        "\"\(action.name)\" declares no parameters, so \"\(name)\" is not "
                            + "one of its arguments."))
            }
            return .failure(
                ActionError(
                    "\"\(action.name)\" has no parameter named \"\(name)\" — it declares "
                        + action.parameters.map { "\"\($0.name)\"" }
                        .joined(separator: ", ") + "."))
        }
        var bound: [String: Any] = [:]
        for parameter in action.parameters {
            if let value = supplied[parameter.name], !(value is NSNull) {
                guard ActionParameter.matches(value, parameter.type) else {
                    return .failure(
                        ActionError(
                            "\"\(parameter.name)\" must be "
                                + "\(ActionParameter.article(parameter.type)) "
                                + "\(parameter.type.rawValue)."))
                }
                bound[parameter.name] = value
                continue
            }
            guard let fallback = parameter.default else {
                return .failure(
                    ActionError(
                        "\"\(action.name)\" needs a value for \"\(parameter.name)\" "
                            + "(\(parameter.type.rawValue)"
                            + (parameter.description.map { ": \($0)" } ?? "")
                            + ") — it has no default."))
            }
            bound[parameter.name] = fallback
        }
        return .success(bound)
    }

    /// The supplied values as an MCP/JSON argument object earns them: nil
    /// when the key is absent, a refusal when it is present and not an
    /// object. Shared by `run_action` and `run_batch` so the two cannot word
    /// the same mistake differently.
    static func argument(_ value: Any?) -> Result<[String: Any], ActionError>? {
        guard let value = value, !(value is NSNull) else { return nil }
        guard let object = value as? [String: Any] else {
            return .failure(
                ActionError("parameters must be an object of name: value pairs"))
        }
        return .success(object)
    }
}
