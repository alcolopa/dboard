import SwiftUI

public struct ContentView: View {
    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var tabManager = TabManager.shared
    @ObservedObject var appSettings = AppSettings.shared
    @ObservedObject var editHistory = EditHistoryManager.shared

    @State private var isCommandPaletteOpen: Bool = false
    @State private var isGlobalSearchOpen: Bool = false
    @State private var isConnectionManagerOpen: Bool = (ConnectionManager.shared.activeDriver == nil)
    @State private var isInspectorOpen: Bool = true
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        ZStack {
            VStack(spacing: 0) {
                // Top Unified Toolbar
                AppToolbarView(
                    isCommandPaletteOpen: $isCommandPaletteOpen,
                    isGlobalSearchOpen: $isGlobalSearchOpen,
                    isConnectionManagerOpen: $isConnectionManagerOpen,
                    isInspectorOpen: $isInspectorOpen
                )

                // 3-Pane Developer Layout
                HSplitView {
                    // Left Pane: Database Tree Sidebar
                    SidebarView(isConnectionManagerOpen: $isConnectionManagerOpen)

                    // Center Pane: Multi-tab Workspace
                    VStack(spacing: 0) {
                        WorkspaceTabBarView()

                        if let activeTab = tabManager.activeTab {
                            tabContent(for: activeTab)
                        } else {
                            VStack(spacing: 12) {
                                Image(systemName: "square.grid.2x2")
                                    .font(.system(size: 32))
                                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                Text("No open tabs")
                                    .font(ThemeTokens.uiFont(size: 13, weight: .medium))
                                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                                Button("Open New Query (⌘N)") {
                                    tabManager.openQueryTab()
                                }
                                .buttonStyle(.borderedProminent)
                            }
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            .background(ThemeTokens.bgPrimary(for: scheme))
                        }
                    }
                    .frame(minWidth: 400)

                    // Right Pane: Context Inspector (Activity, History, Info)
                    if isInspectorOpen {
                        ContextInspectorView()
                    }
                }
            }

            // Command Palette Modal (⌘K)
            if isCommandPaletteOpen {
                Color.black.opacity(0.4)
                    .edgesIgnoringSafeArea(.all)
                    .onTapGesture { isCommandPaletteOpen = false }

                CommandPaletteView(isPresented: $isCommandPaletteOpen)
                    .transition(.scale(scale: 0.95).combined(with: .opacity))
            }

            // Global Search Modal (⌘P)
            if isGlobalSearchOpen {
                Color.black.opacity(0.4)
                    .edgesIgnoringSafeArea(.all)
                    .onTapGesture { isGlobalSearchOpen = false }

                GlobalSearchView(isPresented: $isGlobalSearchOpen)
                    .transition(.scale(scale: 0.95).combined(with: .opacity))
            }

            // Toast Floating Notification HUD
            ToastContainerView()
        }
        .preferredColorScheme(appSettings.theme.colorScheme)
        .frame(minWidth: 960, maxWidth: .infinity, minHeight: 620, maxHeight: .infinity)
        .sheet(isPresented: $isConnectionManagerOpen) {
            ConnectionManagerModal(isPresented: $isConnectionManagerOpen)
        }
        .onAppear {
            if connectionManager.activeDriver == nil {
                Task {
                    await connectionManager.autoConnectIfPossible()
                    if connectionManager.connectionStatus == .connected {
                        isConnectionManagerOpen = false
                    }
                }
            }
        }
        // Native Keyboard Shortcuts
        .background(
            // Hidden buttons to capture key equivalents
            Group {
                Button("") { isCommandPaletteOpen.toggle() }
                    .keyboardShortcut("k", modifiers: [.command])
                Button("") { isGlobalSearchOpen.toggle() }
                    .keyboardShortcut("p", modifiers: [.command])
                Button("") { tabManager.openQueryTab() }
                    .keyboardShortcut("n", modifiers: [.command])
                Button("") {
                    if let id = tabManager.activeTabId { tabManager.closeTab(id: id) }
                }
                .keyboardShortcut("w", modifiers: [.command])
                Button("") { tabManager.reopenLastClosedTab() }
                    .keyboardShortcut("t", modifiers: [.command, .shift])
                Button("") {
                    if let driver = connectionManager.activeDriver {
                        Task { _ = try? await driver.refreshMetadata(database: connectionManager.activeDatabase) }
                        ToastManager.shared.show("Refreshed", style: .info)
                    }
                }
                .keyboardShortcut("r", modifiers: [.command])
                Button("") {
                    Task {
                        if let entry = editHistory.popUndoEntry(), let reverseOp = entry.reverseOperation, let driver = connectionManager.activeDriver {
                            _ = try? await driver.executeQuery(sql: reverseOp.statement, database: connectionManager.activeDatabase)
                            ToastManager.shared.show("Reverted Edit", subtitle: entry.summaryText, style: .info)
                        }
                    }
                }
                .keyboardShortcut("z", modifiers: [.command])
                Button("") {
                    withAnimation { isInspectorOpen.toggle() }
                }
                .keyboardShortcut("i", modifiers: [.command, .option])
            }
            .opacity(0)
            .allowsHitTesting(false)
        )
    }

    @ViewBuilder
    private func tabContent(for tab: WorkspaceTab) -> some View {
        switch tab.kind {
        case .tableData(let schema, let table):
            TableDataBrowserView(schema: schema, tableName: table)
                .id(tab.id)

        case .tableStructure(let schema, let table):
            SchemaStructureView(schema: schema, tableName: table)
                .id(tab.id)

        case .routine(let schema, let name):
            RoutineDetailView(schema: schema, routineName: name)
                .id(tab.id)

        case .sqlQuery(let queryId):
            SQLQueryEditorView(
                tabId: queryId,
                queryText: Binding(
                    get: { tab.queryText },
                    set: { tabManager.updateActiveTabQueryText($0) }
                )
            )
            .id(tab.id)

        case .mongoDocuments(let collection):
            MongoWorkspaceView(collectionName: collection)
                .id(tab.id)

        case .mongoAggregation(let collection):
            MongoAggregationView(collectionName: collection)
                .id(tab.id)

        case .settings:
            SettingsView()
                .id(tab.id)
        }
    }
}
