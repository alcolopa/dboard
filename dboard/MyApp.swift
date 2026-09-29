import SwiftUI

@main
struct MyApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .background(FullScreenEnabler())
                .frame(minWidth: 960, idealWidth: 1280, maxWidth: .infinity, minHeight: 620, idealHeight: 800, maxHeight: .infinity)
        }
        .windowStyle(.titleBar)
        .windowToolbarStyle(.unified(showsTitle: false))
        .commands {
            SidebarCommands()

            ToolbarCommands()
            TextEditingCommands()

            CommandGroup(replacing: .appSettings) {
                Button("Settings...") {
                    TabManager.shared.openSettingsTab()
                }
                .keyboardShortcut(",", modifiers: [.command])
            }

            CommandGroup(replacing: .newItem) {
                Button("New SQL Query") {
                    TabManager.shared.openQueryTab()
                }
                .keyboardShortcut("n", modifiers: [.command])
            }

            CommandMenu("Database") {
                Button("Undo Database Edit") {
                    Task {
                        if let entry = EditHistoryManager.shared.popUndoEntry(), let reverseOp = entry.reverseOperation, let driver = ConnectionManager.shared.activeDriver {
                            _ = try? await driver.executeQuery(sql: reverseOp.statement, database: ConnectionManager.shared.activeDatabase)
                            ToastManager.shared.show("Reverted Edit", subtitle: entry.summaryText, style: .info)
                        }
                    }
                }
                .keyboardShortcut("z", modifiers: [.command, .option])
            }
        }
    }
}

/// Ensures the window supports the standard macOS full-screen mode (⌃⌘F, green button).
private struct FullScreenEnabler: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async {
            view.window?.collectionBehavior.insert(.fullScreenPrimary)
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}
