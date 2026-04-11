import Foundation
import HFAPI
import MLXLMCommon

enum HuggingFaceDownloaderError: LocalizedError {
    case invalidRepositoryID(String)

    var errorDescription: String? {
        switch self {
        case .invalidRepositoryID(let id):
            "Invalid Hugging Face repository ID: '\(id)'"
        }
    }
}

extension HubClient: @retroactive Downloader {
    public func download(
        id: String,
        revision: String?,
        matching patterns: [String],
        useLatest: Bool,
        progressHandler: @Sendable @escaping (Progress) -> Void
    ) async throws -> URL {
        guard let repoID = Repo.ID(rawValue: id) else {
            throw HuggingFaceDownloaderError.invalidRepositoryID(id)
        }
        let resolvedRevision = revision ?? "main"

        if !useLatest,
            let cached = resolveCachedSnapshot(
                repo: repoID,
                revision: resolvedRevision,
                matching: patterns
            )
        {
            return cached
        }

        return try await downloadSnapshot(
            of: repoID,
            revision: resolvedRevision,
            matching: patterns,
            progressHandler: progressHandler
        )
    }
}
