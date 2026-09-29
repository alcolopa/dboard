import SwiftUI
import AppKit

public struct QueryResultsGridView: View {
    public let result: QueryResult
    @Environment(\.colorScheme) var scheme
    @State private var columnWidths: [String: CGFloat] = [:]
    @State private var resizeStartWidths: [String: CGFloat] = [:]

    public var body: some View {
        VStack(spacing: 0) {
            // Stats bar
            HStack(spacing: 12) {
                if result.isSuccess {
                    HStack(spacing: 4) {
                        Circle()
                            .fill(ThemeTokens.accentEmerald)
                            .frame(width: 6, height: 6)
                        Text("Success")
                            .font(ThemeTokens.uiFont(size: 11, weight: .semibold))
                            .foregroundColor(ThemeTokens.accentEmerald)
                    }

                    Text("•")
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))

                    Text("\(result.rows.count) rows")
                        .font(ThemeTokens.codeFont(size: 11))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                    Text("•")
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))

                    Text(String(format: "%.1f ms", result.executionDurationMs))
                        .font(ThemeTokens.codeFont(size: 11))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                } else if let err = result.errorMessage {
                    HStack(spacing: 6) {
                        Image(systemName: "xmark.octagon.fill")
                            .foregroundColor(ThemeTokens.accentCrimson)
                            .font(.system(size: 16))
                        Text(err)
                            .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                            .foregroundColor(ThemeTokens.accentCrimson)
                    }
                }

                Spacer()

                // Copy buttons
                if !result.rows.isEmpty {
                    Button(action: copyAsTSV) {
                        Text("Copy TSV")
                            .font(ThemeTokens.uiFont(size: 10.5))
                    }
                    .buttonStyle(.hit)

                    Button(action: copyAsCSV) {
                        Text("Copy CSV")
                            .font(ThemeTokens.uiFont(size: 10.5))
                    }
                    .buttonStyle(.hit)

                    Button(action: copyAsJSON) {
                        Text("Copy JSON")
                            .font(ThemeTokens.uiFont(size: 10.5))
                    }
                    .buttonStyle(.hit)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(ThemeTokens.tableHeaderBg(for: scheme))
            .overlay(
                Rectangle()
                    .frame(height: 1)
                    .foregroundColor(ThemeTokens.borderColor(for: scheme)),
                alignment: .bottom
            )

            // Grid
            if result.rows.isEmpty {
                VStack(spacing: 6) {
                    Text(result.isSuccess ? "Query executed successfully. (0 rows returned)" : "Query execution failed")
                        .font(ThemeTokens.uiFont(size: 12))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                GeometryReader { geo in
                // Columns keep their own widths; spare panel width stays empty so the
                // vertical scroller sits at the panel's right edge.
                let naturalWidth = result.columns.reduce(CGFloat(40)) { $0 + (columnWidths[$1.name] ?? 140) }
                let totalWidth = max(naturalWidth, geo.size.width)
                ScrollView(.horizontal, showsIndicators: true) {
                    VStack(alignment: .leading, spacing: 0) {
                        // Pinned Header
                        HStack(spacing: 0) {
                            Text("#")
                                .font(ThemeTokens.codeBoldFont(size: 10))
                                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                .frame(width: 40, height: 24)
                                .background(ThemeTokens.tableHeaderBg(for: scheme))
                                .border(ThemeTokens.borderColor(for: scheme), width: 0.5)

                            ForEach(result.columns) { col in
                                let width = columnWidths[col.name] ?? 140
                                HStack(spacing: 0) {
                                    Text(col.name)
                                        .font(ThemeTokens.uiFont(size: 11, weight: .bold))
                                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                                        .lineLimit(1)
                                        .padding(.horizontal, 6)
                                        .frame(maxWidth: .infinity, alignment: .leading)

                                    Rectangle()
                                        .fill(ThemeTokens.borderColor(for: scheme).opacity(0.6))
                                        .frame(width: 8, height: 24)
                                        .contentShape(Rectangle())
                                        .onHover { inside in
                                            if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
                                        }
                                        .gesture(
                                            DragGesture(minimumDistance: 0)
                                                .onChanged { gesture in
                                                    let start = resizeStartWidths[col.name] ?? width
                                                    resizeStartWidths[col.name] = start
                                                    columnWidths[col.name] = max(55, start + gesture.translation.width)
                                                }
                                                .onEnded { _ in resizeStartWidths[col.name] = nil }
                                        )
                                }
                                .frame(width: width, height: 24)
                                .background(ThemeTokens.tableHeaderBg(for: scheme))
                                .border(ThemeTokens.borderColor(for: scheme), width: 0.5)
                            }
                        }
                        .frame(width: totalWidth, height: 24, alignment: .leading)
                        .zIndex(2)

                        // Vertical virtualized scroll
                        ScrollView(.vertical, showsIndicators: true) {
                            LazyVStack(alignment: .leading, spacing: 0) {
                                ForEach(Array(result.rows.enumerated()), id: \.element.id) { idx, row in
                                    QueryResultRowView(
                                        index: idx,
                                        row: row,
                                        columns: result.columns,
                                        columnWidths: columnWidths,
                                        scheme: scheme
                                    )
                                    .equatable()
                                }
                            }
                            .frame(width: totalWidth, alignment: .leading)
                        }
                    }
                    .frame(width: totalWidth, alignment: .leading)
                }
                }
            }
        }
        .background(ThemeTokens.bgPrimary(for: scheme))
    }

    private func copyAsTSV() {
        var lines: [String] = []
        lines.append(result.columns.map { $0.name }.joined(separator: "\t"))
        for row in result.rows {
            lines.append(result.columns.map { row[$0.name].displayText }.joined(separator: "\t"))
        }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(lines.joined(separator: "\n"), forType: .string)
        ToastManager.shared.show("Copied TSV to Clipboard", style: .success)
    }

    private func copyAsCSV() {
        var lines: [String] = []
        lines.append(result.columns.map { "\"\($0.name)\"" }.joined(separator: ","))
        for row in result.rows {
            let rowVals = result.columns.map { col in
                let val = row[col.name]
                if val.isNull { return "" }
                let escaped = val.displayText.replacingOccurrences(of: "\"", with: "\"\"")
                return "\"\(escaped)\""
            }
            lines.append(rowVals.joined(separator: ","))
        }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(lines.joined(separator: "\n"), forType: .string)
        ToastManager.shared.show("Copied CSV to Clipboard", style: .success)
    }

    private func copyAsJSON() {
        var list: [[String: Any]] = []
        for row in result.rows {
            var dict: [String: Any] = [:]
            for col in result.columns {
                dict[col.name] = row[col.name].displayText
            }
            list.append(dict)
        }
        if let data = try? JSONSerialization.data(withJSONObject: list, options: [.prettyPrinted]),
           let str = String(data: data, encoding: .utf8) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(str, forType: .string)
            ToastManager.shared.show("Copied JSON to Clipboard", style: .success)
        }
    }
}

public struct QueryResultRowView: View, Equatable {
    public let index: Int
    public let row: DataRow
    public let columns: [ColumnDefinition]
    public let columnWidths: [String: CGFloat]
    public let scheme: ColorScheme

    public static func == (lhs: QueryResultRowView, rhs: QueryResultRowView) -> Bool {
        lhs.index == rhs.index &&
        lhs.row == rhs.row &&
        lhs.columns == rhs.columns &&
        lhs.columnWidths == rhs.columnWidths &&
        lhs.scheme == rhs.scheme
    }

    public var body: some View {
        HStack(spacing: 0) {
            Text("\(index + 1)")
                .font(ThemeTokens.codeFont(size: 10))
                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                .frame(width: 40, height: 24)
                .background(index % 2 == 0 ? ThemeTokens.tableRowEven(for: scheme) : ThemeTokens.tableRowOdd(for: scheme))
                .border(ThemeTokens.borderColor(for: scheme), width: 0.5)

            ForEach(columns) { col in
                let val = row[col.name]
                Text(val.displayText)
                    .font(val.isNumeric ? ThemeTokens.codeFont(size: 11) : ThemeTokens.uiFont(size: 11))
                    .foregroundColor(val.isNull ? ThemeTokens.textMuted(for: scheme) : ThemeTokens.textPrimary(for: scheme))
                    .padding(.horizontal, 6)
                    .frame(width: columnWidths[col.name] ?? 140, height: 24, alignment: .leading)
                    .background(index % 2 == 0 ? ThemeTokens.tableRowEven(for: scheme) : ThemeTokens.tableRowOdd(for: scheme))
                    .border(ThemeTokens.borderColor(for: scheme), width: 0.5)
            }
        }
    }
}
