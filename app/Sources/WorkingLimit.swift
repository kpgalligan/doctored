import Foundation

// `WorkingLimit.automatic` is `min(RAM/2, max(RAM/3, RAM − 8 GiB))`, overridden by the
// `WorkingLimitBytes` preference (Doctored ▸ Working Memory…, clamped to 2 GiB…¾ RAM, applied at
// the next launch) and by `DR_WORKING_LIMIT_BYTES` (unclamped; owned test instances pin
// 2147483648). `storedPreference` reads the raw object (NSNumber or argv String), never
// `integer(forKey:)`.

/// Where the process's working limit came from.
enum WorkingLimitSource: String {
    case automatic, preference, environment

    /// The wording the Memory Usage alert and the Working Memory dialog show.
    var label: String {
        switch self {
        case .automatic: return "automatic"
        case .preference: return "your setting"
        case .environment: return WorkingLimit.environmentKey
        }
    }
}

/// The working-memory limit policy (designs/working-limit-ram-scaled.md):
/// a RAM-scaled automatic default, a user preference (Doctored ▸ Working
/// Memory…) and an environment override for tests. Resolved once per
/// process; a preference change applies at the next launch, because history
/// and snapshot budgets and job controls capture the limit at creation.
struct WorkingLimit {
    static let referenceBytes: UInt64 = 2 << 30
    static let environmentKey = "DR_WORKING_LIMIT_BYTES"
    /// UserDefaults; argv `-WorkingLimitBytes <n>` works too (a String).
    static let preferenceKey = "WorkingLimitBytes"
    static let current = resolve(
        environment: ProcessInfo.processInfo.environment, defaults: .standard,
        physicalMemory: ProcessInfo.processInfo.physicalMemory)

    let effectiveBytes: UInt64
    let source: WorkingLimitSource
    let automaticBytes: UInt64
    let physicalMemoryBytes: UInt64

    private static let gib: UInt64 = 1 << 30
    private static let mib: UInt64 = 1 << 20

    /// At least a third of RAM, never more than half, and from 16 GB up
    /// never leaving less than 8 GiB for the OS, display, Core Image's
    /// uncounted scratch and other apps (below that the third wins):
    /// `min(ram/2, max(ram/3, ram − 8 GiB))`, floored to MiB.
    static func automatic(physicalMemory ram: UInt64) -> UInt64 {
        let headroom = ram > 8 * gib ? ram - 8 * gib : 0
        let bytes = min(ram / 2, max(ram / 3, headroom))
        return bytes / mib * mib
    }

    /// The preference's allowed span: 2 GiB up to three quarters of RAM.
    static func preferenceRange(physicalMemory ram: UInt64) -> ClosedRange<UInt64> {
        referenceBytes...max(referenceBytes, ram / 4 * 3)
    }

    /// The dialog's choices, in GiB, filtered to `preferenceRange`.
    static func preferenceChoices(physicalMemory ram: UInt64) -> [UInt64] {
        let range = preferenceRange(physicalMemory: ram)
        return [2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256]
            .map { $0 * gib }
            .filter { range.contains($0) }
    }

    static func resolve(
        environment: [String: String], defaults: UserDefaults, physicalMemory ram: UInt64
    ) -> WorkingLimit {
        let automaticBytes = automatic(physicalMemory: ram)
        if let raw = environment[environmentKey] {
            if let bytes = UInt64(raw), bytes > 0 {
                return WorkingLimit(
                    effectiveBytes: bytes, source: .environment,
                    automaticBytes: automaticBytes, physicalMemoryBytes: ram)
            }
            FileHandle.standardError.write(
                Data("[WorkingLimit] ignoring \(environmentKey)=\(raw)\n".utf8))
        }
        if let stored = storedPreference(defaults) {
            return WorkingLimit(
                effectiveBytes: clamped(stored, physicalMemory: ram), source: .preference,
                automaticBytes: automaticBytes, physicalMemoryBytes: ram)
        }
        return WorkingLimit(
            effectiveBytes: automaticBytes, source: .automatic,
            automaticBytes: automaticBytes, physicalMemoryBytes: ram)
    }

    /// The stored preference, nil for Automatic. Reads the raw object, never
    /// `integer(forKey:)`, which clamps an argv String to Int32.
    static func storedPreference(_ defaults: UserDefaults) -> UInt64? {
        let value: UInt64?
        switch defaults.object(forKey: preferenceKey) {
        case let number as NSNumber:
            value = number.int64Value > 0 ? number.uint64Value : nil
        case let text as String:
            value = UInt64(text)
        default:
            value = nil
        }
        guard let value, value > 0 else { return nil }
        return value
    }

    static func storePreference(_ bytes: UInt64?, _ defaults: UserDefaults) {
        if let bytes {
            defaults.set(NSNumber(value: bytes), forKey: preferenceKey)
        } else {
            defaults.removeObject(forKey: preferenceKey)
        }
    }

    static func clamped(_ bytes: UInt64, physicalMemory ram: UInt64) -> UInt64 {
        let range = preferenceRange(physicalMemory: ram)
        return min(max(bytes, range.lowerBound), range.upperBound)
    }

    /// The `working_limit` object of Help ▸ Memory Usage and MCP
    /// `resource_usage`. `preference_bytes` is read live: what the next
    /// launch uses without the environment override.
    func report(defaults: UserDefaults = .standard) -> [String: Any] {
        let preference: Any = Self.storedPreference(defaults).map {
            Self.clamped($0, physicalMemory: physicalMemoryBytes)
        } ?? NSNull()
        return [
            "effective_bytes": effectiveBytes,
            "source": source.rawValue,
            "automatic_bytes": automaticBytes,
            "preference_bytes": preference,
            "physical_memory_bytes": physicalMemoryBytes,
        ]
    }
}
