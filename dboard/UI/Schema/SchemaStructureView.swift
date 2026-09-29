import SwiftUI
import AppKit

public struct SchemaStructureView: View {
    public let schema: String
    public let tableName: String

    @ObservedObject var connectionManager = ConnectionManager.shared
    @State private var selectedStructureTab: Int = 0 // 0 = Columns, 1 = Indexes, 2 = Constraints, 3 = Triggers, 4 = DDL
    @State private var ddlText: String = ""
    @State private var isDestructiveModalOpen: Bool = false
    @Environment(\.colorScheme) var scheme

    private var tableMeta: TableMetadata? {
        connectionManager.activeDriver?.metadata.table(named: tableName, schema: schema)
    }

    private var tableIndexes: [IndexMetadata] {
        connectionManager.activeDriver?.metadata.indexes.filter { $0.tableName == tableName } ?? []
    }

    private var tableConstraints: [ConstraintMetadata] {
        connectionManager.activeDriver?.metadata.constraints.filter { $0.tableName == tableName } ?? []
    }

    private var tableTriggers: [TriggerMetadata] {
        connectionManager.activeDriver?.metadata.triggers.filter { $0.tableName == tableName } ?? []
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Header Bar
            HStack(spacing: 12) {
                HStack(spacing: 6) {
                    Image(systemName: "wrench.and.screwdriver")
                        .foregroundColor(ThemeTokens.accentBlue)
                    Text("\(schema).\(tableName)")
                        .font(ThemeTokens.uiFont(size: 13, weight: .bold))
                }

                // Sub-tabs
                Picker("", selection: $selectedStructureTab) {
                    Text("Columns (\(tableMeta?.columns.count ?? 0))").tag(0)
                    Text("Indexes (\(tableIndexes.count))").tag(1)
                    Text("Constraints (\(tableConstraints.count))").tag(2)
                    Text("Triggers (\(tableTriggers.count))").tag(3)
                    Text("DDL").tag(4)
                }
                .pickerStyle(.segmented)
                .frame(width: 440)

                Spacer()

                Button("Drop Table...") {
                    isDestructiveModalOpen = true
                }
                .font(ThemeTokens.uiFont(size: 11))
                .foregroundColor(ThemeTokens.accentCrimson)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .background(ThemeTokens.bgElevated(for: scheme))
            .overlay(
                Rectangle()
                    .frame(height: 1)
                    .foregroundColor(ThemeTokens.borderColor(for: scheme)),
                alignment: .bottom
            )

            // Content Area
            if selectedStructureTab == 0 {
                columnsTableView
            } else if selectedStructureTab == 1 {
                indexesTableView
            } else if selectedStructureTab == 2 {
                constraintsTableView
            } else if selectedStructureTab == 3 {
                triggersTableView
            } else {
                ddlViewer
            }
        }
        .background(ThemeTokens.bgPrimary(for: scheme))
        .onAppear {
            loadDDL()
        }
        .onChange(of: tableName) { _, _ in
            loadDDL()
        }
        .onChange(of: schema) { _, _ in
            loadDDL()
        }
        .sheet(isPresented: $isDestructiveModalOpen) {
            DestructiveConfirmationModal(
                isPresented: $isDestructiveModalOpen,
                title: "Drop Table '\(tableName)'?",
                message: "This will permanently drop the table and all associated indexes and data. This operation cannot be rolled back.",
                environment: connectionManager.activeConnection?.environment ?? .local,
                requiredPhrase: connectionManager.activeConnection?.environment == .production ? tableName : nil,
                onConfirm: {
                    Task {
                        try? await connectionManager.activeDriver?.dropTable(schema: schema, table: tableName)
                        ToastManager.shared.show("Table Dropped", style: .warning)
                    }
                }
            )
        }
    }

    private var columnsTableView: some View {
        ScrollView {
            VStack(spacing: 0) {
                // Header
                HStack(spacing: 0) {
                    Text("#").font(ThemeTokens.codeBoldFont(size: 10)).frame(width: 32)
                    Text("Column Name").font(ThemeTokens.uiFont(size: 11, weight: .bold)).frame(width: 160, alignment: .leading)
                    Text("Data Type").font(ThemeTokens.uiFont(size: 11, weight: .bold)).frame(width: 140, alignment: .leading)
                    Text("Nullable").font(ThemeTokens.uiFont(size: 11, weight: .bold)).frame(width: 70)
                    Text("Default Value").font(ThemeTokens.uiFont(size: 11, weight: .bold)).frame(width: 160, alignment: .leading)
                    Text("Key / Flags").font(ThemeTokens.uiFont(size: 11, weight: .bold)).frame(width: 120, alignment: .leading)
                    Spacer()
                }
                .padding(.horizontal, 10)
                .frame(height: 26)
                .background(ThemeTokens.tableHeaderBg(for: scheme))
                .border(ThemeTokens.borderColor(for: scheme), width: 0.5)

                // Rows
                ForEach(tableMeta?.columns ?? []) { col in
                    HStack(spacing: 0) {
                        Text("\(col.ordinalPosition)")
                            .font(ThemeTokens.codeFont(size: 10))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                            .frame(width: 32)

                        Text(col.name)
                            .font(ThemeTokens.codeBoldFont(size: 11.5))
                            .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                            .frame(width: 160, alignment: .leading)

                        Text(col.dataTypeName)
                            .font(ThemeTokens.codeFont(size: 11))
                            .foregroundColor(ThemeTokens.accentBlue)
                            .frame(width: 140, alignment: .leading)

                        Image(systemName: col.isNullable ? "checkmark" : "xmark")
                            .font(.system(size: 14, weight: .bold))
                            .foregroundColor(col.isNullable ? ThemeTokens.accentEmerald : ThemeTokens.textMuted(for: scheme))
                            .frame(width: 70)

                        Text(col.defaultValue ?? "—")
                            .font(ThemeTokens.codeFont(size: 10.5))
                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                            .frame(width: 160, alignment: .leading)

                        HStack(spacing: 4) {
                            if col.isPrimaryKey {
                                Text("PK")
                                    .font(ThemeTokens.codeBoldFont(size: 9))
                                    .foregroundColor(ThemeTokens.accentAmber)
                                    .padding(.horizontal, 4)
                                    .background(ThemeTokens.accentAmber.opacity(0.15))
                                    .cornerRadius(3)
                            }
                            if col.isForeignKey {
                                Text("FK → \(col.foreignTable ?? "")")
                                    .font(ThemeTokens.codeBoldFont(size: 9))
                                    .foregroundColor(ThemeTokens.accentBlue)
                                    .padding(.horizontal, 4)
                                    .background(ThemeTokens.accentBlue.opacity(0.15))
                                    .cornerRadius(3)
                            }
                        }
                        .frame(width: 120, alignment: .leading)

                        Spacer()
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 26)
                    .background(col.ordinalPosition % 2 == 0 ? ThemeTokens.tableRowEven(for: scheme) : ThemeTokens.tableRowOdd(for: scheme))
                    .border(ThemeTokens.borderColor(for: scheme), width: 0.5)
                }
            }
        }
    }

    private var indexesTableView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(tableIndexes) { idx in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(idx.name)
                                .font(ThemeTokens.codeBoldFont(size: 12))
                                .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                            if idx.isPrimary {
                                Text("PRIMARY")
                                    .font(ThemeTokens.uiFont(size: 9.5, weight: .bold))
                                    .foregroundColor(ThemeTokens.accentAmber)
                                    .padding(.horizontal, 5)
                                    .background(ThemeTokens.accentAmber.opacity(0.15))
                                    .cornerRadius(3)
                            }

                            if idx.isUnique {
                                Text("UNIQUE")
                                    .font(ThemeTokens.uiFont(size: 9.5, weight: .bold))
                                    .foregroundColor(ThemeTokens.accentEmerald)
                                    .padding(.horizontal, 5)
                                    .background(ThemeTokens.accentEmerald.opacity(0.15))
                                    .cornerRadius(3)
                            }

                            Spacer()

                            Text(idx.method)
                                .font(ThemeTokens.codeFont(size: 10))
                                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                        }

                        Text("Columns: \(idx.columnNames.joined(separator: ", "))")
                            .font(ThemeTokens.codeFont(size: 11))
                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))

                        Text(idx.definition)
                            .font(ThemeTokens.codeFont(size: 10.5))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .padding(10)
                    .background(ThemeTokens.bgElevated(for: scheme))
                    .cornerRadius(6)
                    .overlay(
                        RoundedRectangle(cornerRadius: 6)
                            .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.8)
                    )
                }
            }
            .padding(12)
        }
    }

    private var constraintsTableView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(tableConstraints) { c in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(c.name)
                                .font(ThemeTokens.codeBoldFont(size: 12))
                            Spacer()
                            Text(c.type.rawValue)
                                .font(ThemeTokens.uiFont(size: 10, weight: .bold))
                                .foregroundColor(ThemeTokens.accentBlue)
                        }
                        Text(c.definition)
                            .font(ThemeTokens.codeFont(size: 11))
                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    }
                    .padding(10)
                    .background(ThemeTokens.bgElevated(for: scheme))
                    .cornerRadius(6)
                }
            }
            .padding(12)
        }
    }

    private var triggersTableView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(tableTriggers) { trg in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(trg.name)
                                .font(ThemeTokens.codeBoldFont(size: 12))
                            Spacer()
                            Text("\(trg.timing) \(trg.event)")
                                .font(ThemeTokens.codeFont(size: 10.5))
                                .foregroundColor(ThemeTokens.accentAmber)
                        }
                        Text("Calls: \(trg.functionName)()")
                            .font(ThemeTokens.codeFont(size: 11))
                        Text(trg.definition)
                            .font(ThemeTokens.codeFont(size: 10))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .padding(10)
                    .background(ThemeTokens.bgElevated(for: scheme))
                    .cornerRadius(6)
                }
            }
            .padding(12)
        }
    }

    private var ddlViewer: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Generated SQL DDL")
                    .font(ThemeTokens.uiFont(size: 12, weight: .bold))
                Spacer()
                Button("Copy DDL") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(ddlText, forType: .string)
                    ToastManager.shared.show("Copied DDL to Clipboard", style: .success)
                }
                .font(ThemeTokens.uiFont(size: 11))
            }
            .padding(.horizontal, 12)
            .padding(.top, 8)

            TextEditor(text: $ddlText)
                .font(ThemeTokens.codeFont(size: 12))
                .padding(8)
                .background(ThemeTokens.bgElevated(for: scheme))
        }
    }

    private func loadDDL() {
        guard let driver = connectionManager.activeDriver else { return }
        Task {
            if let ddl = try? await driver.generateTableDDL(schema: schema, table: tableName) {
                self.ddlText = ddl
            }
        }
    }
}
