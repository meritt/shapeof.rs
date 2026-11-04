import Foundation
import ShapeOfFFI

public enum ShapeOf {
    public static let version = String(cString: shapeof_version())

    public static func schema(_ examples: String, for target: Target) throws(ShapeOfError) -> Inferred {
        let input = Array(examples.utf8)
        var out = shapeof_report()
        input.withUnsafeBufferPointer { input in
            shapeof_infer(input.baseAddress, input.count, target.rawValue, &out)
        }
        defer { shapeof_report_free(&out) }
        switch out.status {
        case UInt32(SHAPEOF_OK):
            return try decode(Data(UnsafeBufferPointer(start: out.data, count: out.len)))
        case UInt32(SHAPEOF_INVALID_UTF8):
            throw .invalidUTF8
        case UInt32(SHAPEOF_INVALID_ARGUMENT):
            throw .invalidArgument
        default:
            throw .internal
        }
    }

    private static func decode(_ json: Data) throws(ShapeOfError) -> Inferred {
        let decoder = JSONDecoder()
        if let failure = try? decoder.decode(Unreadable.self, from: json) {
            let at = failure.error
            throw .unreadable(reason: at.reason, line: at.line, column: at.column, source: at.source)
        }
        if let failure = try? decoder.decode(Refused.self, from: json), failure.error == "depth" {
            throw .depth
        }
        do {
            return try decoder.decode(Inferred.self, from: json)
        } catch {
            throw .internal
        }
    }
}

public enum Target: UInt32, CaseIterable, Sendable {
    case openai
    case anthropic
    case plain
}

public struct Inferred: Decodable, Equatable, Sendable {
    public let schema: String
    public let notes: Notes
}

public struct Notes: Decodable, Equatable, Sendable {
    public let empty: [[String]]
    public let unions: Int
    public let optional: Int
    public let depth: Int
    public let properties: Int
    public let names: Int
    public let rootObject: Bool
}

public enum ShapeOfError: Error, Equatable, Sendable {
    case unreadable(reason: String, line: Int, column: Int, source: String)
    case depth
    case invalidUTF8
    case invalidArgument
    case `internal`
}

private struct Unreadable: Decodable {
    struct Place: Decodable {
        let reason: String
        let line: Int
        let column: Int
        let source: String
    }

    let error: Place
}

private struct Refused: Decodable {
    let error: String
}
