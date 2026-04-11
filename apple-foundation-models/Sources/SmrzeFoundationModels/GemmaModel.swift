import Foundation
import MLX
import MLXLLM
import MLXLMCommon
import MLXNN

private typealias RopeParameters = [String: StringOrNumber]

struct Gemma4Configuration: Codable {
    let modelType: String
    let textConfig: Gemma4TextConfiguration

    enum CodingKeys: String, CodingKey {
        case modelType = "model_type"
        case textConfig = "text_config"
    }
}

struct Gemma4TextConfiguration: Codable {
    let modelType: String
    let hiddenSize: Int
    let numHiddenLayers: Int
    let intermediateSize: Int
    let numAttentionHeads: Int
    let headDim: Int
    let globalHeadDim: Int
    let rmsNormEps: Float
    let vocabSize: Int
    let vocabSizePerLayerInput: Int
    let numKeyValueHeads: Int
    let numKvSharedLayers: Int
    let hiddenSizePerLayerInput: Int
    let slidingWindow: Int
    let maxPositionEmbeddings: Int
    let finalLogitSoftcapping: Float
    let useDoubleWideMLP: Bool
    let enableMoeBlock: Bool
    let attentionKEqV: Bool
    let ropeTraditional: Bool
    let layerTypes: [String]
    fileprivate let ropeParameters: [String: RopeParameters]

    enum CodingKeys: String, CodingKey {
        case modelType = "model_type"
        case hiddenSize = "hidden_size"
        case numHiddenLayers = "num_hidden_layers"
        case intermediateSize = "intermediate_size"
        case numAttentionHeads = "num_attention_heads"
        case headDim = "head_dim"
        case globalHeadDim = "global_head_dim"
        case rmsNormEps = "rms_norm_eps"
        case vocabSize = "vocab_size"
        case vocabSizePerLayerInput = "vocab_size_per_layer_input"
        case numKeyValueHeads = "num_key_value_heads"
        case numKvSharedLayers = "num_kv_shared_layers"
        case hiddenSizePerLayerInput = "hidden_size_per_layer_input"
        case slidingWindow = "sliding_window"
        case maxPositionEmbeddings = "max_position_embeddings"
        case finalLogitSoftcapping = "final_logit_softcapping"
        case useDoubleWideMLP = "use_double_wide_mlp"
        case enableMoeBlock = "enable_moe_block"
        case attentionKEqV = "attention_k_eq_v"
        case ropeTraditional = "rope_traditional"
        case layerTypes = "layer_types"
        case ropeParameters = "rope_parameters"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)

        modelType = try container.decode(String.self, forKey: .modelType)
        hiddenSize = try container.decode(Int.self, forKey: .hiddenSize)
        numHiddenLayers = try container.decode(Int.self, forKey: .numHiddenLayers)
        intermediateSize = try container.decode(Int.self, forKey: .intermediateSize)
        numAttentionHeads = try container.decode(Int.self, forKey: .numAttentionHeads)
        headDim = try container.decode(Int.self, forKey: .headDim)
        globalHeadDim = try container.decode(Int.self, forKey: .globalHeadDim)
        rmsNormEps = try container.decode(Float.self, forKey: .rmsNormEps)
        vocabSize = try container.decode(Int.self, forKey: .vocabSize)
        vocabSizePerLayerInput =
            try container.decodeIfPresent(Int.self, forKey: .vocabSizePerLayerInput) ?? vocabSize
        numKeyValueHeads = try container.decode(Int.self, forKey: .numKeyValueHeads)
        numKvSharedLayers = try container.decode(Int.self, forKey: .numKvSharedLayers)
        hiddenSizePerLayerInput =
            try container.decodeIfPresent(Int.self, forKey: .hiddenSizePerLayerInput) ?? 0
        slidingWindow = try container.decode(Int.self, forKey: .slidingWindow)
        maxPositionEmbeddings = try container.decode(Int.self, forKey: .maxPositionEmbeddings)
        finalLogitSoftcapping =
            try container.decodeIfPresent(Float.self, forKey: .finalLogitSoftcapping) ?? 30
        useDoubleWideMLP =
            try container.decodeIfPresent(Bool.self, forKey: .useDoubleWideMLP) ?? false
        enableMoeBlock =
            try container.decodeIfPresent(Bool.self, forKey: .enableMoeBlock) ?? false
        attentionKEqV =
            try container.decodeIfPresent(Bool.self, forKey: .attentionKEqV) ?? false
        ropeTraditional =
            try container.decodeIfPresent(Bool.self, forKey: .ropeTraditional) ?? false
        ropeParameters =
            try container.decodeIfPresent([String: RopeParameters].self, forKey: .ropeParameters)
            ?? Self.defaultRopeParameters()
        layerTypes =
            try container.decodeIfPresent([String].self, forKey: .layerTypes)
            ?? Self.defaultLayerTypes(numHiddenLayers: numHiddenLayers)
    }

    private static func defaultLayerTypes(numHiddenLayers: Int) -> [String] {
        let pattern = ["sliding_attention", "sliding_attention", "sliding_attention", "sliding_attention", "full_attention"]
        return Array((0..<numHiddenLayers).map { pattern[$0 % pattern.count] })
    }

    private static func defaultRopeParameters() -> [String: RopeParameters] {
        [
            "full_attention": [
                "partial_rotary_factor": .float(0.25),
                "rope_theta": .float(1_000_000),
                "rope_type": .string("proportional"),
            ],
            "sliding_attention": [
                "rope_theta": .float(10_000),
                "rope_type": .string("default"),
            ],
        ]
    }
}

private final class RMSNormNoScale: Module, UnaryLayer {
    let eps: Float

    init(eps: Float) {
        self.eps = eps
        super.init()
    }

    func callAsFunction(_ x: MLXArray) -> MLXArray {
        MLXFast.rmsNorm(x, weight: MLXArray.mlxNone, eps: eps)
    }
}

private final class RMSNormZeroShift: Module, UnaryLayer {
    let weight: MLXArray
    let eps: Float

    init(dimensions: Int, eps: Float) {
        self.weight = MLXArray.ones([dimensions])
        self.eps = eps
        super.init()
    }

    func callAsFunction(_ x: MLXArray) -> MLXArray {
        MLXFast.rmsNorm(x, weight: weight, eps: eps)
    }
}

private final class ProportionalRoPE: Module, OffsetLayer, ArrayOffsetLayer {
    let dims: Int
    let traditional: Bool
    let rotatedDims: Int
    private let _freqs: MLXArray?

    init(
        dims: Int,
        traditional: Bool,
        base: Float,
        scalingConfig: RopeParameters
    ) {
        self.dims = dims
        self.traditional = traditional

        let factor = scalingConfig["factor"]?.asFloat() ?? 1.0
        let partialRotaryFactor = scalingConfig["partial_rotary_factor"]?.asFloat() ?? 1.0
        let rotatedDims = 2 * Int((partialRotaryFactor * Float(dims) / 2).rounded(.down))
        self.rotatedDims = rotatedDims

        if rotatedDims > 0 {
            let exponents = MLXArray(stride(from: 0, to: rotatedDims, by: 2)).asType(.float32)
                / Float(dims)
            self._freqs = factor * MLX.pow(MLXArray(base), exponents)
        } else {
            self._freqs = nil
        }
        super.init()
    }

    func callAsFunction(_ x: MLXArray, offset: Int = 0) -> MLXArray {
        guard rotatedDims > 0, let freqs = _freqs else {
            return x
        }

        let head = x[.ellipsis, ..<dims]
        let tail = x[.ellipsis, dims...]
        let half = dims / 2
        let rotatedHalf = rotatedDims / 2

        let left = head[.ellipsis, ..<half]
        let right = head[.ellipsis, half...]
        let rotated = concatenated(
            [
                left[.ellipsis, ..<rotatedHalf],
                right[.ellipsis, ..<rotatedHalf],
            ],
            axis: -1
        )
        let ropeRotated = MLXFast.RoPE(
            rotated,
            dimensions: rotatedDims,
            traditional: traditional,
            base: nil,
            scale: 1.0,
            offset: offset,
            freqs: freqs
        )
        let newLeft = concatenated(
            [
                ropeRotated[.ellipsis, ..<rotatedHalf],
                left[.ellipsis, rotatedHalf...],
            ],
            axis: -1
        )
        let newRight = concatenated(
            [
                ropeRotated[.ellipsis, rotatedHalf...],
                right[.ellipsis, rotatedHalf...],
            ],
            axis: -1
        )
        let newHead = concatenated([newLeft, newRight], axis: -1)

        if tail.dim(-1) == 0 {
            return newHead
        }

        return concatenated([newHead, tail], axis: -1)
    }

    func callAsFunction(_ x: MLXArray, offset: MLXArray) -> MLXArray {
        let scalarOffset = offset.item(Int.self)
        return callAsFunction(x, offset: scalarOffset)
    }
}

private func makeRoPE(config: Gemma4TextConfiguration, layerType: String) -> any RoPELayer {
    let parameters = config.ropeParameters[layerType] ?? [:]
    let ropeType =
        if case .string(let ropeType) = parameters["rope_type"] {
            ropeType
        } else {
            "default"
        }
    let base = parameters["rope_theta"]?.asFloat() ?? 10_000

    if ropeType == "proportional" {
        return ProportionalRoPE(
            dims: config.globalHeadDim,
            traditional: config.ropeTraditional,
            base: base,
            scalingConfig: parameters
        )
    }

    return initializeRope(
        dims: layerType == "full_attention" ? config.globalHeadDim : config.headDim,
        base: base,
        traditional: config.ropeTraditional,
        scalingConfig: parameters,
        maxPositionEmbeddings: config.maxPositionEmbeddings
    )
}

private struct Gemma4SharedKVState {
    let keys: MLXArray
    let values: MLXArray
}

private struct Gemma4LayerIntermediates {
    let sharedKV: Gemma4SharedKVState?
    let offset: Int?

    init(sharedKV: Gemma4SharedKVState? = nil, offset: Int? = nil) {
        self.sharedKV = sharedKV
        self.offset = offset
    }
}

private final class Gemma4MLP: Module {
    @ModuleInfo(key: "gate_proj") var gateProj: Linear
    @ModuleInfo(key: "down_proj") var downProj: Linear
    @ModuleInfo(key: "up_proj") var upProj: Linear

    init(config: Gemma4TextConfiguration, layerIdx: Int) {
        let firstSharedLayerIdx = config.numHiddenLayers - config.numKvSharedLayers
        let isKvSharedLayer = layerIdx >= firstSharedLayerIdx && config.numKvSharedLayers > 0
        let hiddenDimensions =
            if config.useDoubleWideMLP && isKvSharedLayer {
                config.intermediateSize * 2
            } else {
                config.intermediateSize
            }

        self._gateProj.wrappedValue = Linear(config.hiddenSize, hiddenDimensions, bias: false)
        self._downProj.wrappedValue = Linear(hiddenDimensions, config.hiddenSize, bias: false)
        self._upProj.wrappedValue = Linear(config.hiddenSize, hiddenDimensions, bias: false)
        super.init()
    }

    func callAsFunction(_ x: MLXArray) -> MLXArray {
        downProj(geluApproximate(gateProj(x)) * upProj(x))
    }
}

private final class Gemma4Attention: Module {
    let layerType: String
    let headDim: Int
    let numHeads: Int
    let numKVHeads: Int
    let isKVSharedLayer: Bool

    @ModuleInfo(key: "q_proj") var queryProj: Linear
    @ModuleInfo(key: "k_proj") var keyProj: Linear
    @ModuleInfo(key: "v_proj") var valueProj: Linear
    @ModuleInfo(key: "o_proj") var outputProj: Linear
    @ModuleInfo(key: "q_norm") var queryNorm: RMSNorm
    @ModuleInfo(key: "k_norm") var keyNorm: RMSNorm
    @ModuleInfo(key: "v_norm") var valueNorm: RMSNormNoScale
    @ModuleInfo var rope: any RoPELayer

    init(config: Gemma4TextConfiguration, layerIdx: Int) {
        guard !config.attentionKEqV else {
            fatalError("Gemma 4 attention_k_eq_v is not supported in smrze")
        }

        self.layerType = config.layerTypes[layerIdx]
        self.numHeads = config.numAttentionHeads
        self.numKVHeads = config.numKeyValueHeads
        self.headDim = layerType == "full_attention" ? config.globalHeadDim : config.headDim
        let firstSharedLayerIdx = config.numHiddenLayers - config.numKvSharedLayers
        self.isKVSharedLayer = layerIdx >= firstSharedLayerIdx && config.numKvSharedLayers > 0

        self._queryProj.wrappedValue = Linear(
            config.hiddenSize,
            numHeads * headDim,
            bias: false
        )
        self._keyProj.wrappedValue = Linear(
            config.hiddenSize,
            numKVHeads * headDim,
            bias: false
        )
        self._valueProj.wrappedValue = Linear(
            config.hiddenSize,
            numKVHeads * headDim,
            bias: false
        )
        self._outputProj.wrappedValue = Linear(
            numHeads * headDim,
            config.hiddenSize,
            bias: false
        )
        self._queryNorm.wrappedValue = RMSNorm(dimensions: headDim, eps: config.rmsNormEps)
        self._keyNorm.wrappedValue = RMSNorm(dimensions: headDim, eps: config.rmsNormEps)
        self._valueNorm.wrappedValue = RMSNormNoScale(eps: config.rmsNormEps)
        self._rope.wrappedValue = makeRoPE(config: config, layerType: layerType)
        super.init()
    }

    func callAsFunction(
        _ x: MLXArray,
        mask: MLXFast.ScaledDotProductAttentionMaskMode,
        cache: KVCache?,
        sharedKV: Gemma4SharedKVState? = nil,
        offset: Int? = nil
    ) -> (output: MLXArray, sharedKV: Gemma4SharedKVState, offset: Int) {
        let (batchSize, sequenceLength, _) = (x.dim(0), x.dim(1), x.dim(2))

        var queries = queryProj(x)
        queries = queries.reshaped(batchSize, sequenceLength, numHeads, headDim)
        queries = queryNorm(queries)

        var keys: MLXArray
        var values: MLXArray
        let localOffset: Int

        if let sharedKV {
            keys = sharedKV.keys
            values = sharedKV.values
            localOffset = offset ?? cache?.offset ?? 0
        } else {
            localOffset = offset ?? cache?.offset ?? 0

            keys = keyProj(x).reshaped(batchSize, sequenceLength, numKVHeads, headDim)
            keys = keyNorm(keys)
            keys = keys.transposed(0, 2, 1, 3)
            keys = rope(keys, offset: localOffset)

            values = valueProj(x).reshaped(batchSize, sequenceLength, numKVHeads, headDim)
            values = valueNorm(values)
            values = values.transposed(0, 2, 1, 3)
        }

        queries = queries.transposed(0, 2, 1, 3)
        queries = rope(queries, offset: localOffset)

        if let cache {
            (keys, values) = cache.update(keys: keys, values: values)
        }

        let attentionMask = adjustedAttentionMask(mask, keysSequenceLength: keys.dim(2), dtype: queries.dtype)
        let output = MLXFast.scaledDotProductAttention(
            queries: queries,
            keys: keys,
            values: values,
            scale: 1.0,
            mask: attentionMask
        )
        .transposed(0, 2, 1, 3)
        .reshaped(batchSize, sequenceLength, -1)

        return (
            output: outputProj(output),
            sharedKV: Gemma4SharedKVState(keys: keys, values: values),
            offset: localOffset
        )
    }

    private func adjustedAttentionMask(
        _ mask: MLXFast.ScaledDotProductAttentionMaskMode,
        keysSequenceLength: Int,
        dtype: DType
    ) -> MLXFast.ScaledDotProductAttentionMaskMode {
        guard case .array(let maskArray) = mask else {
            return mask
        }

        let localMask: MLXArray
        if maskArray.dim(-1) == keysSequenceLength {
            localMask = maskArray
        } else {
            let startIndex = max(maskArray.dim(-1) - keysSequenceLength, 0)
            localMask = maskArray[.ellipsis, startIndex...]
        }

        return .array(localMask.asType(dtype))
    }
}

private final class Gemma4DecoderLayer: Module {
    let layerType: String

    @ModuleInfo(key: "self_attn") var selfAttention: Gemma4Attention
    @ModuleInfo var mlp: Gemma4MLP
    @ModuleInfo(key: "input_layernorm") var inputLayerNorm: RMSNorm
    @ModuleInfo(key: "post_attention_layernorm") var postAttentionLayerNorm: RMSNorm
    @ModuleInfo(key: "pre_feedforward_layernorm") var preFeedforwardLayerNorm: RMSNorm
    @ModuleInfo(key: "post_feedforward_layernorm") var postFeedforwardLayerNorm: RMSNorm
    @ModuleInfo(key: "per_layer_input_gate") var perLayerInputGate: Linear
    @ModuleInfo(key: "per_layer_projection") var perLayerProjection: Linear
    @ModuleInfo(key: "post_per_layer_input_norm") var postPerLayerInputNorm: RMSNorm
    @ModuleInfo(key: "layer_scalar") var layerScalar: MLXArray

    init(config: Gemma4TextConfiguration, layerIdx: Int) {
        self.layerType = config.layerTypes[layerIdx]
        self._selfAttention.wrappedValue = Gemma4Attention(config: config, layerIdx: layerIdx)
        self._mlp.wrappedValue = Gemma4MLP(config: config, layerIdx: layerIdx)
        self._inputLayerNorm.wrappedValue = RMSNorm(
            dimensions: config.hiddenSize,
            eps: config.rmsNormEps
        )
        self._postAttentionLayerNorm.wrappedValue = RMSNorm(
            dimensions: config.hiddenSize,
            eps: config.rmsNormEps
        )
        self._preFeedforwardLayerNorm.wrappedValue = RMSNorm(
            dimensions: config.hiddenSize,
            eps: config.rmsNormEps
        )
        self._postFeedforwardLayerNorm.wrappedValue = RMSNorm(
            dimensions: config.hiddenSize,
            eps: config.rmsNormEps
        )
        self._perLayerInputGate.wrappedValue = Linear(
            config.hiddenSize,
            config.hiddenSizePerLayerInput,
            bias: false
        )
        self._perLayerProjection.wrappedValue = Linear(
            config.hiddenSizePerLayerInput,
            config.hiddenSize,
            bias: false
        )
        self._postPerLayerInputNorm.wrappedValue = RMSNorm(
            dimensions: config.hiddenSize,
            eps: config.rmsNormEps
        )
        self._layerScalar.wrappedValue = MLXArray.ones([1])
        super.init()
    }

    func callAsFunction(
        _ x: MLXArray,
        mask: MLXFast.ScaledDotProductAttentionMaskMode,
        cache: KVCache?,
        perLayerInput: MLXArray?,
        sharedKV: Gemma4SharedKVState?,
        offset: Int?
    ) -> (hidden: MLXArray, sharedKV: Gemma4SharedKVState, offset: Int) {
        let attention = selfAttention(
            inputLayerNorm(x),
            mask: mask,
            cache: cache,
            sharedKV: sharedKV,
            offset: offset
        )
        var hidden = attention.output
        hidden = postAttentionLayerNorm(hidden)
        hidden = x + hidden

        let residual = hidden
        hidden = preFeedforwardLayerNorm(hidden)
        hidden = mlp(hidden)
        hidden = postFeedforwardLayerNorm(hidden)
        hidden = residual + hidden

        if let perLayerInput {
            let perLayerResidual = hidden
            var gate = perLayerInputGate(hidden)
            gate = geluApproximate(gate)
            gate = gate * perLayerInput
            gate = perLayerProjection(gate)
            gate = postPerLayerInputNorm(gate)
            hidden = perLayerResidual + gate
        }

        return (
            hidden: hidden * layerScalar,
            sharedKV: attention.sharedKV,
            offset: attention.offset
        )
    }
}

final class Gemma4TextModel: Module {
    @ModuleInfo(key: "embed_tokens") var embedTokens: Embedding
    @ModuleInfo fileprivate var layers: [Gemma4DecoderLayer]
    @ModuleInfo var norm: RMSNorm
    @ModuleInfo(key: "embed_tokens_per_layer") var embedTokensPerLayer: Embedding
    @ModuleInfo(key: "per_layer_model_projection") var perLayerModelProjection: Linear
    @ModuleInfo(key: "per_layer_projection_norm") fileprivate var perLayerProjectionNorm: RMSNormZeroShift

    let config: Gemma4TextConfiguration
    let embedScale: Float
    let embedTokensPerLayerScale: Float
    private let _perLayerInputScale: MLXArray
    private let _perLayerProjectionScale: MLXArray
    let firstKVSharedLayerIdx: Int
    let previousKVs: [Int]

    init(config: Gemma4TextConfiguration) {
        guard !config.enableMoeBlock else {
            fatalError("Gemma 4 MoE text blocks are not supported in smrze")
        }

        self.config = config
        self.embedScale = pow(Float(config.hiddenSize), 0.5)
        self.embedTokensPerLayerScale = pow(Float(config.hiddenSizePerLayerInput), 0.5)
        self._perLayerInputScale = MLXArray(pow(2.0, -0.5))
        self._perLayerProjectionScale = MLXArray(pow(Float(config.hiddenSize), -0.5))
        self.firstKVSharedLayerIdx = config.numHiddenLayers - config.numKvSharedLayers

        var previousKVs = Array(0 ..< config.numHiddenLayers)
        if config.numKvSharedLayers > 0 {
            let concreteLayerTypes = Array(config.layerTypes[..<firstKVSharedLayerIdx])
            var kvsByType = [String: Int]()
            for (index, layerType) in concreteLayerTypes.enumerated() {
                kvsByType[layerType] = index
            }
            for index in firstKVSharedLayerIdx ..< config.numHiddenLayers {
                previousKVs[index] = kvsByType[config.layerTypes[index]] ?? 0
            }
        }
        self.previousKVs = previousKVs

        self._embedTokens.wrappedValue = Embedding(
            embeddingCount: config.vocabSize,
            dimensions: config.hiddenSize
        )
        self._layers.wrappedValue = (0 ..< config.numHiddenLayers).map {
            Gemma4DecoderLayer(config: config, layerIdx: $0)
        }
        self._norm.wrappedValue = RMSNorm(
            dimensions: config.hiddenSize,
            eps: config.rmsNormEps
        )
        self._embedTokensPerLayer.wrappedValue = Embedding(
            embeddingCount: config.vocabSizePerLayerInput,
            dimensions: config.numHiddenLayers * config.hiddenSizePerLayerInput
        )
        self._perLayerModelProjection.wrappedValue = Linear(
            config.hiddenSize,
            config.numHiddenLayers * config.hiddenSizePerLayerInput,
            bias: false
        )
        self._perLayerProjectionNorm.wrappedValue = RMSNormZeroShift(
            dimensions: config.hiddenSizePerLayerInput,
            eps: config.rmsNormEps
        )
        super.init()
    }

    func callAsFunction(
        inputs: MLXArray? = nil,
        inputsEmbeds: MLXArray? = nil,
        mask: MLXFast.ScaledDotProductAttentionMaskMode? = nil,
        cache: [KVCache?]? = nil,
        perLayerInputs: MLXArray? = nil
    ) -> MLXArray {
        let hidden: MLXArray
        if let inputsEmbeds {
            hidden = inputsEmbeds
        } else if let inputs {
            var embedded = embedTokens(inputs)
            embedded = (embedded * MLXArray(embedScale, dtype: .float32)).asType(embedded.dtype)
            hidden = embedded
        } else {
            fatalError("Gemma 4 text model requires input ids or embeddings")
        }

        let perLayerInputList: [MLXArray?]
        if config.hiddenSizePerLayerInput > 0 {
            let preparedPerLayerInputs =
                if let perLayerInputs {
                    perLayerInputs
                } else if let inputs {
                    getPerLayerInputs(inputs)
                } else {
                    fatalError("Gemma 4 text model requires per-layer inputs")
                }

            let projectedPerLayerInputs = projectPerLayerInputs(
                hidden,
                perLayerInputs: preparedPerLayerInputs
            )
            perLayerInputList = layers.enumerated().map { index, _ in
                projectedPerLayerInputs[0..., 0..., index, 0...]
            }
        } else {
            perLayerInputList = Array(repeating: nil, count: layers.count)
        }

        let cacheArray: [KVCache?] =
            if let cache {
                cache + Array(repeating: nil, count: max(layers.count - cache.count, 0))
            } else {
                Array(repeating: nil, count: layers.count)
            }

        let masks =
            if let mask {
                Array(repeating: mask, count: layers.count)
            } else {
                makeMasks(hidden: hidden, cache: cacheArray)
            }

        var output = hidden
        var intermediates = Array(
            repeating: Gemma4LayerIntermediates(),
            count: layers.count
        )
        for (index, layer) in layers.enumerated() {
            let previousIntermediate = intermediates[previousKVs[index]]
            let layerResult = layer(
                output,
                mask: masks[index],
                cache: cacheArray[index],
                perLayerInput: perLayerInputList[index],
                sharedKV: previousIntermediate.sharedKV,
                offset: previousIntermediate.offset
            )
            output = layerResult.hidden
            intermediates[index] = Gemma4LayerIntermediates(
                sharedKV: layerResult.sharedKV,
                offset: layerResult.offset
            )
        }

        return norm(output)
    }

    func getPerLayerInputs(_ inputIds: MLXArray) -> MLXArray {
        let validMask = logicalAnd(
            inputIds .>= 0,
            inputIds .< config.vocabSizePerLayerInput
        )
        let tokens = MLX.where(validMask, inputIds, MLXArray.zeros(like: inputIds))
        var result = embedTokensPerLayer(tokens)
        result = (result * MLXArray(embedTokensPerLayerScale, dtype: .float32)).asType(result.dtype)
        return result.reshaped(
            Array(inputIds.shape) + [config.numHiddenLayers, config.hiddenSizePerLayerInput]
        )
    }

    func projectPerLayerInputs(_ inputsEmbeds: MLXArray, perLayerInputs: MLXArray) -> MLXArray {
        var projection = perLayerModelProjection(inputsEmbeds)
        projection = projection * _perLayerProjectionScale.asType(inputsEmbeds.dtype)
        projection = projection.reshaped(
            Array(inputsEmbeds.shape.dropLast()) + [
                config.numHiddenLayers,
                config.hiddenSizePerLayerInput,
            ]
        )
        projection = perLayerProjectionNorm(projection)

        var adjustedInputs = perLayerInputs
        if adjustedInputs.shape != projection.shape {
            let targetLayers = min(
                config.numHiddenLayers,
                adjustedInputs.shape[adjustedInputs.shape.count - 2]
            )
            adjustedInputs = adjustedInputs[.ellipsis, ..<targetLayers, 0...]
        }

        return (projection + adjustedInputs) * _perLayerInputScale.asType(inputsEmbeds.dtype)
    }

    private func makeMasks(
        hidden: MLXArray,
        cache: [KVCache?]
    ) -> [MLXFast.ScaledDotProductAttentionMaskMode] {
        var masksByType = [String: MLXFast.ScaledDotProductAttentionMaskMode]()

        return zip(layers, cache).map { layer, layerCache in
            if let mask = masksByType[layer.layerType] {
                return mask
            }

            let mask =
                if layer.layerType == "full_attention" {
                    createAttentionMask(h: hidden, cache: layerCache)
                } else {
                    createAttentionMask(
                        h: hidden,
                        cache: layerCache,
                        windowSize: config.slidingWindow
                    )
                }
            masksByType[layer.layerType] = mask
            return mask
        }
    }

    func newCache(parameters: GenerateParameters?) -> [any KVCache] {
        config.layerTypes[..<firstKVSharedLayerIdx].map { layerType in
            if layerType == "full_attention" {
                KVCacheSimple()
            } else {
                RotatingKVCache(maxSize: config.slidingWindow, keep: 0)
            }
        }
    }
}

final class Gemma4LanguageModel: Module {
    @ModuleInfo var model: Gemma4TextModel

    let config: Gemma4TextConfiguration

    init(config: Gemma4TextConfiguration) {
        self.config = config
        self.model = Gemma4TextModel(config: config)
        super.init()
    }

    func callAsFunction(
        inputs: MLXArray? = nil,
        inputsEmbeds: MLXArray? = nil,
        mask: MLXFast.ScaledDotProductAttentionMaskMode? = nil,
        cache: [KVCache?]? = nil,
        perLayerInputs: MLXArray? = nil
    ) -> MLXArray {
        var output = model(
            inputs: inputs,
            inputsEmbeds: inputsEmbeds,
            mask: mask,
            cache: cache,
            perLayerInputs: perLayerInputs
        )
        output = model.embedTokens.asLinear(output)
        output = tanh(output / config.finalLogitSoftcapping) * config.finalLogitSoftcapping
        return output
    }

    func newCache(parameters: GenerateParameters?) -> [any KVCache] {
        model.newCache(parameters: parameters)
    }
}

final class Gemma4TextModelWrapper: Module, LLMModel {
    @ModuleInfo(key: "language_model") var languageModel: Gemma4LanguageModel

    let config: Gemma4TextConfiguration

    init(config: Gemma4TextConfiguration) {
        self.config = config
        self._languageModel.wrappedValue = Gemma4LanguageModel(config: config)
        super.init()
    }

    var loraLayers: [Module] {
        languageModel.model.layers
    }

    func newCache(parameters: GenerateParameters?) -> [any KVCache] {
        languageModel.newCache(parameters: parameters)
    }

    func messageGenerator(tokenizer _: any Tokenizer) -> any MessageGenerator {
        DefaultMessageGenerator()
    }

    func callAsFunction(_ inputs: MLXArray, cache: [KVCache]?) -> MLXArray {
        let cacheArray = cache?.map { $0 as KVCache? }
        return languageModel(inputs: inputs, cache: cacheArray)
    }

    func sanitize(weights: [String: MLXArray]) -> [String: MLXArray] {
        var processedWeights = [String: MLXArray]()

        for (key, value) in weights {
            if key.contains("vision_tower")
                || key.contains("audio_tower")
                || key.contains("embed_vision")
                || key.contains("embed_audio")
                || key.contains("self_attn.rotary_emb")
            {
                continue
            }

            if key.contains("input_max")
                || key.contains("input_min")
                || key.contains("output_max")
                || key.contains("output_min")
            {
                continue
            }

            var newKey = key
            if newKey.hasPrefix("model.") {
                newKey.removeFirst("model.".count)
            }
            if newKey.hasPrefix("language_model.")
                && !newKey.hasPrefix("language_model.model.")
            {
                let rest = newKey.dropFirst("language_model.".count)
                newKey = "language_model.model.\(rest)"
            }

            processedWeights[newKey] = value
        }

        let expectedVocab = config.vocabSize
        let vocabKeys = [
            "language_model.model.embed_tokens.weight",
            "language_model.model.embed_tokens.scales",
            "language_model.model.embed_tokens.biases",
            "language_model.model.embed_tokens_per_layer.weight",
            "language_model.model.embed_tokens_per_layer.scales",
            "language_model.model.embed_tokens_per_layer.biases",
        ]
        for key in vocabKeys {
            if let tensor = processedWeights[key], tensor.dim(0) > expectedVocab {
                processedWeights[key] = tensor[0 ..< expectedVocab]
            }
        }

        return processedWeights
    }
}

func makeGemma4Model(configurationData: Data) throws -> any LanguageModel {
    let configuration = try JSONDecoder.json5().decode(Gemma4Configuration.self, from: configurationData)
    return Gemma4TextModelWrapper(config: configuration.textConfig)
}
