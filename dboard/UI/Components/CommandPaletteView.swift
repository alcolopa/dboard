import SwiftUI

public struct PaletteCommand: Identifiable {
    public let id: String
    public var title: String
    public var category: String
    public var shortcut: String?
    public var iconName: String
    public var action: () -> Void

    public init(
        id: String,
        title: String,
        category: String,
        shortcut: String? = nil,
        iconName: String,
        action: @escaping () -> Void
    ) {
        self.id = id
        self.title = title
        self.category = category
        self.shortcut = shortcut
        self.iconName = iconName
        self.action = action
    }
}

public struct CommandPaletteView: View {
    @Binding var isPresented: Bool
    @State private var searchText: String = ""
    @State private var selectedIndex: Int = 0
    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var tabManager = TabManager.shared
    @ObservedObject var appSettings = AppSettings.shared
    @Environment(\.colorScheme) var scheme

    private var allCommands: [PaletteCommand] {
        var list: [PaletteCommand] = [
            PaletteCommand(id: "new_query", title: "New SQL Query", category: "Query", shortcut: "⌘N", iconName: "bolt.fill") {
                tabManager.openQueryTab()
            },
            PaletteCommand(id: "exec_query", title: "Execute Query / Selection", category: "Query", shortcut: "⌘↵", iconName: "play.fill") {
                // Trigger query execution
            },
            PaletteCommand(id: "reopen_tab", title: "Reopen Closed Tab", category: "Tabs", shortcut: "⇧⌘T", iconName: "arrow.uturn.backward") {
                tabManager.reopenLastClosedTab()
            },
            PaletteCommand(id: "close_tab", title: "Close Active Tab", category: "Tabs", shortcut: "⌘W", iconName: "xmark") {
                if let id = tabManager.activeTabId { tabManager.closeTab(id: id) }
            },
            PaletteCommand(id: "toggle_theme", title: "Toggle Dark / Light Theme", category: "Appearance", shortcut: "⌘T", iconName: "circle.lefthalf.filled") {
                appSettings.theme = (appSettings.theme == .dark) ? .light : .dark
            },
            PaletteCommand(id: "open_settings", title: "Open Settings", category: "Preferences", shortcut: "⌘,", iconName: "gearshape") {
                tabManager.openSettingsTab()
            }
        ]

        // Add Tables from metadata
        if let driver = connectionManager.activeDriver {
            for table in driver.metadata.tables {
                list.append(PaletteCommand(
                    id: "open_table_\(table.name)",
                    title: "Open Table: \(table.name)",
                    category: "Tables",
                    shortcut: nil,
                    iconName: table.type.iconName
                ) {
                    if table.type == .collection {
                        tabManager.openMongoTab(collection: table.name)
                    } else {
                        tabManager.openTableDataTab(schema: table.schemaName, table: table.name)
                    }
                })

                list.append(PaletteCommand(
                    id: "view_structure_\(table.name)",
                    title: "Inspect Structure: \(table.name)",
                    category: "Schema",
                    shortcut: nil,
                    iconName: "wrench.and.screwdriver"
                ) {
                    tabManager.openTableStructureTab(schema: table.schemaName, table: table.name)
                })
            }

            for routine in driver.metadata.routines {
                list.append(PaletteCommand(
                    id: "routine_\(routine.id)",
                    title: "\(routine.isProcedure ? "Inspect Procedure" : "Inspect Function"): \(routine.name)",
                    category: routine.isProcedure ? "Procedures" : "Functions",
                    shortcut: nil,
                    iconName: routine.isProcedure ? "gearshape.2.fill" : "function"
                ) {
                    tabManager.openRoutineTab(schema: routine.schemaName, name: routine.name, isProcedure: routine.isProcedure)
                })
            }
        }

        // Add Saved Connections
        for conn in connectionManager.savedConnections {
            list.append(PaletteCommand(
                id: "connect_\(conn.id.uuidString)",
                title: "Switch Connection: \(conn.name) (\(conn.environment.rawValue))",
                category: "Connections",
                shortcut: nil,
                iconName: conn.type.iconName
            ) {
                Task { await connectionManager.connect(to: conn) }
            })
        }

        return list
    }

    private var filteredCommands: [PaletteCommand] {
        if searchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return allCommands
        }
        let lower = searchText.lowercased()
        return allCommands.filter {
            $0.title.lowercased().contains(lower) ||
            $0.category.lowercased().contains(lower)
        }
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Search Input Header
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass")
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    .font(.system(size: 18))

                TextField("Type a command, table name, or search action... (⌘K)", text: $searchText)
                    .textFieldStyle(.plain)
                    .font(ThemeTokens.uiFont(size: 14))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                if !searchText.isEmpty {
                    Button(action: { searchText = "" }) {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .buttonStyle(.hit)
                }

                Text("ESC to dismiss")
                    .font(ThemeTokens.uiFont(size: 10.5))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(ThemeTokens.bgSecondary(for: scheme))
                    .cornerRadius(4)
            }
            .padding(14)
            .background(ThemeTokens.bgElevated(for: scheme))

            Divider().background(ThemeTokens.borderColor(for: scheme))

            // Command List
            ScrollView {
                LazyVStack(spacing: 2) {
                    ForEach(Array(filteredCommands.prefix(15).enumerated()), id: \.element.id) { index, cmd in
                        let isSelected = index == selectedIndex
                        HStack(spacing: 12) {
                            Image(systemName: cmd.iconName)
                                .foregroundColor(isSelected ? ThemeTokens.accentBlue : ThemeTokens.textSecondary(for: scheme))
                                .frame(width: 22)
                                .font(.system(size: 12))

                            VStack(alignment: .leading, spacing: 1) {
                                Text(cmd.title)
                                    .font(ThemeTokens.uiFont(size: 12.5, weight: isSelected ? .medium : .regular))
                                    .foregroundColor(isSelected ? ThemeTokens.textPrimary(for: scheme) : ThemeTokens.textSecondary(for: scheme))
                            }

                            Spacer()

                            Text(cmd.category.uppercased())
                                .font(ThemeTokens.uiFont(size: 9.5, weight: .semibold))
                                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                .padding(.horizontal, 6)
                                .padding(.vertical, 2)
                                .background(ThemeTokens.bgSecondary(for: scheme))
                                .cornerRadius(3)

                            if let shortcut = cmd.shortcut {
                                Text(shortcut)
                                    .font(ThemeTokens.codeFont(size: 11))
                                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                                    .padding(.horizontal, 5)
                                    .padding(.vertical, 2)
                                    .background(ThemeTokens.bgPrimary(for: scheme))
                                    .cornerRadius(4)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.5)
                                    )
                            }
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(
                            RoundedRectangle(cornerRadius: 6)
                                .fill(isSelected ? ThemeTokens.accentBlue.opacity(0.12) : Color.clear)
                        )
                        .contentShape(Rectangle())
                        .onHover { hovering in
                            if hovering { selectedIndex = index }
                        }
                        .onTapGesture {
                            cmd.action()
                            isPresented = false
                        }
                    }
                }
                .padding(8)
            }
            .frame(maxHeight: 340)
        }
        .frame(width: 580)
        .background(ThemeTokens.bgElevated(for: scheme))
        .cornerRadius(10)
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 1)
        )
        .shadow(color: Color.black.opacity(0.35), radius: 24, x: 0, y: 12)
    }
}
