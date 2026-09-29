import SwiftUI
import AppKit

public struct ContextInspectorView: View {
    @ObservedObject var activityLogger = ActivityLogger.shared
    @ObservedObject var editHistory = EditHistoryManager.shared
    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var tabManager = TabManager.shared
    @State private var selectedInspectorTab: Int = 0 // 0 = Activity Log, 1 = Undo History, 2 = Info
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        VStack(spacing: 0) {
            // Tab Selector
            Picker("", selection: $selectedInspectorTab) {
                Text("Activity (\(activityLogger.entries.count))").tag(0)
                Text("Edit History (\(editHistory.history.count))").tag(1)
                Text("Database Info").tag(2)
            }
            .pickerStyle(.segmented)
            .padding(8)

            Divider().background(ThemeTokens.borderColor(for: scheme))

            // Subview
            if selectedInspectorTab == 0 {
                activityLogView
            } else if selectedInspectorTab == 1 {
                undoHistoryView
            } else {
                databaseInfoView
            }
        }
        .frame(minWidth: 260, idealWidth: 300, maxWidth: 360)
        .background(ThemeTokens.bgSidebar(for: scheme))
        .overlay(
            Rectangle()
                .frame(width: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .leading
        )
    }

    private var activityLogView: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Live Operations Stream")
                    .font(ThemeTokens.uiFont(size: 11, weight: .bold))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                Spacer()
                Button("Clear") {
                    activityLogger.clear()
                }
                .font(ThemeTokens.uiFont(size: 10))
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)

            ScrollView {
                LazyVStack(spacing: 6) {
                    ForEach(activityLogger.entries) { entry in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Circle()
                                    .fill(entry.isSuccess ? ThemeTokens.accentEmerald : ThemeTokens.accentCrimson)
                                    .frame(width: 6, height: 6)

                                Text(String(format: "%.1f ms", entry.durationMs))
                                    .font(ThemeTokens.codeFont(size: 9.5))
                                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))

                                Spacer()

                                Text(entry.timestamp, style: .time)
                                    .font(ThemeTokens.codeFont(size: 9.5))
                                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                            }

                            Text(entry.statement)
                                .font(ThemeTokens.codeFont(size: 10))
                                .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                                .lineLimit(3)

                            if let friendly = entry.userFriendlyMessage {
                                Text(friendly)
                                    .font(ThemeTokens.uiFont(size: 10))
                                    .foregroundColor(ThemeTokens.accentCrimson)
                                    .padding(4)
                                    .background(ThemeTokens.accentCrimson.opacity(0.1))
                                    .cornerRadius(3)
                            }
                        }
                        .padding(8)
                        .background(ThemeTokens.bgElevated(for: scheme))
                        .cornerRadius(6)
                        .overlay(
                            RoundedRectangle(cornerRadius: 6)
                                .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.5)
                        )
                    }
                }
                .padding(.horizontal, 8)
            }
        }
    }

    private var undoHistoryView: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Local Database Edits")
                    .font(ThemeTokens.uiFont(size: 11, weight: .bold))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                Spacer()

                if editHistory.canUndo {
                    Button("Undo (⌘Z)") {
                        Task { await executeUndo() }
                    }
                    .font(ThemeTokens.uiFont(size: 10, weight: .semibold))
                    .foregroundColor(ThemeTokens.accentBlue)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)

            if editHistory.history.isEmpty {
                VStack(spacing: 8) {
                    Text("No edits recorded yet.")
                        .font(ThemeTokens.uiFont(size: 11))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    Text("Edits made in data tables will be tracked here with safe revert capabilities.")
                        .font(ThemeTokens.uiFont(size: 10.5))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                        .multilineTextAlignment(.center)
                }
                .padding(20)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 6) {
                        ForEach(editHistory.history) { item in
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    Text(item.kind.rawValue.uppercased())
                                        .font(ThemeTokens.uiFont(size: 9, weight: .bold))
                                        .foregroundColor(ThemeTokens.accentBlue)

                                    if item.isUndone {
                                        Text("UNDONE")
                                            .font(ThemeTokens.uiFont(size: 8.5, weight: .bold))
                                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                            .padding(.horizontal, 4)
                                            .background(ThemeTokens.bgSecondary(for: scheme))
                                            .cornerRadius(2)
                                    }

                                    Spacer()

                                    Text(item.timestamp, style: .time)
                                        .font(ThemeTokens.codeFont(size: 9))
                                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                }

                                Text(item.summaryText)
                                    .font(ThemeTokens.codeBoldFont(size: 10.5))
                                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                                Text(item.reversibilityDescription)
                                    .font(ThemeTokens.uiFont(size: 9.5))
                                    .foregroundColor(item.canBeRevertedInDatabase ? ThemeTokens.accentEmerald : ThemeTokens.accentAmber)

                                if item.canBeRevertedInDatabase && !item.isUndone {
                                    Button("Revert This Operation") {
                                        Task { await revertSpecificItem(item) }
                                    }
                                    .font(ThemeTokens.uiFont(size: 10, weight: .medium))
                                    .buttonStyle(.bordered)
                                }
                            }
                            .padding(8)
                            .background(ThemeTokens.bgElevated(for: scheme))
                            .cornerRadius(6)
                            .overlay(
                                RoundedRectangle(cornerRadius: 6)
                                    .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.5)
                            )
                        }
                    }
                    .padding(.horizontal, 8)
                }
            }
        }
    }

    private var databaseInfoView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if let conn = connectionManager.activeConnection {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Active Connection")
                            .font(ThemeTokens.uiFont(size: 10, weight: .bold))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                        Text(conn.name)
                            .font(ThemeTokens.uiFont(size: 13, weight: .bold))
                        Text(conn.displayURI)
                            .font(ThemeTokens.codeFont(size: 11))
                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    }

                    Divider().background(ThemeTokens.borderColor(for: scheme))

                    VStack(alignment: .leading, spacing: 6) {
                        Text("Environment Safety")
                            .font(ThemeTokens.uiFont(size: 10, weight: .bold))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))

                        EnvironmentBadgeView(environment: conn.environment)

                        if conn.environment == .production {
                            Text("Protected Mode Enabled: Destructive SQL (DROP, TRUNCATE, DELETE) requires explicit keyword verification.")
                                .font(ThemeTokens.uiFont(size: 10.5))
                                .foregroundColor(ThemeTokens.accentCrimson)
                        } else {
                            Text("Development environment. Auto-save is active.")
                                .font(ThemeTokens.uiFont(size: 10.5))
                                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                        }
                    }

                    Divider().background(ThemeTokens.borderColor(for: scheme))

                    if let driver = connectionManager.activeDriver {
                        let meta = driver.metadata
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Database Metadata")
                                .font(ThemeTokens.uiFont(size: 10, weight: .bold))
                                .foregroundColor(ThemeTokens.textMuted(for: scheme))

                            HStack {
                                Text("Tables:")
                                    .font(ThemeTokens.uiFont(size: 11))
                                Spacer()
                                Text("\(meta.tables.count)")
                                    .font(ThemeTokens.codeFont(size: 11))
                            }

                            HStack {
                                Text("Views:")
                                    .font(ThemeTokens.uiFont(size: 11))
                                Spacer()
                                Text("\(meta.views.count)")
                                    .font(ThemeTokens.codeFont(size: 11))
                            }

                            HStack {
                                Text("Indexes:")
                                    .font(ThemeTokens.uiFont(size: 11))
                                Spacer()
                                Text("\(meta.indexes.count)")
                                    .font(ThemeTokens.codeFont(size: 11))
                            }

                            HStack {
                                Text("Routines:")
                                    .font(ThemeTokens.uiFont(size: 11))
                                Spacer()
                                Text("\(meta.routines.count)")
                                    .font(ThemeTokens.codeFont(size: 11))
                            }
                        }
                    }
                }
            }
            .padding(12)
        }
    }

    private func executeUndo() async {
        guard let entry = editHistory.popUndoEntry() else { return }
        if let reverseOp = entry.reverseOperation, let driver = connectionManager.activeDriver {
            _ = try? await driver.executeQuery(sql: reverseOp.statement, database: connectionManager.activeDatabase)
            ToastManager.shared.show("Reverted Edit", subtitle: entry.summaryText, style: .info)
        }
    }

    private func revertSpecificItem(_ item: EditHistoryEntry) async {
        guard let reverseOp = item.reverseOperation, let driver = connectionManager.activeDriver else { return }
        _ = try? await driver.executeQuery(sql: reverseOp.statement, database: connectionManager.activeDatabase)
        ToastManager.shared.show("Operation Reverted", subtitle: item.summaryText, style: .info)
    }
}
