import SwiftUI
import AppKit

public struct TableDataBrowserView: View {
    public let schema: String
    public let tableName: String

    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var tabManager = TabManager.shared
    @ObservedObject var appSettings = AppSettings.shared

    @State private var queryResult: QueryResult = QueryResult()
    @State private var isLoading: Bool = false
    @State private var sortColumn: String? = nil
    @State private var sortAscending: Bool = true
    @State private var filterText: String = ""
    @State private var currentPage: Int = 0
    @State private var pageSize: Int = 100
    @State private var selectedRowId: UUID? = nil
    @State private var selectedColumnName: String? = nil
    @State private var columnWidths: [String: CGFloat] = [:]
    @State private var isExportSheetOpen: Bool = false
    @State private var isInsertRowSheetOpen: Bool = false
    @State private var isDeleteRowModalOpen: Bool = false
    @State private var insertValues: [String: String] = [:]
    @State private var editingJSONContext: (row: DataRow, column: ColumnDefinition, value: String)? = nil
    @FocusState private var gridFocused: Bool
    @Environment(\.colorScheme) var scheme

    private var tableMeta: TableMetadata? {
        connectionManager.activeDriver?.metadata.table(named: tableName, schema: schema)
    }

    private var hasPrimaryKey: Bool {
        tableMeta?.hasPrimaryKey ?? false
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Safety banner if no primary key
            if !hasPrimaryKey && !isLoading {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundColor(ThemeTokens.accentAmber)
                        .font(.system(size: 12))

                    Text("No Primary Key Detected: Automatic inline editing is restricted for '\(tableName)' to prevent unintended multi-row modifications.")
                        .font(ThemeTokens.uiFont(size: 11.5, weight: .medium))
                        .foregroundColor(ThemeTokens.accentAmber)

                    Spacer()

                    Button("Inspect Structure") {
                        tabManager.openTableStructureTab(schema: schema, table: tableName)
                    }
                    .font(ThemeTokens.uiFont(size: 11))
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(ThemeTokens.accentAmber.opacity(0.12))
                .overlay(
                    Rectangle()
                        .frame(height: 1)
                        .foregroundColor(ThemeTokens.accentAmber.opacity(0.3)),
                    alignment: .bottom
                )
            }

            // Filter Bar
            TableFilterBarView(
                columns: queryResult.columns,
                filterText: $filterText,
                onApplyFilter: {
                    Task { await loadData() }
                },
                onClearFilter: {
                    Task { await loadData() }
                }
            )

            // Table Grid Area
            if isLoading && queryResult.rows.isEmpty {
                VStack(spacing: 12) {
                    ProgressView()
                    Text("Loading rows from \(tableName)...")
                        .font(ThemeTokens.uiFont(size: 12))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if queryResult.rows.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: "tray")
                        .font(.system(size: 28))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    Text("No records found in \(tableName)")
                        .font(ThemeTokens.uiFont(size: 13, weight: .medium))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    Button("Insert New Row") {
                        isInsertRowSheetOpen = true
                    }
                    .buttonStyle(.borderedProminent)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                tableScrollView
            }

            // Bottom Pagination & Latency Bar
            PaginationBarView(
                currentPage: $currentPage,
                pageSize: $pageSize,
                totalRows: queryResult.totalRowCount,
                executionDurationMs: queryResult.executionDurationMs,
                onPageChange: { newPage in
                    currentPage = newPage
                    Task { await loadData() }
                },
                onExportClick: {
                    isExportSheetOpen = true
                },
                onInsertRowClick: {
                    isInsertRowSheetOpen = true
                },
                canDeleteRow: hasPrimaryKey && selectedRowId != nil,
                onDeleteRowClick: {
                    isDeleteRowModalOpen = true
                }
            )
        }
        .background(ThemeTokens.bgPrimary(for: scheme))
        .onAppear {
            Task { await loadData() }
        }
        .onChange(of: tableName) { _, _ in
            Task { await loadData() }
        }
        .onChange(of: schema) { _, _ in
            Task { await loadData() }
        }
        .sheet(isPresented: $isExportSheetOpen) {
            ExportModalView(
                isPresented: $isExportSheetOpen,
                tableName: tableName,
                columns: queryResult.columns,
                rows: queryResult.rows
            )
        }
        .sheet(isPresented: $isInsertRowSheetOpen) {
            insertRowSheet
        }
        .sheet(isPresented: $isDeleteRowModalOpen) {
            DestructiveConfirmationModal(
                isPresented: $isDeleteRowModalOpen,
                title: "Delete selected row from '\(tableName)'?",
                message: "This permanently deletes the row from the database. Use Edit History to revert.",
                environment: connectionManager.activeConnection?.environment ?? .local,
                requiredPhrase: connectionManager.activeConnection?.environment == .production ? tableName : nil,
                onConfirm: {
                    Task { await deleteSelectedRow() }
                }
            )
        }
        .sheet(isPresented: Binding(
            get: { editingJSONContext != nil },
            set: { if !$0 { editingJSONContext = nil } }
        )) {
            if let ctx = editingJSONContext {
                JSONEditorSheet(
                    isPresented: Binding(
                        get: { editingJSONContext != nil },
                        set: { if !$0 { editingJSONContext = nil } }
                    ),
                    initialJSON: ctx.value
                ) { newJSON in
                    let parsed = DataValue.json(newJSON)
                    Task {
                        try? await commitCellEdit(row: ctx.row, column: ctx.column, newValue: parsed)
                    }
                }
            }
        }
    }

    private var totalTableWidth: CGFloat {
        let colsWidth = queryResult.columns.reduce(CGFloat(0)) { sum, col in
            sum + (columnWidths[col.name] ?? defaultWidthForColumn(col))
        }
        return max(colsWidth + 44, 400)
    }

    private var tableScrollView: some View {
        ScrollView(.horizontal, showsIndicators: true) {
            VStack(alignment: .leading, spacing: 0) {
                // Pinned Header Row (always visible when scrolling vertically)
                headerRow
                    .frame(width: totalTableWidth, height: 26)
                    .zIndex(2)

                // High-performance single-axis virtualized vertical scroll view
                ScrollViewReader { proxy in
                    ScrollView(.vertical, showsIndicators: true) {
                        LazyVStack(alignment: .leading, spacing: 0) {
                            ForEach(Array(queryResult.rows.enumerated()), id: \.element.id) { index, row in
                                dataRowView(index: index, row: row)
                            }
                        }
                        .frame(width: totalTableWidth)
                    }
                    .onChange(of: selectedRowId) { _, newId in
                        if let newId { proxy.scrollTo(newId) }
                    }
                }
            }
            .frame(width: totalTableWidth)
        }
        .focusable()
        .focusEffectDisabled()
        .focused($gridFocused)
        .onKeyPress(keys: [.upArrow, .downArrow, .leftArrow, .rightArrow]) { press in
            moveSelection(for: press.key)
            return .handled
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "c"), phases: .down) { press in
            guard press.modifiers.contains(.command) else { return .ignored }
            copySelection(wholeRow: press.modifiers.contains(.shift))
            return .handled
        }
    }

    private func moveSelection(for key: KeyEquivalent) {
        let rows = queryResult.rows
        let cols = queryResult.columns
        guard !rows.isEmpty, !cols.isEmpty else { return }

        let rowIdx = rows.firstIndex(where: { $0.id == selectedRowId }) ?? -1
        let colIdx = cols.firstIndex(where: { $0.name == selectedColumnName }) ?? 0

        var newRow = max(rowIdx, 0)
        var newCol = colIdx
        switch key {
        case .upArrow: newRow = max(0, rowIdx - 1)
        case .downArrow: newRow = min(rows.count - 1, rowIdx + 1)
        case .leftArrow: newCol = max(0, colIdx - 1)
        case .rightArrow: newCol = min(cols.count - 1, colIdx + 1)
        default: return
        }
        selectedRowId = rows[newRow].id
        selectedColumnName = cols[newCol].name
    }

    private func copySelection(wholeRow: Bool) {
        guard let row = queryResult.rows.first(where: { $0.id == selectedRowId }) else { return }
        let text: String
        if wholeRow {
            text = queryResult.columns.map { row[$0.name].displayText }.joined(separator: "\t")
        } else if let colName = selectedColumnName {
            text = row[colName].displayText
        } else {
            return
        }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        ToastManager.shared.show(wholeRow ? "Copied row" : "Copied cell", style: .success, duration: 1.0)
    }

    private var headerRow: some View {
        HStack(spacing: 0) {
            // Row index column header (#)
            Text("#")
                .font(ThemeTokens.codeBoldFont(size: 10.5))
                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                .frame(width: 44, height: 26)
                .background(ThemeTokens.tableHeaderBg(for: scheme))
                .border(ThemeTokens.borderColor(for: scheme), width: 0.5)

            ForEach(queryResult.columns) { col in
                let width = columnWidths[col.name] ?? defaultWidthForColumn(col)
                headerCell(col: col, width: width)
            }
        }
    }

    private func dataRowView(index: Int, row: DataRow) -> some View {
        let isSelected = row.id == selectedRowId
        let globalIndex = (currentPage * pageSize) + index + 1
        return TableDataRowView(
            index: index,
            globalIndex: globalIndex,
            row: row,
            columns: queryResult.columns,
            columnWidths: columnWidths,
            isSelected: isSelected,
            selectedColumnName: isSelected ? selectedColumnName : nil,
            hasPrimaryKey: hasPrimaryKey,
            scheme: scheme,
            onSelectRow: {
                selectedRowId = row.id
                gridFocused = true
            },
            onSelectCell: { colName in
                gridFocused = true
                selectedRowId = row.id
                selectedColumnName = colName
            },
            onRequestJSONEdit: { col, val in
                editingJSONContext = (row: row, column: col, value: val)
            },
            onCommitCell: { col, newVal in
                try await commitCellEdit(row: row, column: col, newValue: newVal)
            },
            defaultWidthForColumn: { col in
                defaultWidthForColumn(col)
            }
        )
        .equatable()
    }

    private func headerCell(col: ColumnDefinition, width: CGFloat) -> some View {
        HStack(spacing: 0) {
            HStack(spacing: 4) {
                if col.isPrimaryKey {
                    Image(systemName: "key.fill")
                        .font(.system(size: 8))
                        .foregroundColor(ThemeTokens.accentAmber)
                } else if col.isForeignKey {
                    Image(systemName: "arrow.turn.down.right")
                        .font(.system(size: 8))
                        .foregroundColor(ThemeTokens.accentBlue)
                }

                Text(col.name)
                    .font(ThemeTokens.uiFont(size: 11.5, weight: .bold))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                    .lineLimit(1)

                Text(col.dataTypeName)
                    .font(ThemeTokens.codeFont(size: 9))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    .lineLimit(1)

                Spacer()

                if sortColumn == col.name {
                    Image(systemName: sortAscending ? "chevron.up" : "chevron.down")
                        .font(.system(size: 8, weight: .bold))
                        .foregroundColor(ThemeTokens.accentBlue)
                }
            }
            .padding(.horizontal, 6)
            .frame(width: max(30, width - 6), height: 26)
            .contentShape(Rectangle())
            .onTapGesture {
                if sortColumn == col.name {
                    sortAscending.toggle()
                } else {
                    sortColumn = col.name
                    sortAscending = true
                }
                Task { await loadData() }
            }

            // Interactive Column Resize Handle
            Rectangle()
                .fill(ThemeTokens.borderColor(for: scheme).opacity(0.6))
                .frame(width: 6, height: 26)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture()
                        .onChanged { gesture in
                            let current = columnWidths[col.name] ?? defaultWidthForColumn(col)
                            columnWidths[col.name] = max(55, current + gesture.translation.width)
                        }
                )
        }
        .frame(width: width, height: 26)
        .background(ThemeTokens.tableHeaderBg(for: scheme))
        .border(ThemeTokens.borderColor(for: scheme), width: 0.5)
    }

    private func defaultWidthForColumn(_ col: ColumnDefinition) -> CGFloat {
        if col.name == "id" || col.name == "_id" { return 70 }
        if col.name.contains("email") { return 180 }
        if col.name.contains("name") || col.name.contains("title") { return 160 }
        if col.dataTypeName.contains("json") { return 220 }
        if col.dataTypeName.contains("time") || col.dataTypeName.contains("date") { return 160 }
        return 130
    }

    private func loadData() async {
        guard let driver = connectionManager.activeDriver else { return }
        isLoading = true
        do {
            let offset = currentPage * pageSize
            let res = try await driver.fetchTableRows(
                schema: schema,
                table: tableName,
                limit: pageSize,
                offset: offset,
                sortColumn: sortColumn,
                sortAscending: sortAscending,
                filterClause: filterText.isEmpty ? nil : filterText
            )
            self.queryResult = res
            self.isLoading = false
        } catch {
            self.isLoading = false
            ToastManager.shared.show("Failed to load table", subtitle: error.localizedDescription, style: .error)
        }
    }

    private func deleteSelectedRow() async {
        guard let driver = connectionManager.activeDriver,
              let rowId = selectedRowId,
              let row = queryResult.rows.first(where: { $0.id == rowId }) else { return }

        var pks: [String: DataValue] = [:]
        for pkName in tableMeta?.primaryKeyColumnNames ?? [] {
            pks[pkName] = row[pkName]
        }
        // Refuse to run a DELETE without a WHERE clause.
        guard !pks.isEmpty else {
            ToastManager.shared.show("Cannot delete row", subtitle: "No primary key available for this table", style: .error)
            return
        }

        do {
            try await driver.deleteRow(schema: schema, table: tableName, primaryKeys: pks)
            selectedRowId = nil
            selectedColumnName = nil
            ToastManager.shared.show("Row Deleted", style: .success, duration: 1.5)
            await loadData()
        } catch {
            ToastManager.shared.show("Failed to delete row", subtitle: error.localizedDescription, style: .error)
        }
    }

    private func commitCellEdit(row: DataRow, column: ColumnDefinition, newValue: DataValue) async throws {
        guard let driver = connectionManager.activeDriver else { return }

        // Gather primary key values
        var pks: [String: DataValue] = [:]
        let pkNames = tableMeta?.primaryKeyColumnNames ?? ["id"]
        for pkName in pkNames {
            pks[pkName] = row[pkName]
        }

        let payload = CellEditPayload(
            rowId: row.id,
            schema: schema,
            tableName: tableName,
            columnName: column.name,
            oldValue: row[column.name],
            newValue: newValue,
            primaryKeys: pks
        )

        _ = try await driver.executeCellEdit(payload: payload)

        // Update local state immediately
        if let idx = queryResult.rows.firstIndex(where: { $0.id == row.id }) {
            queryResult.rows[idx].values[column.name] = newValue
        }

        ToastManager.shared.show("Row Updated", subtitle: "\(column.name) saved to database", style: .success, duration: 1.5)
    }

    private var insertRowSheet: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Image(systemName: "plus.circle.fill")
                    .foregroundColor(ThemeTokens.accentBlue)
                Text("Insert Row into \(tableName)")
                    .font(ThemeTokens.uiFont(size: 14, weight: .bold))
                Spacer()
                Button(action: { isInsertRowSheetOpen = false }) {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.plain)
            }

            ScrollView {
                VStack(spacing: 8) {
                    ForEach(queryResult.columns.filter { !($0.defaultValue?.contains("nextval") ?? false) }) { col in
                        HStack {
                            Text(col.name)
                                .font(ThemeTokens.codeBoldFont(size: 11))
                                .frame(width: 120, alignment: .leading)

                            TextField(col.dataTypeName, text: Binding(
                                get: { insertValues[col.name] ?? "" },
                                set: { insertValues[col.name] = $0 }
                            ))
                            .textFieldStyle(.roundedBorder)
                        }
                    }
                }
            }
            .frame(maxHeight: 280)

            HStack {
                Spacer()
                Button("Cancel") { isInsertRowSheetOpen = false }
                Button("Execute INSERT") {
                    Task {
                        var parsedVals: [String: DataValue] = [:]
                        for col in queryResult.columns {
                            if let raw = insertValues[col.name], !raw.isEmpty {
                                parsedVals[col.name] = DataValue.parseFromInput(raw, targetType: col.dataTypeName)
                            }
                        }
                        if let driver = connectionManager.activeDriver {
                            _ = try? await driver.insertRow(schema: schema, table: tableName, values: parsedVals)
                            await loadData()
                            ToastManager.shared.show("Row Inserted", style: .success)
                        }
                        isInsertRowSheetOpen = false
                    }
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(16)
        .frame(width: 440)
    }
}

public struct TableDataRowView: View, Equatable {
    public let index: Int
    public let globalIndex: Int
    public let row: DataRow
    public let columns: [ColumnDefinition]
    public let columnWidths: [String: CGFloat]
    public let isSelected: Bool
    public let selectedColumnName: String?
    public let hasPrimaryKey: Bool
    public let scheme: ColorScheme
    public let onSelectRow: () -> Void
    public let onSelectCell: (String) -> Void
    public let onRequestJSONEdit: (ColumnDefinition, String) -> Void
    public let onCommitCell: (ColumnDefinition, DataValue) async throws -> Void
    public let defaultWidthForColumn: (ColumnDefinition) -> CGFloat

    @State private var pendingEditColumn: String? = nil

    public static func == (lhs: TableDataRowView, rhs: TableDataRowView) -> Bool {
        lhs.index == rhs.index &&
        lhs.globalIndex == rhs.globalIndex &&
        lhs.row == rhs.row &&
        lhs.isSelected == rhs.isSelected &&
        lhs.selectedColumnName == rhs.selectedColumnName &&
        lhs.hasPrimaryKey == rhs.hasPrimaryKey &&
        lhs.scheme == rhs.scheme &&
        lhs.columns == rhs.columns &&
        lhs.columnWidths == rhs.columnWidths
    }

    public var body: some View {
        HStack(spacing: 0) {
            // Row number cell
            Text("\(globalIndex)")
                .font(ThemeTokens.codeFont(size: 10))
                .foregroundColor(isSelected ? ThemeTokens.accentBlue : ThemeTokens.textMuted(for: scheme))
                .frame(width: 44, height: 26)
                .background(isSelected ? ThemeTokens.tableRowSelected(for: scheme) : (index % 2 == 0 ? ThemeTokens.tableRowEven(for: scheme) : ThemeTokens.tableRowOdd(for: scheme)))
                .border(ThemeTokens.borderColor(for: scheme), width: 0.5)

            ForEach(columns) { col in
                let width = columnWidths[col.name] ?? defaultWidthForColumn(col)
                let isCellSelected = isSelected && selectedColumnName == col.name

                let bg = isSelected ? ThemeTokens.tableRowSelected(for: scheme) : (index % 2 == 0 ? ThemeTokens.tableRowEven(for: scheme) : ThemeTokens.tableRowOdd(for: scheme))
                let val = row[col.name]

                // Only the selected cell (and cells with inline controls) pay for the full
                // interactive view; every other cell is a plain Text so scrolling stays cheap.
                if isCellSelected || val.needsInteractiveCell {
                    TableCellView(
                        column: col,
                        value: val,
                        isSelected: isCellSelected,
                        isReadOnly: !hasPrimaryKey,
                        onRequestJSONEdit: { currentVal in
                            onRequestJSONEdit(col, currentVal)
                        },
                        onCommit: { newVal in
                            try await onCommitCell(col, newVal)
                        },
                        onSelect: {
                            onSelectCell(col.name)
                        },
                        autoEdit: isCellSelected && pendingEditColumn == col.name,
                        onAutoEditConsumed: { pendingEditColumn = nil }
                    )
                    .frame(width: width, height: 26)
                    .background(bg)
                } else {
                    Text(val.displayText)
                        .font(val.isNumeric ? ThemeTokens.codeFont(size: 11.5) : ThemeTokens.uiFont(size: 11.5))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                        .lineLimit(1)
                        .padding(.horizontal, 6)
                        .frame(width: width, height: 26, alignment: .leading)
                        .background(bg)
                        .border(ThemeTokens.borderColor(for: scheme).opacity(0.6), width: 0.5)
                        .contentShape(Rectangle())
                        .onTapGesture(count: 2) {
                            onSelectCell(col.name)
                            if hasPrimaryKey { pendingEditColumn = col.name }
                        }
                        .onTapGesture {
                            onSelectCell(col.name)
                        }
                }
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            onSelectRow()
        }
    }
}
