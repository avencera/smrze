import Foundation

struct SummaryRequestPayload: Sendable {
    let title: String
    let turns: [String]
}

struct SummaryActionItemPayload: Equatable, Sendable {
    let owner: String?
    let task: String
}

struct SummaryDocumentPayload: Equatable, Sendable {
    let overview: String
    let keyPoints: [String]
    let decisions: [String]
    let actionItems: [SummaryActionItemPayload]
}

extension BridgeSummaryRequest {
    func intoPayload() -> SummaryRequestPayload {
        SummaryRequestPayload(
            title: title.toString(),
            turns: turns.map { $0.stringValue() }
        )
    }
}

extension SummaryDocumentPayload {
    func intoBridge() -> BridgeSummaryDocument {
        BridgeSummaryDocument(
            overview: RustString(overview),
            key_points: rustStringVec(keyPoints),
            decisions: rustStringVec(decisions),
            action_item_owners: rustStringVec(actionItems.map(\.owner).map { $0 ?? "" }),
            action_item_tasks: rustStringVec(actionItems.map(\.task))
        )
    }
}

extension BridgeSummaryError: Error {}
extension BridgeSummaryError: @unchecked Sendable {}

extension RustStringRef {
    func stringValue() -> String {
        as_str().toString()
    }
}

func rustStringVec(_ values: [String]) -> RustVec<RustString> {
    let result = RustVec<RustString>()
    for value in values {
        result.push(value: RustString(value))
    }
    return result
}
