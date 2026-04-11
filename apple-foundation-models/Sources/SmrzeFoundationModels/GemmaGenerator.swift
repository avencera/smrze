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
        let promptTokens = await container.encode(request.prompt)
        let input = LMInput(tokens: MLXArray(promptTokens))
        let parameters = GenerateParameters(maxTokens: request.maxNewTokens, temperature: 0)
        let stream = try await container.generate(input: input, parameters: parameters)

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

        return output
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
