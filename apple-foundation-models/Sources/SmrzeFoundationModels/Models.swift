import Foundation

struct SummaryRequestPayload: Sendable {
    let title: String
    let turns: [String]
}

struct GemmaRequestPayload: Sendable {
    let modelId: String
    let localModelPath: String?
    let prompt: String
    let maxNewTokens: Int
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

extension BridgeGemmaRequest {
    func intoPayload() -> GemmaRequestPayload {
        GemmaRequestPayload(
            modelId: model_id.toString(),
            localModelPath: local_model_path?.toString(),
            prompt: prompt.toString(),
            maxNewTokens: Int(max_new_tokens)
        )
    }
}

extension SummaryDocumentPayload {
    func renderMarkdown() -> String {
        var lines = [
            "# Summary",
            "",
            "## Overview",
            overview.trimmingCharacters(in: .whitespacesAndNewlines),
            "",
            "## Key Points",
        ]

        for keyPoint in keyPoints {
            lines.append("- \(keyPoint.trimmingCharacters(in: .whitespacesAndNewlines))")
        }

        if !decisions.isEmpty {
            lines.append("")
            lines.append("## Decisions")
            for decision in decisions {
                lines.append("- \(decision.trimmingCharacters(in: .whitespacesAndNewlines))")
            }
        }

        if !actionItems.isEmpty {
            lines.append("")
            lines.append("## Action Items")
            for actionItem in actionItems {
                let task = actionItem.task.trimmingCharacters(in: .whitespacesAndNewlines)
                let owner = actionItem.owner?.trimmingCharacters(in: .whitespacesAndNewlines)
                if let owner, !owner.isEmpty {
                    lines.append("- \(owner): \(task)")
                } else {
                    lines.append("- \(task)")
                }
            }
        }

        return lines.joined(separator: "\n")
    }
}

extension BridgeSummaryResponse {
    init(text: String) {
        self.init(text: RustString(text))
    }
}

extension BridgeGemmaResponse {
    init(text: String) {
        self.init(text: RustString(text))
    }
}

extension BridgeSummaryError: Error {}
extension BridgeSummaryError: @unchecked Sendable {}
extension BridgeGemmaError: Error {}
extension BridgeGemmaError: @unchecked Sendable {}

extension RustStringRef {
    func stringValue() -> String {
        as_str().toString()
    }
}
