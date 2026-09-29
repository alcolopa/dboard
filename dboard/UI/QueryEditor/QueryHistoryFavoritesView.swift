import SwiftUI

public struct QueryHistoryFavoritesView: View {
    @ObservedObject var historyManager = QueryHistoryManager.shared
    public let onSelectQuery: (String) -> Void
    @State private var selectedTab: Int = 0 // 0 = History, 1 = Saved
    @State private var searchHistoryText: String = ""
    @Environment(\.colorScheme) var scheme

    private var filteredHistory: [QueryHistoryItem] {
        if searchHistoryText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return historyManager.history
        }
        let lower = searchHistoryText.lowercased()
        return historyManager.history.filter {
            $0.query.lowercased().contains(lower) ||
            $0.database.lowercased().contains(lower)
        }
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Segmented Header
            Picker("", selection: $selectedTab) {
                Text("Query History (\(historyManager.history.count))").tag(0)
                Text("Saved Queries").tag(1)
            }
            .pickerStyle(.segmented)
            .padding(8)

            Divider().background(ThemeTokens.borderColor(for: scheme))

            if selectedTab == 0 {
                // History List
                VStack(spacing: 6) {
                    HStack(spacing: 6) {
                        Image(systemName: "magnifyingglass")
                            .font(.system(size: 15))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                        TextField("Search history...", text: $searchHistoryText)
                            .textFieldStyle(.plain)
                            .font(ThemeTokens.uiFont(size: 11))
                        if !searchHistoryText.isEmpty {
                            Button(action: { searchHistoryText = "" }) {
                                Image(systemName: "xmark.circle.fill")
                                    .font(.system(size: 14))
                                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 6)
                    .padding(.vertical, 4)
                    .background(ThemeTokens.bgElevated(for: scheme))
                    .cornerRadius(5)
                    .padding(.horizontal, 8)

                    ScrollView {
                        LazyVStack(spacing: 4) {
                            ForEach(filteredHistory) { item in
                                historyItemCard(item)
                            }
                        }
                        .padding(.horizontal, 8)
                    }
                }
            } else {
                // Saved Query Folders
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 10) {
                        ForEach(historyManager.folders) { folder in
                            VStack(alignment: .leading, spacing: 4) {
                                HStack(spacing: 5) {
                                    Image(systemName: "folder.fill")
                                        .font(.system(size: 15))
                                        .foregroundColor(ThemeTokens.accentAmber)
                                    Text(folder.name)
                                        .font(ThemeTokens.uiFont(size: 11.5, weight: .bold))
                                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                                    Spacer()
                                    Text("\(folder.queries.count)")
                                        .font(ThemeTokens.codeFont(size: 9))
                                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                }

                                ForEach(folder.queries) { sq in
                                    savedQueryCard(sq)
                                }
                            }
                        }
                    }
                    .padding(8)
                }
            }
        }
        .frame(width: 280)
        .background(ThemeTokens.bgSidebar(for: scheme))
        .overlay(
            Rectangle()
                .frame(width: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .leading
        )
    }

    private func historyItemCard(_ item: QueryHistoryItem) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Circle()
                    .fill(item.isSuccess ? ThemeTokens.accentEmerald : ThemeTokens.accentCrimson)
                    .frame(width: 6, height: 6)

                Text(String(format: "%.1f ms", item.durationMs))
                    .font(ThemeTokens.codeFont(size: 10))
                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))

                Spacer()

                Button(action: { historyManager.toggleFavorite(id: item.id) }) {
                    Image(systemName: item.isFavorite ? "star.fill" : "star")
                        .font(.system(size: 14))
                        .foregroundColor(item.isFavorite ? ThemeTokens.accentAmber : ThemeTokens.textMuted(for: scheme))
                }
                .buttonStyle(.plain)
            }

            Text(item.query)
                .font(ThemeTokens.codeFont(size: 10.5))
                .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                .lineLimit(2)

            HStack {
                Text(item.database)
                    .font(ThemeTokens.uiFont(size: 9.5))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                Spacer()
                Text("Insert in Editor →")
                    .font(ThemeTokens.uiFont(size: 9.5, weight: .medium))
                    .foregroundColor(ThemeTokens.accentBlue)
            }
        }
        .padding(8)
        .background(ThemeTokens.bgElevated(for: scheme))
        .cornerRadius(6)
        .overlay(
            RoundedRectangle(cornerRadius: 6)
                .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.5)
        )
        .contentShape(Rectangle())
        .onTapGesture {
            onSelectQuery(item.query)
        }
    }

    private func savedQueryCard(_ sq: SavedQueryItem) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(sq.title)
                .font(ThemeTokens.uiFont(size: 11, weight: .semibold))
                .foregroundColor(ThemeTokens.textPrimary(for: scheme))

            Text(sq.query)
                .font(ThemeTokens.codeFont(size: 10))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .lineLimit(2)
        }
        .padding(6)
        .background(ThemeTokens.bgElevated(for: scheme))
        .cornerRadius(5)
        .contentShape(Rectangle())
        .onTapGesture {
            onSelectQuery(sq.query)
        }
    }
}
