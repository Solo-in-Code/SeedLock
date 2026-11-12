import SwiftUI

/// Where SeedLock's source code is.
let sourceCode = "https://github.com/Solo-in-Code/SeedLock"

/// What SeedLock is, its license, and the license notices of the libraries it is built from.
struct AboutScreen: View {
    var body: some View {
        List {
            Section {
                Text(verbatim: "An offline tool for Bitcoin seeds: encrypted backups, the wallet for Sparrow, signing, "
                    + "multisig. Free software under the MIT license. It is not independently audited. This iPhone app "
                    + "is a preview: built on the same library as the Android app, it does what that app does.")
                Text(verbatim: "Its source code: \(sourceCode)")
                    .textSelection(.enabled)
            } header: {
                Text(verbatim: "SeedLock \(seedlockVersion())")
            }
            if let notices = Notices.shared {
                ForEach(notices.parts) { part in
                    Section {
                        ForEach(part.paragraphs.indices, id: \.self) { i in
                            Text(verbatim: part.paragraphs[i])
                                .font(.footnote)
                        }
                    } header: {
                        Text(verbatim: part.heading)
                    }
                }
            } else {
                Section {
                    Text(verbatim: "The licenses of the libraries SeedLock is built from are missing from this "
                        + "build of the app (ios/build-rust.sh writes them before the app is built).")
                        .foregroundStyle(Color.red)
                }
            }
        }
        .navigationTitle("About SeedLock")
        .navigationBarTitleDisplayMode(.inline)
    }
}
