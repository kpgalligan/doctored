import Foundation

/// One edit to an action's declared parameters — what the Actions window's Parameters… sheet and
/// MCP `edit_action_parameters` both send. No JSON is hand-edited on either path.
///
/// **A rename carries its references.** Renaming `sigma` to `radius` rewrites every
/// `{"$param": "sigma"}` in every step, so the action still runs; removing a parameter a step
/// still references is refused naming the steps, because the decode would refuse the file anyway
/// (`Action.decode`: a `$param` that names nothing).
///
/// **A retype keeps what still holds.** A default that matches the new type is kept; integer ↔
/// number converts an exact value; any other default is refused unless the same edit gives a new
/// one, so a retype never silently drops a default and makes a parameter required.
enum ActionParameterEdit {
    case add(name: String, type: ActionParameter.ParameterType, default: Any?, description: String?)
    case rename(from: String, to: String)
    /// `default` present (`.some`) replaces the default in the same edit; `.some(nil)` removes it.
    case retype(name: String, type: ActionParameter.ParameterType, default: Any??)
    /// nil removes the default, which makes the parameter required.
    case setDefault(name: String, value: Any?)
    /// nil or empty removes the description.
    case setDescription(name: String, text: String?)
    case remove(name: String)

    /// The MCP form: `{"op": "add" | "rename" | "retype" | "set_default" | "set_description" |
    /// "remove", "name", "to", "type", "default", "description"}`.
    static func parse(_ value: Any, at index: Int) -> Result<ActionParameterEdit, ActionError> {
        let label = "edits[\(index)]"
        guard let fields = value as? [String: Any], let op = fields["op"] as? String else {
            return .failure(ActionError("\(label) must be an object with an \"op\""))
        }
        guard let name = fields["name"] as? String else {
            return .failure(ActionError("\(label) needs \"name\""))
        }
        func type() -> Result<ActionParameter.ParameterType, ActionError> {
            guard let raw = fields["type"] as? String,
                  let type = ActionParameter.ParameterType(rawValue: raw) else {
                return .failure(ActionError(
                    "\(label).type must be one of "
                        + ActionParameter.ParameterType.allCases.map(\.rawValue)
                        .joined(separator: ", ")))
            }
            return .success(type)
        }
        /// Absent → nil; JSON null → .some(nil); a value → .some(value).
        let defaultValue: Any?? = fields.keys.contains("default")
            ? .some(fields["default"].flatMap { $0 is NSNull ? nil : $0 }) : nil
        switch op {
        case "add":
            return type().map {
                ActionParameterEdit.add(name: name, type: $0, default: defaultValue ?? nil,
                                        description: fields["description"] as? String)
            }
        case "rename":
            guard let to = fields["to"] as? String else {
                return .failure(ActionError("\(label): rename needs \"to\""))
            }
            return .success(.rename(from: name, to: to))
        case "retype":
            return type().map {
                ActionParameterEdit.retype(name: name, type: $0, default: defaultValue)
            }
        case "set_default":
            guard let defaultValue else {
                return .failure(ActionError(
                    "\(label): set_default needs \"default\" (null makes the parameter required)"))
            }
            return .success(.setDefault(name: name, value: defaultValue))
        case "set_description":
            return .success(.setDescription(name: name, text: fields["description"] as? String))
        case "remove":
            return .success(.remove(name: name))
        default:
            return .failure(ActionError(
                "\(label).op must be add, rename, retype, set_default, set_description or remove "
                    + "(got \"\(op)\")"))
        }
    }
}

extension Action {
    /// `self` with every edit applied in order, or the first one's refusal — all or nothing. The
    /// result is re-read through `Action.decode` of its own encoding, so it is exactly what the
    /// library will accept.
    func applying(_ edits: [ActionParameterEdit]) -> Result<Action, ActionError> {
        var action = self
        for (index, edit) in edits.enumerated() {
            if let refusal = action.apply(edit) {
                return .failure(ActionError("edits[\(index)]: \(refusal)"))
            }
        }
        return Action.decode(action.encoded())
    }

    /// Applies one edit in place, or answers why it cannot.
    private mutating func apply(_ edit: ActionParameterEdit) -> String? {
        func index(of name: String) -> Int? { parameters.firstIndex { $0.name == name } }
        func missing(_ name: String) -> String {
            "\"\(self.name)\" has no parameter named \"\(name)\""
        }
        func checked(_ value: Any?, _ type: ActionParameter.ParameterType) -> String? {
            guard let value, !ActionParameter.matches(value, type) else { return nil }
            return "the default is not \(ActionParameter.article(type)) \(type.rawValue)"
        }
        switch edit {
        case .add(let name, let type, let value, let description):
            guard ActionParameter.isValidName(name) else { return Self.nameRule(name) }
            guard index(of: name) == nil else { return "\"\(name)\" is already declared" }
            if let problem = checked(value, type) { return problem }
            parameters.append(ActionParameter(
                name: name, type: type, default: value, description: Self.text(description)))
        case .rename(let from, let to):
            guard let at = index(of: from) else { return missing(from) }
            guard ActionParameter.isValidName(to) else { return Self.nameRule(to) }
            guard from == to || index(of: to) == nil else { return "\"\(to)\" is already declared" }
            let old = parameters[at]
            parameters[at] = ActionParameter(
                name: to, type: old.type, default: old.default, description: old.description)
            for step in steps.indices {
                steps[step].arguments = Self.renamed(steps[step].arguments, from: from, to: to)
                    as? [String: Any] ?? steps[step].arguments
            }
        case .retype(let name, let type, let replacement):
            guard let at = index(of: name) else { return missing(name) }
            let old = parameters[at]
            var value: Any?
            if let replacement {
                value = replacement
                if let problem = checked(value, type) { return problem }
            } else if let current = old.default {
                guard let converted = Self.converted(current, to: type) else {
                    return "its default \(Self.shown(current)) is not "
                        + "\(ActionParameter.article(type)) \(type.rawValue) — give a new default "
                        + "in the same edit, or null to make it required"
                }
                value = converted
            }
            parameters[at] = ActionParameter(
                name: name, type: type, default: value, description: old.description)
        case .setDefault(let name, let value):
            guard let at = index(of: name) else { return missing(name) }
            let old = parameters[at]
            if let problem = checked(value, old.type) { return problem }
            parameters[at] = ActionParameter(
                name: name, type: old.type, default: value, description: old.description)
        case .setDescription(let name, let text):
            guard let at = index(of: name) else { return missing(name) }
            let old = parameters[at]
            parameters[at] = ActionParameter(
                name: name, type: old.type, default: old.default, description: Self.text(text))
        case .remove(let name):
            guard let at = index(of: name) else { return missing(name) }
            let users = steps.indices.filter { Self.references(steps[$0].arguments, name) }
            guard users.isEmpty else {
                return "\"\(name)\" is still used by "
                    + users.map { "step \($0 + 1) (\(steps[$0].tool))" }.joined(separator: ", ")
                    + " — point those arguments at a value first"
            }
            parameters.remove(at: at)
        }
        return nil
    }

    private static func nameRule(_ name: String) -> String {
        "\"\(name)\" is not a parameter name: a letter or underscore followed by letters, digits "
            + "or underscores"
    }

    private static func text(_ value: String?) -> String? {
        guard let value, !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return nil
        }
        return value
    }

    private static func shown(_ value: Any) -> String {
        if let string = value as? String { return "\"\(string)\"" }
        return "\(value)"
    }

    /// `value` as `type` when it still is one, or converts exactly (integer ↔ number).
    private static func converted(_ value: Any, to type: ActionParameter.ParameterType) -> Any? {
        if ActionParameter.matches(value, type) { return value }
        if type == .number, let number = ActionJSON.number(value) { return number.doubleValue }
        return nil
    }

    /// `value` with every `{"$param": from}` rewritten to `to`.
    private static func renamed(_ value: Any, from: String, to: String) -> Any {
        if ActionSymbol.looksLikeSymbol(value) {
            if case .success(.parameter(let name)) = ActionSymbol.parse(value), name == from {
                return ["$param": to]
            }
            return value
        }
        if let object = value as? [String: Any] {
            return object.mapValues { renamed($0, from: from, to: to) }
        }
        if let array = value as? [Any] {
            return array.map { renamed($0, from: from, to: to) }
        }
        return value
    }

    /// Whether `value` holds a `{"$param": name}` anywhere.
    private static func references(_ value: Any, _ name: String) -> Bool {
        if ActionSymbol.looksLikeSymbol(value) {
            if case .success(.parameter(let found)) = ActionSymbol.parse(value) {
                return found == name
            }
            return false
        }
        if let object = value as? [String: Any] {
            return object.values.contains { references($0, name) }
        }
        if let array = value as? [Any] {
            return array.contains { references($0, name) }
        }
        return false
    }
}
