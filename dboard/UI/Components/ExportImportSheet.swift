import SwiftUI
import AppKit

public enum ExportFormat: String, CaseIterable, Identifiable {
    case csv = "CSV"
    case json = "JSON"
    case sql = "SQL INSERT Statements"
    case tsv = "TSV (Tab-separated)"

    public var id: String { rawValue }
}

public struct ExportModalView: View {
    @Binding var isPresented: Bool
    public let tableName: String
    public let columns: [ColumnDefinition]
    public let rows: [DataRow]
    @State private var selectedFormat: ExportFormat = .csv
    @State private var includeHeaders: Bool = true
    @State private var isExporting: Bool = false
    @State private var exportProgress: Double = 0.0
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Image(systemName: "square.and.arrow.up")
                    .foregroundColor(ThemeTokens.accentBlue)
                    .font(.system(size: 20))

                Text("Export Data: \(tableName)")
                    .font(ThemeTokens.uiFont(size: 14, weight: .semibold))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                Spacer()

                Button(action: { isPresented = false }) {
                    Image(systemName: "xmark")
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }
                .buttonStyle(.plain)
            }

            Divider().background(ThemeTokens.borderColor(for: scheme))

            VStack(alignment: .leading, spacing: 12) {
                Text("Export Format")
                    .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))

                Picker("", selection: $selectedFormat) {
                    ForEach(ExportFormat.allCases) { format in
                        Text(format.rawValue).tag(format)
                    }
                }
                .pickerStyle(.segmented)

                if selectedFormat == .csv || selectedFormat == .tsv {
                    Toggle("Include column header row", isOn: $includeHeaders)
                        .font(ThemeTokens.uiFont(size: 12))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                }

                HStack {
                    Text("Total Rows:")
                        .font(ThemeTokens.uiFont(size: 12))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    Text("\(rows.count) records")
                        .font(ThemeTokens.codeFont(size: 12))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                }

                if isExporting {
                    VStack(alignment: .leading, spacing: 6) {
                        ProgressView(value: exportProgress, total: 1.0)
                        Text("Exporting data... \(Int(exportProgress * 100))%")
                            .font(ThemeTokens.uiFont(size: 11))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                }
            }

            Divider().background(ThemeTokens.borderColor(for: scheme))

            HStack {
                Button("Copy to Clipboard") {
                    copyToClipboard()
                }
                .font(ThemeTokens.uiFont(size: 12))

                Spacer()

                Button("Cancel") {
                    isPresented = false
                }
                .font(ThemeTokens.uiFont(size: 12))

                Button("Save to File...") {
                    saveToFile()
                }
                .font(ThemeTokens.uiFont(size: 12, weight: .semibold))
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(20)
        .frame(width: 440)
        .background(ThemeTokens.bgElevated(for: scheme))
        .cornerRadius(10)
    }

    private func generateExportString() -> String {
        switch selectedFormat {
        case .csv:
            return generateCSV(separator: ",")
        case .tsv:
            return generateCSV(separator: "\t")
        case .json:
            return generateJSON()
        case .sql:
            return generateSQLInserts()
        }
    }

    private func generateCSV(separator: String) -> String {
        var lines: [String] = []
        if includeHeaders {
            lines.append(columns.map { "\"\($0.name)\"" }.joined(separator: separator))
        }
        for row in rows {
            let rowVals = columns.map { col in
                let val = row[col.name]
                if val.isNull { return "" }
                let escaped = val.displayText.replacingOccurrences(of: "\"", with: "\"\"")
                return "\"\(escaped)\""
            }
            lines.append(rowVals.joined(separator: separator))
        }
        return lines.joined(separator: "\n")
    }

    private func generateJSON() -> String {
        var list: [[String: Any]] = []
        for row in rows {
            var dict: [String: Any] = [:]
            for col in columns {
                let val = row[col.name]
                switch val {
                case .null: dict[col.name] = NSNull()
                case .string(let s): dict[col.name] = s
                case .integer(let i): dict[col.name] = i
                case .double(let d): dict[col.name] = d
                case .boolean(let b): dict[col.name] = b
                case .date(let d): dict[col.name] = ISO8601DateFormatter().string(from: d)
                default: dict[col.name] = val.displayText
                }
            }
            list.append(dict)
        }
        if let data = try? JSONSerialization.data(withJSONObject: list, options: [.prettyPrinted]),
           let str = String(data: data, encoding: .utf8) {
            return str
        }
        return "[]"
    }

    private func generateSQLInserts() -> String {
        var lines: [String] = []
        let colNames = columns.map { "\"\($0.name)\"" }.joined(separator: ", ")
        for row in rows {
            let valList = columns.map { row[$0.name].sqlLiteral }.joined(separator: ", ")
            lines.append("INSERT INTO \"\(tableName)\" (\(colNames)) VALUES (\(valList));")
        }
        return lines.joined(separator: "\n")
    }

    private func copyToClipboard() {
        let text = generateExportString()
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        ToastManager.shared.show("Copied to Clipboard", subtitle: "\(rows.count) rows exported as \(selectedFormat.rawValue)", style: .success)
        isPresented = false
    }

    private func saveToFile() {
        let panel = NSSavePanel()
        panel.canCreateDirectories = true
        let ext: String
        switch selectedFormat {
        case .csv: ext = "csv"
        case .tsv: ext = "tsv"
        case .json: ext = "json"
        case .sql: ext = "sql"
        }
        panel.nameFieldStringValue = "\(tableName)_export.\(ext)"

        if panel.runModal() == .OK, let url = panel.url {
            let text = generateExportString()
            try? text.write(to: url, atomically: true, encoding: .utf8)
            ToastManager.shared.show("Export Complete", subtitle: "Saved to \(url.lastPathComponent)", style: .success)
            isPresented = false
        }
    }
}
