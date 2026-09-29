import SwiftUI

public struct SQLQueryEditorView: View {
    public let tabId: UUID
    @Binding var queryText: String

    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var historyManager = QueryHistoryManager.shared
    @ObservedObject var appSettings = AppSettings.shared

    @State private var queryResult: QueryResult? = nil
    @State private var isExecuting: Bool = false
    @State private var showHistoryDrawer: Bool = false
    @State private var activeResultTab: Int = 0 // 0 = Grid, 1 = Explain, 2 = DDL/Messages
    @State private var autocompleteQuery: String = ""
    @State private var showAutocomplete: Bool = false
    @Environment(\.colorScheme) var scheme

    private var autocompleteSuggestions: [String] {
        guard let driver = connectionManager.activeDriver else { return [] }
        var list: [String] = [
            "SELECT", "FROM", "WHERE", "JOIN", "LEFT JOIN", "RIGHT JOIN", "INNER JOIN",
            "GROUP BY", "ORDER BY", "LIMIT", "OFFSET", "INSERT INTO", "UPDATE", "DELETE FROM",
            "COUNT(*)", "AVG()", "SUM()", "DISTINCT", "AND", "OR", "NOT", "IN", "IS NULL", "AS"
        ]
        // Add tables and columns
        for t in driver.metadata.tables {
            list.append(t.name)
            for c in t.columns {
                list.append(c.name)
            }
        }
        if autocompleteQuery.isEmpty { return Array(list.prefix(12)) }
        let lower = autocompleteQuery.lowercased()
        return list.filter { $0.lowercased().contains(lower) }
    }

    public var body: some View {
        HStack(spacing: 0) {
            VStack(spacing: 0) {
                // Editor Action Toolbar
                editorToolbar

                // Split: Top Editor, Bottom Results
                VResizableSplit(initialHeight: 220, minTop: 120, minBottom: 140) {
                    // SQL Text Editor with Line Numbers & Autocomplete
                    VStack(alignment: .leading, spacing: 0) {
                        HStack(alignment: .top, spacing: 0) {
                            // Line Number Gutter
                            lineNumbersGutter

                            // Main Editor Area
                            TextEditor(text: $queryText)
                                .font(ThemeTokens.codeFont(size: CGFloat(appSettings.editorFontSize)))
                                .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                                .padding(8)
                                .background(ThemeTokens.bgPrimary(for: scheme))
                        }
                    }
                } bottom: {
                    // Results Panel
                    resultsPanel
                }
            }

            // Right side Query History & Saved Drawer
            if showHistoryDrawer {
                QueryHistoryFavoritesView { selectedSQL in
                    queryText = selectedSQL
                }
                .transition(.move(edge: .trailing))
            }
        }
        .background(ThemeTokens.bgPrimary(for: scheme))
    }

    private var editorToolbar: some View {
        HStack(spacing: 8) {
            // Execute Button
            Button(action: {
                Task { await executeCurrentSQL() }
            }) {
                HStack(spacing: 5) {
                    Image(systemName: isExecuting ? "rays" : "play.fill")
                        .font(.system(size: 12))
                    Text("Execute")
                        .font(ThemeTokens.uiFont(size: 11.5, weight: .bold))
                    Text("⌘↵")
                        .font(ThemeTokens.codeFont(size: 10))
                        .opacity(0.8)
                }
                .padding(.horizontal, 9)
                .padding(.vertical, 4)
                .background(ThemeTokens.accentEmerald)
                .foregroundColor(.white)
                .cornerRadius(5)
            }
            .buttonStyle(.hit)
            .disabled(isExecuting)

            // Explain Button
            Button(action: {
                Task { await explainCurrentSQL() }
            }) {
                HStack(spacing: 4) {
                    Image(systemName: "chart.bar.doc.horizontal")
                        .font(.system(size: 12))
                    Text("Explain")
                        .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(ThemeTokens.bgSecondary(for: scheme))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .cornerRadius(5)
            }
            .buttonStyle(.hit)

            // Format Button
            Button(action: formatSQL) {
                HStack(spacing: 4) {
                    Image(systemName: "text.alignleft")
                        .font(.system(size: 12))
                    Text("Format")
                        .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                }
                .padding(.horizontal, 7)
                .padding(.vertical, 4)
                .background(ThemeTokens.bgSecondary(for: scheme))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .cornerRadius(5)
            }
            .buttonStyle(.hit)

            // Clear Button
            Button(action: { queryText = "" }) {
                Image(systemName: "trash")
                    .font(.system(size: 12))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    .frame(width: 32, height: 28)
                    .background(ThemeTokens.bgSecondary(for: scheme))
                    .cornerRadius(4)
            }
            .buttonStyle(.hit)

            Spacer()

            // Toggle History Drawer
            Button(action: {
                withAnimation { showHistoryDrawer.toggle() }
            }) {
                HStack(spacing: 4) {
                    Image(systemName: "clock.arrow.circlepath")
                        .font(.system(size: 12))
                    Text("History")
                        .font(ThemeTokens.uiFont(size: 11))
                }
                .padding(.horizontal, 7)
                .padding(.vertical, 4)
                .background(showHistoryDrawer ? ThemeTokens.accentBlue.opacity(0.15) : ThemeTokens.bgSecondary(for: scheme))
                .foregroundColor(showHistoryDrawer ? ThemeTokens.accentBlue : ThemeTokens.textSecondary(for: scheme))
                .cornerRadius(5)
            }
            .buttonStyle(.hit)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(ThemeTokens.bgElevated(for: scheme))
        .overlay(
            Rectangle()
                .frame(height: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .bottom
        )
    }

    private var lineNumbersGutter: some View {
        let totalLines = max(1, queryText.components(separatedBy: "\n").count)
        let displayLines = min(totalLines, 200)
        return VStack(alignment: .trailing, spacing: 2) {
            ForEach(1...displayLines, id: \.self) { lineNum in
                Text("\(lineNum)")
                    .font(ThemeTokens.codeFont(size: 11))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme).opacity(0.7))
                    .frame(width: 28, alignment: .trailing)
            }
            Spacer()
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 4)
        .background(ThemeTokens.tableHeaderBg(for: scheme).opacity(0.5))
        .overlay(
            Rectangle()
                .frame(width: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .trailing
        )
    }

    private var resultsPanel: some View {
        VStack(spacing: 0) {
            // Result View Tabs
            HStack(spacing: 8) {
                Button(action: { activeResultTab = 0 }) {
                    Text("Data Grid")
                        .font(ThemeTokens.uiFont(size: 11, weight: activeResultTab == 0 ? .bold : .regular))
                        .foregroundColor(activeResultTab == 0 ? ThemeTokens.accentBlue : ThemeTokens.textSecondary(for: scheme))
                }
                .buttonStyle(.hit)

                if queryResult?.explainPlan != nil {
                    Button(action: { activeResultTab = 1 }) {
                        Text("Explain Plan")
                            .font(ThemeTokens.uiFont(size: 11, weight: activeResultTab == 1 ? .bold : .regular))
                            .foregroundColor(activeResultTab == 1 ? ThemeTokens.accentBlue : ThemeTokens.textSecondary(for: scheme))
                    }
                    .buttonStyle(.hit)
                }

                Spacer()
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(ThemeTokens.bgSecondary(for: scheme))
            .overlay(
                Rectangle()
                    .frame(height: 1)
                    .foregroundColor(ThemeTokens.borderColor(for: scheme)),
                alignment: .bottom
            )

            if let res = queryResult {
                if activeResultTab == 1, let plan = res.explainPlan {
                    ExplainPlanView(rootPlan: plan)
                } else {
                    QueryResultsGridView(result: res)
                }
            } else {
                VStack(spacing: 8) {
                    Image(systemName: "bolt.badge.clock")
                        .font(.system(size: 28))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    Text("Press ⌘↵ or Click Execute to run your query")
                        .font(ThemeTokens.uiFont(size: 12))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(ThemeTokens.bgPrimary(for: scheme))
            }
        }
    }

    private func executeCurrentSQL() async {
        guard let driver = connectionManager.activeDriver else { return }
        let sql = queryText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !sql.isEmpty else { return }

        isExecuting = true
        do {
            let res = try await driver.executeQuery(sql: sql, database: connectionManager.activeDatabase)
            self.queryResult = res
            self.isExecuting = false
            self.activeResultTab = res.explainPlan != nil ? 1 : 0

            historyManager.recordExecution(
                query: sql,
                database: connectionManager.activeDatabase,
                connectionName: connectionManager.activeConnection?.name ?? "Default",
                durationMs: res.executionDurationMs,
                isSuccess: true,
                affectedRows: res.rows.count
            )

            ToastManager.shared.show("Query Executed", subtitle: "\(res.rows.count) rows in \(String(format: "%.1f", res.executionDurationMs))ms", style: .success)
        } catch {
            isExecuting = false
            self.queryResult = QueryResult(errorMessage: error.localizedDescription)
            historyManager.recordExecution(
                query: sql,
                database: connectionManager.activeDatabase,
                connectionName: connectionManager.activeConnection?.name ?? "Default",
                durationMs: 0.0,
                isSuccess: false,
                errorMessage: error.localizedDescription
            )
            ToastManager.shared.show("Query Error", subtitle: error.localizedDescription, style: .error)
        }
    }

    private func explainCurrentSQL() async {
        let sql = "EXPLAIN ANALYZE " + queryText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let driver = connectionManager.activeDriver else { return }
        isExecuting = true
        do {
            let res = try await driver.executeQuery(sql: sql, database: connectionManager.activeDatabase)
            self.queryResult = res
            self.isExecuting = false
            self.activeResultTab = 1
        } catch {
            isExecuting = false
            ToastManager.shared.show("Explain Failed", subtitle: error.localizedDescription, style: .error)
        }
    }

    private func formatSQL() {
        var formatted = queryText
        let keywords = [
            "SELECT", "FROM", "WHERE", "GROUP BY", "ORDER BY", "HAVING", "LIMIT",
            "OFFSET", "LEFT JOIN", "RIGHT JOIN", "INNER JOIN", "JOIN", "ON",
            "INSERT INTO", "VALUES", "UPDATE", "SET", "DELETE FROM"
        ]
        for kw in keywords {
            formatted = formatted.replacingOccurrences(of: "\\b\(kw)\\b", with: "\n\(kw)", options: [.regularExpression, .caseInsensitive])
        }
        queryText = formatted.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
