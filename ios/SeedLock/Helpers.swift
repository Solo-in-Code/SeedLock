import Foundation

/// An error whose text is written for the person.
struct UserError: LocalizedError {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var errorDescription: String? {
        text
    }
}

/// The text to show for an error: SeedLock's own errors carry a message written for the person.
func userText(_ error: Error) -> String {
    if let e = error as? SeedLockError {
        switch e {
        case .WrongPassword(let message), .Failed(let message):
            return message
        }
    }
    return error.localizedDescription
}

/// The contents of a file, `limit` bytes at most: a larger file is not what it should be.
func readFile(_ url: URL, limit: Int, what: String) throws -> Data {
    let handle = try FileHandle(forReadingFrom: url)
    defer { try? handle.close() }
    let data = try handle.read(upToCount: limit + 1) ?? Data()
    if data.count > limit {
        throw UserError("That file is too large to be \(what).")
    }
    return data
}

/// Runs slow work away from the screen's thread (opening a backup: Argon2id takes a few seconds and up to 256 MiB), on
/// a thread of its own with a large stack, and hands its result to `done` on the screen's thread.
func inBackground<T>(_ work: @escaping () throws -> T, done: @escaping (Result<T, Error>) -> Void) {
    let thread = Thread {
        let result = Result<T, Error>(catching: work)
        DispatchQueue.main.async {
            done(result)
        }
    }
    thread.stackSize = 16 << 20
    thread.start()
}
