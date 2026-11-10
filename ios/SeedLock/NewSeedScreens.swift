import SwiftUI

/// Creating a new seed (the command-line program's menu 1): its words, a check of them, then its backup.
struct NewSeedScreen: View {
    let model: AppModel
    @State private var dice = false
    @State private var problem: String?

    var body: some View {
        List {
            Section {
                Text(verbatim: "You are about to create a NEW seed. Do this in private: nobody looking, and no camera "
                    + "pointed at the screen.")
            }
            Section {
                Toggle("Add my own dice rolls as extra randomness", isOn: $dice)
            } footer: {
                Text(verbatim: "Your rolls are mixed with the iPhone's own randomness, so a few careless rolls cannot "
                    + "hurt.")
            }
            Section {
                Button("24 words (recommended)") {
                    start(24)
                }
                Button("12 words") {
                    start(12)
                }
            } header: {
                Text(verbatim: "How many words?")
            }
            if let problem {
                Section {
                    Text(verbatim: problem)
                        .foregroundStyle(Color.red)
                }
            }
        }
        .navigationTitle("New seed")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func start(_ words: Int32) {
        if dice {
            model.showInstead(.dice(words))
            return
        }
        do {
            let seed = try Seed.generate(words: words)
            model.showInstead(.newWords(OpenSeed(seed)))
        } catch {
            problem = "Not created: \(userText(error))"
        }
    }
}

/// Dice rolls typed in, with a counter, for a new seed.
struct DiceScreen: View {
    let model: AppModel
    let words: Int32
    @State private var rolls = ""
    @State private var problem: String?
    @FocusState private var typing: Bool

    var body: some View {
        let need = (try? diceRollsNeeded(words: words, diceOnly: false)) ?? 0
        List {
            Section {
                Text(verbatim: "Roll a six-sided die \(need) times, and type each result (the digits 1 to 6).")
                TextField("Your rolls", text: $rolls, axis: .vertical)
                    .lineLimit(2...6)
                    .font(.system(.body, design: .monospaced))
                    .keyboardType(.numberPad)
                    .autocorrectionDisabled(true)
                    .focused($typing)
            } footer: {
                Text(verbatim: counter(need))
            }
            if let problem {
                Section {
                    Text(verbatim: problem)
                        .foregroundStyle(Color.red)
                }
            }
            Section {
                Button("Create the seed") {
                    create()
                }
            }
        }
        .navigationTitle("Dice rolls")
        .navigationBarTitleDisplayMode(.inline)
        .scrollDismissesKeyboard(.interactively)
        .toolbar {
            // the number keyboard has no key to close it
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button("Done") {
                    typing = false
                }
            }
        }
    }

    private func counter(_ need: Int32) -> String {
        do {
            let count = try countDiceRolls(text: rolls)
            return count >= need ? "\(count) of \(need) rolls: enough." : "\(count) of \(need) rolls"
        } catch {
            return userText(error)
        }
    }

    private func create() {
        let typed = rolls
        do {
            let seed = try Seed.fromDice(rolls: typed, words: words, diceOnly: false)
            rolls = ""
            model.showInstead(.newWords(OpenSeed(seed)))
        } catch {
            problem = "Not created: \(userText(error))"
        }
    }
}

/// A new seed's words, to write on paper.
struct NewWordsScreen: View {
    let model: AppModel
    let open: OpenSeed

    var body: some View {
        List {
            // read here, and never kept by the screen: closing the screen lets go of the seed (OpenSeed.close)
            if let seed = open.seed, let words = try? seed.words() {
                Section {
                    WordList(words: words)
                } header: {
                    Text(verbatim: "Your new seed: \(words.count) words")
                } footer: {
                    Text(verbatim: "Write them on paper, in this exact order. Nobody may see them, and iOS lets no app "
                        + "block a screenshot: take none.")
                }
                Section {
                    LabeledContent("Wallet fingerprint", value: (try? seed.fingerprint()) ?? "unknown")
                } footer: {
                    Text(verbatim: "A short ID that lets you recognise this wallet later.")
                }
                Section {
                    Button("I wrote every word down") {
                        model.showInstead(.quiz(open))
                    }
                }
            }
        }
        .navigationTitle("Your new seed")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// Three of the new seed's words, typed back from the paper, to make sure they were copied correctly.
struct QuizScreen: View {
    let model: AppModel
    let open: OpenSeed
    @State private var picks: [Int]
    @State private var first = ""
    @State private var second = ""
    @State private var third = ""
    @State private var note: String?

    init(model: AppModel, open: OpenSeed) {
        self.model = model
        self.open = open
        _picks = State(initialValue: quizPicks(count: Int(open.seed?.wordCount() ?? 12)))
    }

    var body: some View {
        List {
            if open.seed != nil, picks.count == 3 {
                Section {
                    wordField(0, $first)
                    wordField(1, $second)
                    wordField(2, $third)
                } header: {
                    Text(verbatim: "Type these three words from your paper")
                } footer: {
                    Text(verbatim: "To make sure they were copied correctly.")
                }
                if let note {
                    Section {
                        Text(verbatim: note)
                            .font(.footnote)
                    }
                }
                Section {
                    Button("Check") {
                        check()
                    }
                }
            }
        }
        .navigationTitle("Quick check")
        .navigationBarTitleDisplayMode(.inline)
        .onChange(of: first + second + third) { old, new in
            if new.count > old.count + 1, let said = model.pastedSecret() {
                note = said
            }
        }
    }

    private func wordField(_ index: Int, _ text: Binding<String>) -> some View {
        TextField("Word #\(picks[index] + 1)", text: text)
            .autocorrectionDisabled(true)
            .textInputAutocapitalization(.never)
            .keyboardType(.asciiCapable)
    }

    private func check() {
        guard let seed = open.seed, let words = try? seed.words() else { return }
        let typed = [first, second, third]
        first = ""
        second = ""
        third = ""
        if picks.indices.allSatisfy({ sameWord(typed[$0], words[picks[$0]]) }) {
            model.showInstead(.saveSeed(open))
        } else {
            model.showInstead(.newWords(open))
            model.say("Not the same", "That did not match the words you were shown. Look at them again, and correct "
                + "your paper where it differs.")
        }
    }
}

/// The words of a seed one already has, to encrypt into a backup (the command-line program's menu 2).
struct EnterWordsScreen: View {
    let model: AppModel
    @State private var words = ""
    @State private var problem: String?
    @State private var note: String?

    var body: some View {
        List {
            Section {
                TextField("Your words", text: $words, axis: .vertical)
                    .lineLimit(3...8)
                    .autocorrectionDisabled(true)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.asciiCapable)
            } header: {
                Text(verbatim: "The words of your seed")
            } footer: {
                Text(verbatim: "12 to 24 words, separated by spaces. The first four letters of each word are enough. "
                    + "Do this in private: nobody looking, and no camera pointed at the screen.")
            }
            if let problem {
                Section {
                    Text(verbatim: problem)
                        .foregroundStyle(Color.red)
                }
            }
            if let note {
                Section {
                    Text(verbatim: note)
                        .font(.footnote)
                }
            }
            Section {
                Button("Next") {
                    next()
                }
            }
        }
        .navigationTitle("Encrypt a seed")
        .navigationBarTitleDisplayMode(.inline)
        .onChange(of: words) { old, new in
            if new.count > old.count + 1, let said = model.pastedSecret() {
                note = said
            }
        }
    }

    private func next() {
        let typed = words
        do {
            let seed = try Seed.fromWords(words: typed)
            words = ""
            model.showInstead(.saveSeed(OpenSeed(seed)))
        } catch {
            problem = userText(error)
        }
    }
}

/// The words of an opened backup.
struct WordsScreen: View {
    let model: AppModel
    let open: OpenSeed

    var body: some View {
        List {
            if let seed = open.seed, let words = try? seed.words() {
                Section {
                    WordList(words: words)
                } header: {
                    Text(verbatim: "The \(words.count) words of this seed, in order")
                } footer: {
                    Text(verbatim: "Make sure nobody can see your screen. iOS lets no app block a screenshot: take "
                        + "none.")
                }
                Section {
                    LabeledContent("Wallet fingerprint", value: (try? seed.fingerprint()) ?? "unknown")
                }
                Section {
                    Button("Hide the words") {
                        model.back()
                    }
                }
            }
        }
        .navigationTitle("The words")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// A seed's words, numbered as on paper, in two columns: the first half on the left.
struct WordList: View {
    let words: [String]

    var body: some View {
        let half = (words.count + 1) / 2
        HStack(alignment: .top, spacing: 28) {
            column(0..<half)
            column(half..<words.count)
        }
        .font(.system(.body, design: .monospaced))
        .padding(.vertical, 6)
    }

    private func column(_ range: Range<Int>) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(range, id: \.self) { index in
                Text(verbatim: numberedWord(index, words[index]))
            }
        }
    }
}
