import Foundation
import HFAPI
import MLX
import MLXLLM
import MLXLMCommon

func generateGemmaText(request: BridgeGemmaRequest) throws(BridgeGemmaError) -> BridgeGemmaResponse {
    let payload = request.intoPayload()
    let semaphore = DispatchSemaphore(value: 0)
    let box = GemmaResultBox()

    Task.detached {
        do {
            let text = try await GemmaGeneratorStore.shared.generate(payload)
            box.set(.success(BridgeGemmaResponse(text: text)))
        } catch let error as BridgeGemmaError {
            box.set(.failure(error))
        } catch {
            box.set(.failure(.Internal(message: RustString(error.localizedDescription))))
        }
        semaphore.signal()
    }

    semaphore.wait()
    guard let result = box.get() else {
        throw .Internal(message: RustString("Gemma generation finished without a result"))
    }
    return try result.get()
}

private struct GemmaModelSource: Hashable, Sendable {
    let modelId: String
    let localDirectory: URL?

    var cacheKey: String {
        if let localDirectory {
            return "local:\(localDirectory.standardizedFileURL.path)"
        }
        return "remote:\(modelId)"
    }

    var modelConfiguration: ModelConfiguration {
        if let localDirectory {
            return ModelConfiguration(
                directory: localDirectory,
                extraEOSTokens: ["<end_of_turn>"]
            )
        }

        return ModelConfiguration(
            id: modelId,
            extraEOSTokens: ["<end_of_turn>"]
        )
    }
}

private actor GemmaGeneratorStore {
    static let shared = GemmaGeneratorStore()

    private var factoryInstance: LLMModelFactory?
    private var containers = [String: Task<ModelContainer, Error>]()

    func generate(_ request: GemmaRequestPayload) async throws -> String {
        let source = try resolveSource(request)
        let container = try await modelContainer(for: source)
        let parameters = GenerateParameters(
            maxTokens: request.maxNewTokens,
            temperature: 0.0,
            repetitionPenalty: 1.1,
            repetitionContextSize: 128
        )
        do {
            return try await container.perform { context in
                let userInput = UserInput(
                    chat: [
                        .system(
                            "You are a concise assistant that writes useful markdown summaries from transcripts. Always return a non-empty summary."
                        ),
                        .user(request.prompt),
                    ],
                    additionalContext: ["enable_thinking": false]
                )
                let input = try await context.processor.prepare(input: userInput)
                let stream = try MLXLMCommon.generate(
                    input: input,
                    parameters: parameters,
                    context: context
                )

                var output = ""
                for await generation in stream {
                    switch generation {
                    case .chunk(let chunk):
                        output += chunk
                    case .info:
                        break
                    case .toolCall:
                        throw BridgeGemmaError.GenerateFailure(
                            message: RustString("Gemma emitted an unexpected tool call")
                        )
                    }
                }

                let normalizedOutput = output.trimmingCharacters(in: .whitespacesAndNewlines)
                if !normalizedOutput.isEmpty {
                    return normalizedOutput
                }

                let rawInput = try await context.processor.prepare(input: userInput)
                let rawStream = try generateTokens(
                    input: rawInput,
                    parameters: parameters,
                    context: context,
                    includeStopToken: true
                )
                var tokenIds = [Int]()
                var completionInfo: GenerateCompletionInfo?

                for await event in rawStream {
                    switch event {
                    case .token(let token):
                        tokenIds.append(token)
                    case .info(let info):
                        completionInfo = info
                    }
                }

                let rawOutput = context.tokenizer.decode(tokenIds: tokenIds, skipSpecialTokens: false)
                let normalizedRawOutput = normalizeGemmaRawOutput(rawOutput)
                if !normalizedRawOutput.isEmpty {
                    return normalizedRawOutput
                }

                let stopReason = completionInfo.map { String(describing: $0.stopReason) } ?? "unknown"
                let tokenSummary = tokenIds.map(String.init).joined(separator: ",")
                let rawSummary = rawOutput.replacingOccurrences(of: "\n", with: "\\n")
                throw BridgeGemmaError.GenerateFailure(
                    message: RustString(
                        "Gemma produced an empty response (tokens: \(tokenIds.count), stop: \(stopReason), token_ids: [\(tokenSummary)], raw: \(rawSummary))"
                    )
                )
            }
        } catch let error as BridgeGemmaError {
            throw error
        } catch {
            throw BridgeGemmaError.Internal(message: RustString(error.localizedDescription))
        }
    }

    private func resolveSource(_ request: GemmaRequestPayload) throws(BridgeGemmaError) -> GemmaModelSource {
        guard let localModelPath = request.localModelPath else {
            return GemmaModelSource(modelId: request.modelId, localDirectory: nil)
        }

        let directory = URL(fileURLWithPath: localModelPath, isDirectory: true).standardizedFileURL
        let configURL = directory.appending(component: "config.json")
        guard FileManager.default.fileExists(atPath: configURL.path) else {
            throw .InvalidModelPath(
                message: RustString("Gemma model directory is missing config.json at \(directory.path)")
            )
        }
        return GemmaModelSource(modelId: request.modelId, localDirectory: directory)
    }

    private func modelContainer(for source: GemmaModelSource) async throws(BridgeGemmaError) -> ModelContainer {
        if let task = containers[source.cacheKey] {
            do {
                return try await task.value
            } catch let error as BridgeGemmaError {
                throw error
            } catch {
                throw .Internal(message: RustString(error.localizedDescription))
            }
        }

        let task = Task<ModelContainer, Error> {
            try await self.loadModelContainer(for: source)
        }
        containers[source.cacheKey] = task

        do {
            return try await task.value
        } catch let error as BridgeGemmaError {
            containers[source.cacheKey] = nil
            throw error
        } catch {
            containers[source.cacheKey] = nil
            throw .Internal(message: RustString(error.localizedDescription))
        }
    }

    private func loadModelContainer(for source: GemmaModelSource) async throws(BridgeGemmaError) -> ModelContainer {
        let factory = try await modelFactory()
        let tokenizerLoader = TokenizersLoader()
        let configuration = source.modelConfiguration

        let resolvedConfiguration: ResolvedModelConfiguration
        if let localDirectory = source.localDirectory {
            resolvedConfiguration = configuration.resolved(
                modelDirectory: localDirectory,
                tokenizerDirectory: localDirectory
            )
        } else {
            do {
                resolvedConfiguration = try await resolve(
                    configuration: configuration,
                    from: HubClient.default,
                    useLatest: false,
                    progressHandler: { _ in }
                )
            } catch {
                throw .DownloadFailure(message: RustString(error.localizedDescription))
            }
        }

        do {
            let context = try await factory._load(
                configuration: resolvedConfiguration,
                tokenizerLoader: tokenizerLoader
            )
            return ModelContainer(context: context)
        } catch {
            throw .LoadFailure(message: RustString(error.localizedDescription))
        }
    }

    private func modelFactory() async throws(BridgeGemmaError) -> LLMModelFactory {
        if let factoryInstance {
            return factoryInstance
        }

        let typeRegistry = ModelTypeRegistry()
        await typeRegistry.registerModelType("gemma4") { configurationData in
            try makeGemma4Model(configurationData: configurationData)
        }
        let factory = LLMModelFactory(
            typeRegistry: typeRegistry,
            modelRegistry: AbstractModelRegistry()
        )
        self.factoryInstance = factory
        return factory
    }

}

private func normalizeGemmaRawOutput(_ rawOutput: String) -> String {
    var normalizedOutput = rawOutput

    for token in [
        "<bos>",
        "<eos>",
        "<pad>",
        "<|think|>",
        "<|turn>",
        "<turn|>",
        "<|tool_call>",
        "<tool_call|>",
        "<|tool>",
        "<tool|>",
        "<|tool_response>",
        "<tool_response|>",
        "<|channel>",
        "<channel|>",
    ] {
        normalizedOutput = normalizedOutput.replacingOccurrences(of: token, with: "")
    }

    normalizedOutput = replacingMatches(
        in: normalizedOutput,
        pattern: #"<\|channel\>thought\s*.*?<channel\|>"#
    )
    normalizedOutput = replacingMatches(
        in: normalizedOutput,
        pattern: #"<\|tool_call\>.*?<tool_call\|>"#
    )
    normalizedOutput = replacingMatches(
        in: normalizedOutput,
        pattern: #"\n{3,}"#,
        replacement: "\n\n"
    )

    return normalizedOutput.trimmingCharacters(in: .whitespacesAndNewlines)
}

private func replacingMatches(
    in value: String,
    pattern: String,
    replacement: String = ""
) -> String {
    guard
        let expression = try? NSRegularExpression(
            pattern: pattern,
            options: [.dotMatchesLineSeparators]
        )
    else {
        return value
    }

    let range = NSRange(value.startIndex..., in: value)
    return expression.stringByReplacingMatches(
        in: value,
        options: [],
        range: range,
        withTemplate: replacement
    )
}

private final class GemmaResultBox: @unchecked Sendable {
    private let lock = NSLock()
    private var result: Result<BridgeGemmaResponse, BridgeGemmaError>?

    func set(_ result: Result<BridgeGemmaResponse, BridgeGemmaError>) {
        lock.lock()
        self.result = result
        lock.unlock()
    }

    func get() -> Result<BridgeGemmaResponse, BridgeGemmaError>? {
        lock.lock()
        defer { lock.unlock() }
        return result
    }
}
