import Foundation

/// The license notices: the app's file notices.txt, which ios/build-rust.sh writes from the very libraries the app is
/// built with (crates/seedlock-tools). Paragraphs are separated by a blank line, and headings are underlined with "=".
struct Notices {
    struct Part: Identifiable {
        let id: Int
        let heading: String
        var paragraphs: [String]
    }

    let parts: [Part]

    /// Read once, from the app's files; nil when the notices are missing.
    static let shared: Notices? = {
        guard let url = Bundle.main.url(forResource: "notices", withExtension: "txt")
            ?? Bundle.main.url(forResource: "notices", withExtension: "txt", subdirectory: "Generated"),
            let data = try? readFile(url, limit: 4 << 20, what: "the license notices"),
            let text = String(data: data, encoding: .utf8)
        else { return nil }
        return Notices(text: text)
    }()

    init(text: String) {
        // the first paragraph is the notices' title: this screen has its own
        let blocks = text.trimmingCharacters(in: .whitespacesAndNewlines)
            .components(separatedBy: "\n\n")
            .dropFirst()
            .map { $0.trimmingCharacters(in: .newlines) }
            .filter { !$0.isEmpty }
        var parts: [Part] = []
        for block in blocks {
            let lines = block.components(separatedBy: "\n")
            if lines.count == 2, !lines[1].isEmpty, lines[1].allSatisfy({ $0 == "=" }) {
                parts.append(Part(id: parts.count, heading: lines[0], paragraphs: []))
            } else if parts.isEmpty {
                parts.append(Part(id: 0, heading: "", paragraphs: [block]))
            } else {
                parts[parts.count - 1].paragraphs.append(block)
            }
        }
        self.parts = parts
    }
}
