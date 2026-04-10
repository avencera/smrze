import Testing
@testable import SmrzeFoundationModels

@Test
func chunkTurnsRespectsCharacterBudget() {
    let summarizer = FoundationModelSummarizer()
    let turns = (0..<4).map { index in
        "Speaker 1: " + String(repeating: "\(index)", count: 3_500)
    }

    let chunks = summarizer.chunkTurns(turns)

    #expect(chunks.count > 1)
    #expect(chunks.flatMap { $0 } == turns)
}

@Test
func chunkSummariesPreservesOrdering() {
    let summarizer = FoundationModelSummarizer()
    let summaries = (0..<5).map { index in
        SummaryDocumentPayload(
            overview: "Overview \(index)",
            keyPoints: [String(repeating: "Point ", count: 200)],
            decisions: [],
            actionItems: []
        )
    }

    let chunks = summarizer.chunkSummaries(summaries)

    #expect(chunks.count > 1)
    #expect(chunks.flatMap { $0 } == summaries)
}
