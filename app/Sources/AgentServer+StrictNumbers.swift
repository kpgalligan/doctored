import Foundation

/// The v1 handlers' strict numeric arguments: a key a tool reads as a number
/// refuses JSON `null` and a JSON boolean by name, instead of the shared
/// argument helpers' reading — null as "absent" (so the default) and
/// `true`/`false` as 1/0. Numeric strings stay accepted; the helpers
/// (`intArg`/`doubleArg`) are unchanged, so tools outside the v1 set keep
/// their behaviour.
extension AgentServer {
    /// Throws `<context>: <key> must be a number, not <null|true|false>.` for
    /// the first of `keys` (in the given order) whose value is NSNull or a
    /// JSON boolean. Absent keys and every other value pass; the tool's own
    /// parsing still validates them. Called first in a handler, before any
    /// default is taken and before anything is edited, so a refusal leaves no
    /// history step.
    func refuseNullOrBooleanNumbers(
        _ a: [String: Any], _ keys: [String], in context: String
    ) throws {
        for key in keys {
            guard let value = a[key] else { continue }
            if value is NSNull {
                throw ToolError(message: "\(context): \(key) must be a number, not null.")
            }
            if let flag = NativeBrushStroke.jsonBoolean(value) {
                throw ToolError(
                    message: "\(context): \(key) must be a number, not \(flag ? "true" : "false").")
            }
        }
    }
}
