import SwiftUI

public enum SearchResultType: String, CaseIterable {
    case table = "Table"
    case column = "Column"
    case view = "View"
    case procedure = "Procedure"
    case function = "Function"
    case collection = "Collection"
    case savedQuery = "Saved Query"

    public var iconName: String {
        switch self {
        case .table: return "tablecells"
        case .column: return "arrow.right.to.line"
        case .view: return "eye"
        case .procedure: return "gearshape.2.fill"
        case .function: return "function"
        case .collection: return "leaf.fill"
        case .savedQuery: return "bookmark.fill"
        }
    }
}

public struct GlobalSearchResultItem: Identifiable {
    public let id: String
    public var title: String
    public var subtitle: String
    public var type: SearchResultType
    public var onSelect: () -> Void
}

public struct GlobalSearchView: View {
    @Binding var isPresented: Bool
    @State private var query: String = ""
    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var tabManager = TabManager.shared
    @ObservedObject var queryHistoryManager = QueryHistoryManager.shared
    @Environment(\.colorScheme) var scheme

    private var results: [GlobalSearchResultItem] {
        guard !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return [] }
        let q = query.lowercased()
        var items: [GlobalSearchResultItem] = []

        if let driver = connectionManager.activeDriver {
            // Tables and Collections
            for t in driver.metadata.tables {
                if t.name.lowercased().contains(q) {
                    let type: SearchResultType = (t.type == .collection) ? .collection : .table
                    items.append(GlobalSearchResultItem(
                        id: "tbl_\(t.id)",
                        title: t.name,
                        subtitle: "\(t.schemaName) • \(t.columns.count) columns",
                        type: type
                    ) {
                        if t.type == .collection {
                            tabManager.openMongoTab(collection: t.name)
                        } else {
                            tabManager.openTableDataTab(schema: t.schemaName, table: t.name)
                        }
                    })
                }

                // Columns within tables
                for col in t.columns {
                    if col.name.lowercased().contains(q) {
                        items.append(GlobalSearchResultItem(
                            id: "col_\(t.name)_\(col.name)",
                            title: "\(t.name).\(col.name)",
                            subtitle: "\(col.dataTypeName) \(col.isPrimaryKey ? "• PRIMARY KEY" : "")",
                            type: .column
                        ) {
                            tabManager.openTableStructureTab(schema: t.schemaName, table: t.name)
                        })
                    }
                }
            }

            // Views
            for v in driver.metadata.views {
                if v.name.lowercased().contains(q) {
                    items.append(GlobalSearchResultItem(
                        id: "view_\(v.name)",
                        title: v.name,
                        subtitle: "\(v.schemaName) • View",
                        type: .view
                    ) {
                        tabManager.openTableDataTab(schema: v.schemaName, table: v.name)
                    })
                }
            }

            // Routines (Procedures & Functions)
            for r in driver.metadata.routines {
                if r.name.lowercased().contains(q) {
                    items.append(GlobalSearchResultItem(
                        id: "\(r.isProcedure ? "proc" : "fn")_\(r.name)",
                        title: r.name,
                        subtitle: "\(r.isProcedure ? "PROCEDURE" : "FUNCTION") • \(r.arguments) -> \(r.returnType)",
                        type: r.isProcedure ? .procedure : .function
                    ) {
                        tabManager.openRoutineTab(schema: r.schemaName, name: r.name, isProcedure: r.isProcedure)
                    })
                }
            }
        }

        // Saved queries
        for folder in queryHistoryManager.folders {
            for sq in folder.queries {
                if sq.title.lowercased().contains(q) || sq.query.lowercased().contains(q) {
                    items.append(GlobalSearchResultItem(
                        id: "sq_\(sq.id.uuidString)",
                        title: sq.title,
                        subtitle: "Saved Query in \(folder.name)",
                        type: .savedQuery
                    ) {
                        tabManager.openQueryTab(initialSQL: sq.query)
                    })
                }
            }
        }

        return items
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass")
                    .foregroundColor(ThemeTokens.accentBlue)
                    .font(.system(size: 19))

                TextField("Search tables, columns, views, functions, collections... (⌘P)", text: $query)
                    .textFieldStyle(.plain)
                    .font(ThemeTokens.uiFont(size: 14))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                if !query.isEmpty {
                    Button(action: { query = "" }) {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(14)
            .background(ThemeTokens.bgElevated(for: scheme))

            Divider().background(ThemeTokens.borderColor(for: scheme))

            if results.isEmpty {
                VStack(spacing: 8) {
                    Text(query.isEmpty ? "Search across all objects in \(connectionManager.activeDatabase)" : "No matching database objects found")
                        .font(ThemeTokens.uiFont(size: 12))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 32)
            } else {
                ScrollView {
                    LazyVStack(spacing: 2) {
                        ForEach(results) { item in
                            HStack(spacing: 12) {
                                Image(systemName: item.type.iconName)
                                    .foregroundColor(ThemeTokens.accentBlue)
                                    .frame(width: 22)
                                    .font(.system(size: 12))

                                VStack(alignment: .leading, spacing: 2) {
                                    Text(item.title)
                                        .font(ThemeTokens.uiFont(size: 13, weight: .medium))
                                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                                    Text(item.subtitle)
                                        .font(ThemeTokens.uiFont(size: 11))
                                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                                }

                                Spacer()

                                Text(item.type.rawValue)
                                    .font(ThemeTokens.uiFont(size: 10, weight: .semibold))
                                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(ThemeTokens.bgSecondary(for: scheme))
                                    .cornerRadius(3)
                            }
                            .padding(.horizontal, 12)
                            .padding(.vertical, 7)
                            .contentShape(Rectangle())
                            .onTapGesture {
                                item.onSelect()
                                isPresented = false
                            }
                        }
                    }
                    .padding(8)
                }
                .frame(maxHeight: 320)
            }
        }
        .frame(width: 540)
        .background(ThemeTokens.bgElevated(for: scheme))
        .cornerRadius(10)
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 1)
        )
        .shadow(color: Color.black.opacity(0.35), radius: 24, x: 0, y: 12)
    }
}
