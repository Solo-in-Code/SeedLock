import SwiftUI

/// SeedLock's screens, one above the other (a navigation stack), and what shows over them: the prompt for a backup's
/// password, and messages.
struct RootView: View {
    let model: AppModel
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        NavigationStack(path: Binding(get: { model.path }, set: { model.setPath($0) })) {
            HomeScreen(model: model)
                .navigationDestination(for: Route.self) { route in
                    screen(route)
                }
        }
        .sheet(item: Binding(get: { model.prompt }, set: { if $0 == nil { model.closePrompt() } })) { request in
            PasswordPrompt(model: model, request: request)
        }
        .alert(
            model.message?.title ?? "",
            isPresented: Binding(get: { model.message != nil }, set: { if !$0 { model.message = nil } }),
            presenting: model.message
        ) { _ in
            Button("OK") {}
        } message: { message in
            Text(verbatim: message.text)
        }
        .onChange(of: scenePhase) { _, phase in
            switch phase {
            case .active:
                model.phaseChanged(.active)
            case .inactive:
                model.phaseChanged(.inactive)
            case .background:
                model.phaseChanged(.background)
            @unknown default:
                model.phaseChanged(.background)
            }
        }
        .onAppear {
            model.start()
        }
    }

    @ViewBuilder
    private func screen(_ route: Route) -> some View {
        switch route {
        case .addBackup:
            AddBackupScreen(model: model)
        case .backup(let name):
            BackupScreen(model: model, name: name)
        case .check(let open):
            CheckScreen(model: model, open: open)
        case .words(let open):
            WordsScreen(model: model, open: open)
        case .newSeed:
            NewSeedScreen(model: model)
        case .dice(let words):
            DiceScreen(model: model, words: words)
        case .newWords(let open):
            NewWordsScreen(model: model, open: open)
        case .quiz(let open):
            QuizScreen(model: model, open: open)
        case .enterWords:
            EnterWordsScreen(model: model)
        case .saveSeed(let open):
            SaveSeedScreen(model: model, open: open)
        case .qr(let shown):
            QrScreen(shown: shown)
        case .sparrow(let open, let name):
            SparrowScreen(model: model, open: open, name: name)
        case .sign(let name):
            SignScreen(model: model, name: name)
        case .review(let signing):
            ReviewScreen(model: model, signing: signing)
        case .signed(let shown):
            SignedScreen(model: model, shown: shown)
        case .addMultisig:
            AddMultisigScreen(model: model)
        case .checkMultisig(let shown):
            CheckMultisigScreen(model: model, shown: shown)
        case .multisig(let name):
            MultisigScreen(model: model, name: name)
        case .cosigner(let shown):
            CosignerScreen(shown: shown)
        case .about:
            AboutScreen()
        }
    }
}
