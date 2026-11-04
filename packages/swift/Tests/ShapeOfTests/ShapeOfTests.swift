import Foundation
import Testing

@testable import ShapeOf

private struct Place: Decodable {
    let reason: String
    let line: Int
    let column: Int
    let source: String
}

private enum Failure: Decodable {
    case unreadable(Place)
    case refused(String)

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let place = try? container.decode(Place.self) {
            self = .unreadable(place)
        } else {
            self = .refused(try container.decode(String.self))
        }
    }
}

private struct Want: Decodable {
    let schema: String?
    let notes: Notes?
    let error: Failure?
}

private struct Case: Decodable {
    let name: String
    let target: String
    let input: String
    let want: Want
}

private struct Corpus: Decodable {
    let format: Int
    let cases: [Case]
}

private let corpusURL: URL = {
    if let path = ProcessInfo.processInfo.environment["SHAPEOF_CORPUS"] {
        return URL(fileURLWithPath: path)
    }
    return URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .appendingPathComponent("../../../../testdata/cases.json")
        .standardized
}()

private func expectedError(_ failure: Failure?) -> ShapeOfError? {
    switch failure {
    case .unreadable(let at):
        .unreadable(reason: at.reason, line: at.line, column: at.column, source: at.source)
    case .refused("depth"):
        .depth
    case .refused:
        .internal
    case nil:
        nil
    }
}

@Test func corpusCases() throws {
    let corpus = try JSONDecoder().decode(Corpus.self, from: Data(contentsOf: corpusURL))
    #expect(corpus.format == 1)
    #expect(!corpus.cases.isEmpty)
    for c in corpus.cases {
        let target = try #require(Target.allCases.first { "\($0)" == c.target }, "\(c.name)")
        do {
            let inferred = try ShapeOf.schema(c.input, for: target)
            #expect(c.want.error == nil, "\(c.name)")
            #expect(inferred.schema == c.want.schema, "\(c.name)")
            #expect(inferred.notes == c.want.notes, "\(c.name)")
        } catch {
            #expect(error == expectedError(c.want.error), "\(c.name)")
        }
    }
}

@Test func reportsTheVersion() {
    #expect(!ShapeOf.version.isEmpty)
}
