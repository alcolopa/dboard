import SwiftUI
import AppKit

public struct RoutineDetailView: View {
    public let schema: String
    public let routineName: String

    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var tabManager = TabManager.shared
    @State private var selectedTab: Int = 0 // 0 = Code/Definition, 1 = Parameters, 2 = Execute/Test
    @State private var paramValues: [String: String] = [:]
    @State private var executionResult: QueryResult? = nil
    @State private var isExecuting: Bool = false
    @Environment(\.colorScheme) var scheme

    private var routine: RoutineMetadata? {
        connectionManager.activeDriver?.metadata.routines.first {
            $0.schemaName == schema && $0.name == routineName
        }
    }

    public init(schema: String, routineName: String) {
        self.schema = schema
        self.routineName = routineName
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Header Bar
            headerBar

            // Sub-tabs
            HStack(spacing: 12) {
                Picker("", selection: $selectedTab) {
                    Text("Source Code").tag(0)
                    Text("Parameters").tag(1)
                    Text("Call / Test").tag(2)
                }
                .pickerStyle(.segmented)
                .frame(width: 320)

                Spacer()

                Button(action: copyCallTemplate) {
                    HStack(spacing: 4) {
                        Image(systemName: "doc.on.doc")
                            .font(.system(size: 15))
                        Text("Copy Call Template")
                            .font(ThemeTokens.uiFont(size: 11))
                    }
                }
                .buttonStyle(.plain)

                Button(action: openInQueryTab) {
                    HStack(spacing: 4) {
                        Image(systemName: "bolt.fill")
                            .font(.system(size: 15))
                        Text("Open in SQL Editor")
                            .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(ThemeTokens.accentBlue)
                    .foregroundColor(.white)
                    .cornerRadius(4)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .background(ThemeTokens.bgSecondary(for: scheme))
            .border(ThemeTokens.borderColor(for: scheme), width: 0.5)

            // Content Area
            if let r = routine {
                switch selectedTab {
                case 0:
                    codeDefinitionView(r)
                case 1:
                    parametersTableView(r)
                default:
                    executeRoutineView(r)
                }
            } else {
                VStack(spacing: 12) {
                    Image(systemName: "questionmark.circle")
                        .font(.system(size: 36))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    Text("Routine '\(schema).\(routineName)' not found in metadata.")
                        .font(ThemeTokens.uiFont(size: 13))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(ThemeTokens.bgPrimary(for: scheme))
    }

    private var headerBar: some View {
        HStack(spacing: 12) {
            let isProc = routine?.isProcedure ?? false
            Image(systemName: isProc ? "gearshape.2.fill" : "function")
                .foregroundColor(isProc ? ThemeTokens.accentPurple : ThemeTokens.accentBlue)
                .font(.system(size: 20))

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(routineName)
                        .font(ThemeTokens.uiFont(size: 15, weight: .bold))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                    // Type Badge
                    Text(isProc ? "PROCEDURE" : "FUNCTION")
                        .font(ThemeTokens.codeBoldFont(size: 9))
                        .padding(.horizontal, 5)
                        .padding(.vertical, 2)
                        .background((isProc ? ThemeTokens.accentPurple : ThemeTokens.accentBlue).opacity(0.15))
                        .foregroundColor(isProc ? ThemeTokens.accentPurple : ThemeTokens.accentBlue)
                        .cornerRadius(3)

                    // Language Badge
                    if let lang = routine?.language, !lang.isEmpty {
                        Text(lang.uppercased())
                            .font(ThemeTokens.codeFont(size: 9))
                            .padding(.horizontal, 5)
                            .padding(.vertical, 2)
                            .background(ThemeTokens.bgElevated(for: scheme))
                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                            .cornerRadius(3)
                    }
                }

                HStack(spacing: 6) {
                    Text("Schema: \(schema)")
                        .font(ThemeTokens.codeFont(size: 11))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))

                    if let ret = routine?.returnType, !ret.isEmpty && ret != "void" {
                        Text("•")
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                        Text("Returns: \(ret)")
                            .font(ThemeTokens.codeFont(size: 11))
                            .foregroundColor(ThemeTokens.accentEmerald)
                    }
                }
            }

            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(ThemeTokens.bgElevated(for: scheme))
        .border(ThemeTokens.borderColor(for: scheme), width: 0.5)
    }

    private func codeDefinitionView(_ r: RoutineMetadata) -> some View {
        ScrollView([.horizontal, .vertical]) {
            VStack(alignment: .leading, spacing: 0) {
                let code = r.definition.isEmpty ? r.callSyntaxTemplate : r.definition
                let lines = code.components(separatedBy: .newlines)

                ForEach(Array(lines.enumerated()), id: \.offset) { idx, line in
                    HStack(alignment: .top, spacing: 12) {
                        Text("\(idx + 1)")
                            .font(ThemeTokens.codeFont(size: 11))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                            .frame(width: 36, alignment: .trailing)

                        Text(line)
                            .font(ThemeTokens.codeFont(size: 12))
                            .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                            .textSelection(.enabled)
                    }
                    .padding(.vertical, 1)
                }
            }
            .padding(14)
        }
        .background(ThemeTokens.bgPrimary(for: scheme))
    }

    private func parametersTableView(_ r: RoutineMetadata) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            let parsedParams = parseArguments(r.arguments)

            if parsedParams.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "tray")
                        .font(.system(size: 28))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    Text("This \(r.isProcedure ? "procedure" : "function") takes no arguments.")
                        .font(ThemeTokens.uiFont(size: 12))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                VStack(spacing: 0) {
                    // Header
                    HStack(spacing: 0) {
                        Text("Parameter")
                            .font(ThemeTokens.codeBoldFont(size: 11))
                            .frame(width: 200, alignment: .leading)
                        Text("Data Type")
                            .font(ThemeTokens.codeBoldFont(size: 11))
                            .frame(width: 180, alignment: .leading)
                        Text("Default / Constraint")
                            .font(ThemeTokens.codeBoldFont(size: 11))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(ThemeTokens.tableHeaderBg(for: scheme))
                    .border(ThemeTokens.borderColor(for: scheme), width: 0.5)

                    // Rows
                    ScrollView {
                        VStack(spacing: 0) {
                            ForEach(Array(parsedParams.enumerated()), id: \.offset) { idx, p in
                                HStack(spacing: 0) {
                                    Text(p.name)
                                        .font(ThemeTokens.codeFont(size: 11.5))
                                        .foregroundColor(ThemeTokens.accentBlue)
                                        .frame(width: 200, alignment: .leading)

                                    Text(p.type)
                                        .font(ThemeTokens.codeFont(size: 11.5))
                                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                                        .frame(width: 180, alignment: .leading)

                                    Text(p.defaultValue ?? "—")
                                        .font(ThemeTokens.codeFont(size: 11.5))
                                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                }
                                .padding(.horizontal, 14)
                                .padding(.vertical, 6)
                                .background(idx % 2 == 0 ? ThemeTokens.tableRowEven(for: scheme) : ThemeTokens.tableRowOdd(for: scheme))
                                .border(ThemeTokens.borderColor(for: scheme), width: 0.5)
                            }
                        }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func executeRoutineView(_ r: RoutineMetadata) -> some View {
        let parsedParams = parseArguments(r.arguments)

        return HStack(spacing: 0) {
            // Left Input Controls
            VStack(alignment: .leading, spacing: 14) {
                Text("Execute / Call \(r.name)")
                    .font(ThemeTokens.uiFont(size: 13, weight: .bold))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                ScrollView {
                    VStack(alignment: .leading, spacing: 10) {
                        ForEach(parsedParams, id: \.name) { p in
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    Text(p.name)
                                        .font(ThemeTokens.codeBoldFont(size: 11))
                                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                                    Text("(\(p.type))")
                                        .font(ThemeTokens.codeFont(size: 10))
                                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                    Spacer()
                                }

                                TextField(p.defaultValue ?? "Value...", text: Binding(
                                    get: { paramValues[p.name] ?? "" },
                                    set: { paramValues[p.name] = $0 }
                                ))
                                .textFieldStyle(.roundedBorder)
                                .font(ThemeTokens.codeFont(size: 11.5))
                            }
                        }
                    }
                }

                Button(action: executeRoutine) {
                    HStack {
                        if isExecuting {
                            ProgressView()
                                .scaleEffect(0.5)
                                .frame(width: 18, height: 18)
                        } else {
                            Image(systemName: "play.fill")
                                .font(.system(size: 15))
                        }
                        Text(r.isProcedure ? "Execute Procedure" : "Run Function")
                            .font(ThemeTokens.uiFont(size: 12, weight: .semibold))
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 6)
                }
                .buttonStyle(.borderedProminent)
                .disabled(isExecuting)
            }
            .padding(14)
            .frame(width: 320)
            .background(ThemeTokens.bgSecondary(for: scheme))
            .border(ThemeTokens.borderColor(for: scheme), width: 0.5)

            // Right Output Area
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    Text("Execution Output")
                        .font(ThemeTokens.uiFont(size: 12, weight: .bold))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    Spacer()
                    if let res = executionResult {
                        Text(String(format: "%.1f ms", res.executionDurationMs))
                            .font(ThemeTokens.codeFont(size: 10.5))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(ThemeTokens.tableHeaderBg(for: scheme))
                .border(ThemeTokens.borderColor(for: scheme), width: 0.5)

                if let res = executionResult {
                    if let err = res.errorMessage {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Execution Error:")
                                .font(ThemeTokens.uiFont(size: 12, weight: .bold))
                                .foregroundColor(ThemeTokens.accentCrimson)
                            Text(err)
                                .font(ThemeTokens.codeFont(size: 11))
                                .foregroundColor(ThemeTokens.accentCrimson)
                        }
                        .padding(14)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    } else {
                        ScrollView {
                            VStack(alignment: .leading, spacing: 6) {
                                ForEach(res.rows) { row in
                                    Text(row.values.values.map { $0.displayText }.joined(separator: "  |  "))
                                        .font(ThemeTokens.codeFont(size: 11.5))
                                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                                        .padding(.vertical, 2)
                                }
                            }
                            .padding(14)
                        }
                    }
                } else {
                    VStack(spacing: 8) {
                        Image(systemName: "terminal")
                            .font(.system(size: 32))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                        Text("Fill parameters and click Execute to test")
                            .font(ThemeTokens.uiFont(size: 12))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
        }
    }

    private struct ParsedParameter {
        let name: String
        let type: String
        let defaultValue: String?
    }

    private func parseArguments(_ argsStr: String) -> [ParsedParameter] {
        guard !argsStr.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return [] }
        let tokens = argsStr.components(separatedBy: ",")
        var list: [ParsedParameter] = []

        for token in tokens {
            let clean = token.trimmingCharacters(in: .whitespacesAndNewlines)
            if clean.isEmpty { continue }

            var parts = clean.components(separatedBy: .whitespaces).filter { !$0.isEmpty }
            // Check for IN/OUT
            if let first = parts.first?.uppercased(), first == "IN" || first == "OUT" || first == "INOUT" {
                parts.removeFirst()
            }

            let name = parts.first ?? "param"
            var type = parts.count > 1 ? parts[1] : "text"
            var def: String? = nil

            if let defIdx = parts.firstIndex(where: { $0.uppercased() == "DEFAULT" }), defIdx + 1 < parts.count {
                def = parts[(defIdx + 1)...].joined(separator: " ")
                type = parts[1..<defIdx].joined(separator: " ")
            }

            list.append(ParsedParameter(name: name, type: type, defaultValue: def))
        }

        return list
    }

    private func buildExecutionSQL() -> String {
        guard let r = routine else { return "" }
        let parsed = parseArguments(r.arguments)
        var evaluatedArgs: [String] = []

        for p in parsed {
            if let userVal = paramValues[p.name], !userVal.isEmpty {
                evaluatedArgs.append(userVal)
            } else if let def = p.defaultValue {
                evaluatedArgs.append(def)
            } else {
                evaluatedArgs.append("NULL")
            }
        }

        let argsList = evaluatedArgs.joined(separator: ", ")
        if r.isProcedure {
            return "CALL \"\(schema)\".\"\(routineName)\"(\(argsList));"
        } else {
            return "SELECT \"\(schema)\".\"\(routineName)\"(\(argsList));"
        }
    }

    private func copyCallTemplate() {
        let sql = buildExecutionSQL()
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(sql, forType: .string)
        ToastManager.shared.show("Call Template Copied", subtitle: sql, style: .success, duration: 2.0)
    }

    private func openInQueryTab() {
        let sql = buildExecutionSQL()
        tabManager.openQueryTab(initialSQL: sql)
    }

    private func executeRoutine() {
        guard let driver = connectionManager.activeDriver else { return }
        let sql = buildExecutionSQL()
        isExecuting = true
        Task {
            do {
                let res = try await driver.executeQuery(sql: sql, database: connectionManager.activeDatabase)
                await MainActor.run {
                    self.executionResult = res
                    self.isExecuting = false
                    ToastManager.shared.show("Routine Executed", style: .success)
                }
            } catch {
                await MainActor.run {
                    self.executionResult = QueryResult(errorMessage: error.localizedDescription)
                    self.isExecuting = false
                    ToastManager.shared.show("Execution Failed", subtitle: error.localizedDescription, style: .error)
                }
            }
        }
    }
}
