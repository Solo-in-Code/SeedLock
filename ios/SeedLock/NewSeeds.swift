import Foundation

/// Encrypts `seed` into a backup with `password`, saves it as `name` among `backups`, then reads the saved file back
/// and opens it with the password: proof that what is stored opens (as the command-line program and the Android app
/// do). Gives the wallet's fingerprint. A saved backup that does not open to the same seed is deleted again, so that
/// nothing is left to rely on. Slow (Argon2id, twice): away from the screen's thread.
func saveNewBackup(
    _ seed: Seed, name: String, password: Data, strength: BackupStrength, in backups: Backups
) throws -> String {
    let backup = try seed.encrypt(password: password, strength: strength)
    try backups.save(name, contents: backup.fileContents)
    do {
        let saved = try backups.read(name)
        let reopened = try Seed.unlock(backupText: saved, password: password)
        if !reopened.sameAs(other: seed) {
            throw UserError("it opens to another seed")
        }
    } catch {
        backups.delete(name)
        throw UserError("The saved backup did not open again (\(userText(error))), so SeedLock deleted it. Please "
            + "try again.")
    }
    return try seed.fingerprint()
}

/// Three different positions among `count` words, in order, chosen at random (with the system's secure random
/// numbers).
func quizPicks(count: Int) -> [Int] {
    var picks = Set<Int>()
    while picks.count < min(3, count) {
        picks.insert(Int.random(in: 0..<count))
    }
    return picks.sorted()
}

/// Whether a word typed back from the paper is `word`: the whole word, or its first letters (3 or more) when no other
/// word starts with them.
func sameWord(_ typed: String, _ word: String) -> Bool {
    let typed = typed.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    return typed == word || (typed.count >= 3 && word.hasPrefix(typed) && wordSuggestions(prefix: typed).count == 1)
}

/// A word with its number, as on paper: " 1. abandon".
func numberedWord(_ index: Int, _ word: String) -> String {
    let number = String(index + 1)
    return String(repeating: " ", count: max(0, 2 - number.count)) + number + ". " + word
}
