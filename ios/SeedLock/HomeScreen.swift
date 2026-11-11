import SwiftUI

/// The first screen: the backups on this iPhone, and what can be done. While the iPhone is online, it says only that
/// SeedLock works only offline, and why the iPhone may be online.
struct HomeScreen: View {
    let model: AppModel

    var body: some View {
        List {
            if let notice = model.notice {
                Section {
                    Text(verbatim: notice)
                    Button("OK") {
                        model.notice = nil
                    }
                }
            }
            if let online = model.online {
                if online.isEmpty {
                    offline
                } else {
                    onlineWarning(online)
                }
            } else {
                Section {
                    Text(verbatim: "Looking at the network ...")
                }
            }
        }
        .navigationTitle("SeedLock")
    }

    @ViewBuilder
    private func onlineWarning(_ reasons: [String]) -> some View {
        Section {
            Text(verbatim: "SEEDLOCK WORKS ONLY OFFLINE. " + reasons.joined(separator: " ")
                + " Turn on airplane mode, keep Wi-Fi, mobile data and Bluetooth off, and turn off any VPN. SeedLock "
                + "opens as soon as the iPhone is offline.")
                .foregroundStyle(Color.red)
        } footer: {
            Text(verbatim: "SeedLock has no code that connects to anything (iOS has no permission for the internet "
                + "that an app could go without, as Android has). It still does nothing while the iPhone is online, "
                + "and if the iPhone goes online while SeedLock is open, it closes everything at once. SeedLock "
                + "\(seedlockVersion()), iPhone app preview.")
        }
    }

    @ViewBuilder
    private var offline: some View {
        Section {
            Label("This iPhone is offline: no network is connected.", systemImage: "checkmark.shield")
        }
        Section {
            if model.entries.isEmpty {
                Text(verbatim: "None yet. Add the backup you made on your computer or on another phone, or create a "
                    + "new seed.")
            }
            ForEach(model.entries, id: \.name) { entry in
                NavigationLink(value: Route.backup(entry.name)) {
                    Text(verbatim: entry.label)
                }
            }
            NavigationLink(value: Route.addBackup) {
                Label("Add a backup (its file, or its text)", systemImage: "plus")
            }
        } header: {
            Text(verbatim: "Backups on this iPhone")
        } footer: {
            if model.entries.contains(where: { $0.fingerprint == nil }) {
                Text(verbatim: "A backup's wallet fingerprint shows beside its name once the backup was opened on this "
                    + "iPhone.")
            }
        }
        Section {
            NavigationLink(value: Route.newSeed) {
                Text(verbatim: "Create a new seed and its backup")
            }
            NavigationLink(value: Route.enterWords) {
                Text(verbatim: "Encrypt the words of a seed I already have")
            }
        } header: {
            Text(verbatim: "New")
        }
        Section {
            NavigationLink(value: Route.about) {
                Text(verbatim: "About SeedLock and its licenses")
            }
        } footer: {
            Text(verbatim: "SeedLock \(seedlockVersion()), iPhone app preview: so far it makes, keeps and checks "
                + "backups, lays out their wallets for Sparrow, and signs transactions. It is not independently "
                + "audited: try it with a test wallet or a small amount first.")
        }
    }
}
