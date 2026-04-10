import Foundation

@main
struct App {
    static func main() async {
        do {
            let request = try decodeRequest()
            let summary = try await FoundationModelSummarizer().summarize(request)
            try writeResponse(summary)
        } catch {
            fputs("\(error.localizedDescription)\n", stderr)
            Foundation.exit(1)
        }
    }

    private static func decodeRequest() throws -> SummaryRequest {
        let data = FileHandle.standardInput.readDataToEndOfFile()
        do {
            return try JSONDecoder().decode(SummaryRequest.self, from: data)
        } catch {
            throw SummaryHelperError.invalidRequest("failed to decode summary request: \(error)")
        }
    }

    private static func writeResponse(_ summary: SummaryResponse) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(summary)
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0A]))
    }
}
