import SwiftUI

public struct AppToolbarView: View {
    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var tabManager = TabManager.shared
    @ObservedObject var appSettings = AppSettings.shared
    @Binding var isCommandPaletteOpen: Bool
    @Binding var isGlobalSearchOpen: Bool
    @Binding var isConnectionManagerOpen: Bool
    @Binding var isInspectorOpen: Bool
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        HStack(spacing: 12) {
            // Connection Selector Dropdown
            Menu {
                ForEach(connectionManager.savedConnections) { conn in
                    Button(action: {
                        Task { await connectionManager.connect(to: conn) }
                    }) {
                        HStack {
                            Text(conn.name)
                            if conn.id == connectionManager.activeConnection?.id {
                                Image(systemName: "checkmark")
                            }
                        }
                    }
                }
                Divider()
                Button(action: {
                    isConnectionManagerOpen = true
                }) {
                    Label("Manage Connections...", systemImage: "slider.horizontal.3")
                }
            } label: {
                HStack(spacing: 6) {
                    if let active = connectionManager.activeConnection {
                        Circle()
                            .fill(Color(hex: active.colorTag ?? active.environment.badgeColorHex))
                            .frame(width: 8, height: 8)
                        Text(active.name)
                            .font(ThemeTokens.uiFont(size: 12, weight: .semibold))
                            .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                    } else {
                        Circle()
                            .fill(ThemeTokens.textMuted(for: scheme))
                            .frame(width: 8, height: 8)
                        Text("Select Connection")
                            .font(ThemeTokens.uiFont(size: 12))
                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    }
                    Image(systemName: "chevron.down")
                        .font(.system(size: 9))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(ThemeTokens.bgSecondary(for: scheme))
                .cornerRadius(6)
            }
            .menuStyle(.borderlessButton)

            // Environment Badge
            if let active = connectionManager.activeConnection {
                EnvironmentBadgeView(environment: active.environment)
            }

            // Database Selector
            if let driver = connectionManager.activeDriver {
                Menu {
                    ForEach(connectionManager.availableDatabases.isEmpty ? [connectionManager.activeDatabase] : connectionManager.availableDatabases, id: \.self) { dbName in
                        Button(action: {
                            Task { await connectionManager.switchDatabase(to: dbName) }
                        }) {
                            Text(dbName)
                        }
                    }
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "cylinder")
                            .font(.system(size: 10))
                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                        Text(connectionManager.activeDatabase)
                            .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                            .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                        Image(systemName: "chevron.down")
                            .font(.system(size: 8))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .padding(.horizontal, 6)
                    .padding(.vertical, 3)
                    .background(ThemeTokens.bgSecondary(for: scheme))
                    .cornerRadius(5)
                }
                .menuStyle(.borderlessButton)

                // Schema Selector (for PostgreSQL)
                if connectionManager.activeConnection?.type == .postgresql {
                    Menu {
                        ForEach(driver.metadata.schemas, id: \.self) { schema in
                            Button(action: {
                                connectionManager.switchSchema(to: schema)
                            }) {
                                Text(schema)
                            }
                        }
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "square.stack.3d.up")
                                .font(.system(size: 10))
                                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                            Text(connectionManager.activeSchema)
                                .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                                .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                            Image(systemName: "chevron.down")
                                .font(.system(size: 8))
                                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                        }
                        .padding(.horizontal, 6)
                        .padding(.vertical, 3)
                        .background(ThemeTokens.bgSecondary(for: scheme))
                        .cornerRadius(5)
                    }
                    .menuStyle(.borderlessButton)
                }
            }

            Spacer()

            // Quick Actions Toolbar
            HStack(spacing: 4) {
                // New Query Button
                Button(action: {
                    tabManager.openQueryTab()
                }) {
                    HStack(spacing: 4) {
                        Image(systemName: "bolt.fill")
                            .font(.system(size: 11))
                        Text("New Query")
                            .font(ThemeTokens.uiFont(size: 11.5, weight: .medium))
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(ThemeTokens.accentBlue.opacity(0.12))
                    .foregroundColor(ThemeTokens.accentBlue)
                    .cornerRadius(5)
                }
                .buttonStyle(.plain)
                .help("Open New SQL Query Tab (⌘N)")

                // Command Palette Shortcut
                Button(action: {
                    isCommandPaletteOpen = true
                }) {
                    HStack(spacing: 4) {
                        Image(systemName: "command")
                            .font(.system(size: 10))
                        Text("⌘K")
                            .font(ThemeTokens.codeFont(size: 11))
                    }
                    .padding(.horizontal, 7)
                    .padding(.vertical, 4)
                    .background(ThemeTokens.bgSecondary(for: scheme))
                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    .cornerRadius(5)
                }
                .buttonStyle(.plain)
                .help("Command Palette (⌘K)")

                // Global Search Button
                Button(action: {
                    isGlobalSearchOpen = true
                }) {
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 12))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                        .frame(width: 26, height: 24)
                        .background(ThemeTokens.bgSecondary(for: scheme))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)
                .help("Search Database Objects (⌘P)")

                // Refresh Button
                Button(action: {
                    if let driver = connectionManager.activeDriver {
                        Task { _ = try? await driver.refreshMetadata(database: connectionManager.activeDatabase) }
                        ToastManager.shared.show("Metadata Refreshed", style: .info)
                    }
                }) {
                    Image(systemName: "arrow.clockwise")
                        .font(.system(size: 11))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                        .frame(width: 26, height: 24)
                        .background(ThemeTokens.bgSecondary(for: scheme))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)
                .help("Refresh Database (⌘R)")

                // Theme Toggle
                Button(action: {
                    appSettings.theme = (appSettings.theme == .dark) ? .light : .dark
                }) {
                    Image(systemName: appSettings.theme == .dark ? "moon.fill" : "sun.max.fill")
                        .font(.system(size: 11))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                        .frame(width: 26, height: 24)
                        .background(ThemeTokens.bgSecondary(for: scheme))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)
                .help("Toggle Theme (Dark / Light)")

                // Inspector Toggle
                Button(action: {
                    withAnimation { isInspectorOpen.toggle() }
                }) {
                    Image(systemName: "sidebar.right")
                        .font(.system(size: 11))
                        .foregroundColor(isInspectorOpen ? ThemeTokens.accentBlue : ThemeTokens.textSecondary(for: scheme))
                        .frame(width: 26, height: 24)
                        .background(ThemeTokens.bgSecondary(for: scheme))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)
                .help("Toggle Inspector Panel (⌥⌘I)")
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 7)
        .background(ThemeTokens.bgPrimary(for: scheme))
        .overlay(
            Rectangle()
                .frame(height: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .bottom
        )
    }
}
