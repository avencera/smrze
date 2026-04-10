import Foundation

struct SummaryRequest: Decodable {
    let title: String
    let turns: [SummaryTurn]
}

struct SummaryTurn: Codable, Equatable {
    let speaker: String
    let text: String

    var formattedText: String {
        "\(speaker): \(text)"
    }
}

struct SummaryResponse: Codable, Equatable {
    let overview: String
    let keyPoints: [String]
    let decisions: [String]
    let actionItems: [SummaryActionItem]
}

struct SummaryActionItem: Codable, Equatable {
    let owner: String?
    let task: String
}

enum SummaryHelperError: LocalizedError {
    case invalidRequest(String)
    case unavailableModel(String)
    case unsupportedLocale(String)
    case exceededContextWindow(String)
    case modelFailure(String)

    var errorDescription: String? {
        switch self {
        case let .invalidRequest(message):
            return message
        case let .unavailableModel(message):
            return message
        case let .unsupportedLocale(message):
            return message
        case let .exceededContextWindow(message):
            return message
        case let .modelFailure(message):
            return message
        }
    }
}
