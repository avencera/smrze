import Foundation
import FoundationModels

private let chunkCharacterBudget = 8_000
private let chunkTurnBudget = 120
private let reductionCharacterBudget = 6_000

@Generable
struct GeneratedActionItem {
    @Guide(description: "The person responsible only when the transcript explicitly assigns the task")
    let owner: String?

    @Guide(description: "A concrete follow-up task only when the transcript explicitly mentions one")
    let task: String
}

@Generable
struct GeneratedSummary {
    @Guide(description: "A concise one sentence overview of the transcript")
    let overview: String

    @Guide(description: "One to six important points from the transcript without filler")
    @Guide(.count(1 ... 6))
    let keyPoints: [String]

    @Guide(description: "Explicit decisions or conclusions mentioned in the transcript. Return an empty array when there are none")
    @Guide(.maximumCount(6))
    let decisions: [String]

    @Guide(description: "Concrete next steps and follow-ups only when the transcript explicitly mentions them. Return an empty array when there are none")
    @Guide(.maximumCount(8))
    let actionItems: [GeneratedActionItem]
}

struct FoundationModelSummarizer {
    private let model = SystemLanguageModel(useCase: .general, guardrails: .default)
    private let options = GenerationOptions(
        sampling: .greedy,
        temperature: nil,
        maximumResponseTokens: 700
    )

    func summarize(_ request: SummaryRequest) async throws -> SummaryResponse {
        try validate(request)
        try validateModel()

        let chunkedTurns = chunkTurns(request.turns)
        let chunkSummaries = try await summarizeTurnGroups(request.title, groups: chunkedTurns)
        let finalSummary = try await reduceSummaries(request.title, chunkSummaries)
        return finalSummary
    }

    private func validate(_ request: SummaryRequest) throws {
        guard !request.turns.isEmpty else {
            throw SummaryHelperError.invalidRequest("transcript contained no turns to summarize")
        }
    }

    private func validateModel() throws {
        switch model.availability {
        case .available:
            break
        case let .unavailable(reason):
            throw SummaryHelperError.unavailableModel(unavailableMessage(for: reason))
        }

        guard model.supportsLocale(Locale.current) else {
            throw SummaryHelperError.unsupportedLocale(
                "the local Apple foundation model does not support locale \(Locale.current.identifier)"
            )
        }
    }

    private func summarizeTurnGroups(
        _ title: String,
        groups: [[SummaryTurn]]
    ) async throws -> [SummaryResponse] {
        var summaries = [SummaryResponse]()
        summaries.reserveCapacity(groups.count)
        for group in groups {
            let summary = try await summarizeTurnGroupRecursively(title, turns: group)
            summaries.append(summary)
        }
        return summaries
    }

    private func summarizeTurnGroupRecursively(
        _ title: String,
        turns: [SummaryTurn]
    ) async throws -> SummaryResponse {
        let prompt = transcriptPrompt(title, turns: turns)

        do {
            let generated = try await generate(prompt)
            return mapSummary(generated)
        } catch let error as LanguageModelSession.GenerationError {
            guard case .exceededContextWindowSize = error else {
                throw mapGenerationError(error)
            }
            guard turns.count > 1 else {
                throw mapGenerationError(error)
            }

            let midpoint = turns.count / 2
            let left = try await summarizeTurnGroupRecursively(title, turns: Array(turns[..<midpoint]))
            let right = try await summarizeTurnGroupRecursively(title, turns: Array(turns[midpoint...]))
            return try await reduceSummaryGroupRecursively(title, summaries: [left, right])
        }
    }

    private func reduceSummaries(
        _ title: String,
        _ summaries: [SummaryResponse]
    ) async throws -> SummaryResponse {
        guard summaries.count > 1 else {
            return summaries[0]
        }

        let groups = chunkSummaries(summaries)
        var merged = [SummaryResponse]()
        merged.reserveCapacity(groups.count)
        for group in groups {
            let summary = try await reduceSummaryGroupRecursively(title, summaries: group)
            merged.append(summary)
        }

        if merged.count == 1 {
            return merged[0]
        }

        return try await reduceSummaries(title, merged)
    }

    private func reduceSummaryGroupRecursively(
        _ title: String,
        summaries: [SummaryResponse]
    ) async throws -> SummaryResponse {
        let prompt = reductionPrompt(title, summaries: summaries)

        do {
            let generated = try await generate(prompt)
            return mapSummary(generated)
        } catch let error as LanguageModelSession.GenerationError {
            guard case .exceededContextWindowSize = error else {
                throw mapGenerationError(error)
            }
            guard summaries.count > 1 else {
                throw mapGenerationError(error)
            }

            let midpoint = summaries.count / 2
            let left = try await reduceSummaryGroupRecursively(
                title,
                summaries: Array(summaries[..<midpoint])
            )
            let right = try await reduceSummaryGroupRecursively(
                title,
                summaries: Array(summaries[midpoint...])
            )
            return try await reduceSummaryGroupRecursively(title, summaries: [left, right])
        }
    }

    private func generate(_ prompt: String) async throws -> GeneratedSummary {
        let session = LanguageModelSession(
            model: model,
            instructions: """
            You summarize transcripts clearly and faithfully.
            Keep names, terminology, and explicit claims accurate.
            Do not invent decisions or action items that are not supported by the transcript.
            Use empty arrays for decisions and actionItems when the transcript does not state them explicitly.
            Do not turn general observations, descriptions, or implied suggestions into action items.
            """
        )
        session.prewarm()
        let response = try await session.respond(
            to: prompt,
            generating: GeneratedSummary.self,
            options: options
        )
        return response.content
    }

    private func transcriptPrompt(_ title: String, turns: [SummaryTurn]) -> String {
        let body = turns
            .map(\.formattedText)
            .joined(separator: "\n")
        return """
        Summarize this transcript chunk from "\(title)".
        Focus on the factual content only.
        If there are no explicit decisions, return an empty decisions array.
        If there are no explicit action items, return an empty actionItems array.

        Transcript:
        \(body)
        """
    }

    private func reductionPrompt(_ title: String, summaries: [SummaryResponse]) -> String {
        let body = summaries
            .enumerated()
            .map { index, summary in
                """
                Chunk \(index + 1)
                Overview: \(summary.overview)
                Key points:
                \(bulletLines(summary.keyPoints))
                Decisions:
                \(bulletLines(summary.decisions))
                Action items:
                \(actionItemLines(summary.actionItems))
                """
            }
            .joined(separator: "\n\n")

        return """
        Merge these partial summaries from "\(title)" into one final summary.
        Deduplicate repeated points.
        Keep only decisions and action items that are explicitly supported by the partial summaries.
        Return empty decisions and actionItems arrays when none are explicitly supported.

        Partial summaries:
        \(body)
        """
    }

    func chunkTurns(_ turns: [SummaryTurn]) -> [[SummaryTurn]] {
        var groups = [[SummaryTurn]]()
        var current = [SummaryTurn]()
        var currentCharacters = 0

        for turn in turns {
            let turnCharacters = turn.formattedText.count + 1
            let wouldOverflow = !current.isEmpty
                && (currentCharacters + turnCharacters > chunkCharacterBudget
                    || current.count >= chunkTurnBudget)
            if wouldOverflow {
                groups.append(current)
                current = []
                currentCharacters = 0
            }

            current.append(turn)
            currentCharacters += turnCharacters
        }

        if !current.isEmpty {
            groups.append(current)
        }

        return groups
    }

    func chunkSummaries(_ summaries: [SummaryResponse]) -> [[SummaryResponse]] {
        var groups = [[SummaryResponse]]()
        var current = [SummaryResponse]()
        var currentCharacters = 0

        for summary in summaries {
            let summaryCharacters = serializedSummary(summary).count + 2
            let wouldOverflow = !current.isEmpty
                && currentCharacters + summaryCharacters > reductionCharacterBudget
            if wouldOverflow {
                groups.append(current)
                current = []
                currentCharacters = 0
            }

            current.append(summary)
            currentCharacters += summaryCharacters
        }

        if !current.isEmpty {
            groups.append(current)
        }

        return groups
    }

    private func serializedSummary(_ summary: SummaryResponse) -> String {
        """
        Overview: \(summary.overview)
        Key points:
        \(bulletLines(summary.keyPoints))
        Decisions:
        \(bulletLines(summary.decisions))
        Action items:
        \(actionItemLines(summary.actionItems))
        """
    }

    private func bulletLines(_ values: [String]) -> String {
        if values.isEmpty {
            return "- none"
        }

        return values.map { "- \($0)" }.joined(separator: "\n")
    }

    private func actionItemLines(_ values: [SummaryActionItem]) -> String {
        if values.isEmpty {
            return "- none"
        }

        return values.map { item in
            if let owner = item.owner?.trimmingCharacters(in: .whitespacesAndNewlines),
               !owner.isEmpty {
                return "- \(owner): \(item.task)"
            }
            return "- \(item.task)"
        }
        .joined(separator: "\n")
    }

    private func mapSummary(_ generated: GeneratedSummary) -> SummaryResponse {
        SummaryResponse(
            overview: generated.overview.trimmingCharacters(in: .whitespacesAndNewlines),
            keyPoints: uniqueNonPlaceholderStrings(generated.keyPoints),
            decisions: uniqueNonPlaceholderStrings(generated.decisions),
            actionItems: uniqueActionItems(
                generated.actionItems
                .map { item in
                    SummaryActionItem(
                        owner: item.owner.map(trimmed).flatMap(optionalNotEmpty),
                        task: trimmed(item.task)
                    )
                }
            )
        )
    }

    private func trimmed(_ value: String) -> String {
        value.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func optionalNotEmpty(_ value: String) -> String? {
        value.isEmpty ? nil : value
    }

    private func uniqueNonPlaceholderStrings(_ values: [String]) -> [String] {
        var seen = Set<String>()
        var result = [String]()
        for value in values.map(trimmed) {
            let normalized = normalizePlaceholder(value)
            guard let cleaned = normalized, seen.insert(cleaned.lowercased()).inserted else {
                continue
            }
            result.append(cleaned)
        }
        return result
    }

    private func uniqueActionItems(_ values: [SummaryActionItem]) -> [SummaryActionItem] {
        var seen = Set<String>()
        var result = [SummaryActionItem]()
        for value in values {
            guard let task = normalizePlaceholder(value.task) else {
                continue
            }

            let owner = value.owner
                .map(trimmed)
                .flatMap(optionalNotEmpty)
                .flatMap(normalizePlaceholder)
            let key = "\(owner?.lowercased() ?? "")\u{0}\(task.lowercased())"
            guard seen.insert(key).inserted else {
                continue
            }

            result.append(SummaryActionItem(owner: owner, task: task))
        }
        return result
    }

    private func normalizePlaceholder(_ value: String) -> String? {
        let cleaned = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleaned.isEmpty else {
            return nil
        }

        let normalized = cleaned
            .lowercased()
            .replacingOccurrences(of: "-", with: "")
            .replacingOccurrences(of: ":", with: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        switch normalized {
        case "none", "n/a", "na", "none provided", "no action items", "no decisions":
            return nil
        default:
            return cleaned
        }
    }

    private func unavailableMessage(
        for reason: SystemLanguageModel.Availability.UnavailableReason
    ) -> String {
        switch reason {
        case .deviceNotEligible:
            return "Apple Intelligence is unavailable because this Mac is not eligible"
        case .appleIntelligenceNotEnabled:
            return "Apple Intelligence is not enabled on this Mac"
        case .modelNotReady:
            return "Apple Intelligence models are not ready yet on this Mac"
        @unknown default:
            return "Apple Intelligence is unavailable on this Mac"
        }
    }

    private func mapGenerationError(_ error: LanguageModelSession.GenerationError) -> Error {
        switch error {
        case let .exceededContextWindowSize(context):
            return SummaryHelperError.exceededContextWindow(context.debugDescription)
        case let .unsupportedLanguageOrLocale(context):
            return SummaryHelperError.unsupportedLocale(context.debugDescription)
        default:
            return SummaryHelperError.modelFailure(error.localizedDescription)
        }
    }
}
