import SwiftUI

@main
struct MyApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .frame(minWidth: 960, idealWidth: 1280, maxWidth: .infinity, minHeight: 620, idealHeight: 800, maxHeight: .infinity)
        }
        .windowStyle(.titleBar)
        .windowToolbarStyle(.unified(showsTitle: false))
        .commands {
            SidebarCommands()

            CommandGroup(replacing: .newItem) {
                Button("New SQL Query") {
                    TabManager.shared.openQueryTab()
                }
                .keyboardShortcut("n", modifiers: [.command])

                Button("Command Palette...") {
                    // Handled via keyboard shortcut
                }
                .keyboardShortcut("k", modifiers: [.command])

                Button("Global Object Search...") {
                    // Handled via keyboard shortcut
                }
                .keyboardShortcut("p", modifiers: [.command])
            }

            CommandMenu("Database") {
                Button("Refresh Metadata") {
                    if let driver = ConnectionManager.shared.activeDriver {
                        Task { _ = try? await driver.refreshMetadata(database: ConnectionManager.shared.activeDatabase) }
                        ToastManager.shared.show("Refreshed", style: .info)
                    }
                }
                .keyboardShortcut("r", modifiers: [.command])

                Button("Undo Database Edit") {
                    Task {
                        if let entry = EditHistoryManager.shared.popUndoEntry(), let reverseOp = entry.reverseOperation, let driver = ConnectionManager.shared.activeDriver {
                            _ = try? await driver.executeQuery(sql: reverseOp.statement, database: ConnectionManager.shared.activeDatabase)
                            ToastManager.shared.show("Reverted Edit", subtitle: entry.summaryText, style: .info)
                        }
                    }
                }
                .keyboardShortcut("z", modifiers: [.command])
            }
        }
    }
}
