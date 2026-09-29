import SwiftUI

public struct SidebarView: View {
    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var tabManager = TabManager.shared
    @State private var objectFilterText: String = ""
    @State private var isTablesExpanded: Bool = true
    @State private var isViewsExpanded: Bool = true
    @State private var isProceduresExpanded: Bool = true
    @State private var isFunctionsExpanded: Bool = true
    @State private var isTriggersExpanded: Bool = false
    @State private var isSequencesExpanded: Bool = false
    @State private var isCollectionsExpanded: Bool = true
    @Binding var isConnectionManagerOpen: Bool
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        VStack(spacing: 0) {
            // Header: Connection Info & Status
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) {
                    if let active = connectionManager.activeConnection {
                        Circle()
                            .fill(Color(hex: active.colorTag ?? active.environment.badgeColorHex))
                            .frame(width: 8, height: 8)
                        Text(active.name)
                            .font(ThemeTokens.uiFont(size: 12, weight: .bold))
                            .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                            .lineLimit(1)
                    } else {
                        Circle()
                            .fill(ThemeTokens.textMuted(for: scheme))
                            .frame(width: 8, height: 8)
                        Text("No Connection")
                            .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }

                    Spacer()

                    Button(action: { isConnectionManagerOpen = true }) {
                        Image(systemName: "plus")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    }
                    .buttonStyle(.plain)
                    .help("Add Connection")
                }

                ConnectionStatusIndicatorView(status: connectionManager.connectionStatus)

                // Search Filter inside Sidebar
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 10))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    TextField("Filter objects...", text: $objectFilterText)
                        .textFieldStyle(.plain)
                        .font(ThemeTokens.uiFont(size: 11))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                    if !objectFilterText.isEmpty {
                        Button(action: { objectFilterText = "" }) {
                            Image(systemName: "xmark.circle.fill")
                                .font(.system(size: 9))
                                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 6)
                .padding(.vertical, 4)
                .background(ThemeTokens.bgElevated(for: scheme))
                .cornerRadius(5)
                .overlay(
                    RoundedRectangle(cornerRadius: 5)
                        .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.8)
                )
            }
            .padding(10)
            .background(ThemeTokens.bgSidebar(for: scheme))

            Divider().background(ThemeTokens.borderColor(for: scheme))

            // Navigation Tree
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    if let driver = connectionManager.activeDriver {
                        let meta = driver.metadata
                        let filter = objectFilterText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()

                        // MongoDB Collections
                        if driver.supportsMongoDocuments {
                            let colls = meta.tables.filter { filter.isEmpty || $0.name.lowercased().contains(filter) }
                            sidebarSectionHeader(title: "COLLECTIONS (\(colls.count))", isExpanded: $isCollectionsExpanded, icon: "leaf.fill")
                            if isCollectionsExpanded {
                                ForEach(colls) { coll in
                                    collectionRow(coll)
                                }
                            }
                        } else {
                            // Relational Tables
                            let tables = meta.tables.filter { filter.isEmpty || $0.name.lowercased().contains(filter) }
                            sidebarSectionHeader(title: "TABLES (\(tables.count))", isExpanded: $isTablesExpanded, icon: "tablecells")
                            if isTablesExpanded {
                                ForEach(tables) { table in
                                    tableRow(table)
                                }
                            }

                            // Views
                            let views = meta.views.filter { filter.isEmpty || $0.name.lowercased().contains(filter) }
                            if !views.isEmpty {
                                sidebarSectionHeader(title: "VIEWS (\(views.count))", isExpanded: $isViewsExpanded, icon: "eye")
                                if isViewsExpanded {
                                    ForEach(views) { view in
                                        tableRow(view)
                                    }
                                }
                            }

                            // Stored Procedures
                            let procedures = meta.routines.filter { $0.isProcedure && (filter.isEmpty || $0.name.lowercased().contains(filter)) }
                            if !procedures.isEmpty {
                                sidebarSectionHeader(title: "PROCEDURES (\(procedures.count))", isExpanded: $isProceduresExpanded, icon: "gearshape.2.fill")
                                if isProceduresExpanded {
                                    ForEach(procedures) { p in
                                        procedureRow(p)
                                    }
                                }
                            }

                            // Functions
                            let functions = meta.routines.filter { $0.isFunction && (filter.isEmpty || $0.name.lowercased().contains(filter)) }
                            if !functions.isEmpty {
                                sidebarSectionHeader(title: "FUNCTIONS (\(functions.count))", isExpanded: $isFunctionsExpanded, icon: "function")
                                if isFunctionsExpanded {
                                    ForEach(functions) { f in
                                        functionRow(f)
                                    }
                                }
                            }

                            // Triggers
                            let triggers = meta.triggers.filter { filter.isEmpty || $0.name.lowercased().contains(filter) || $0.tableName.lowercased().contains(filter) }
                            if !triggers.isEmpty {
                                sidebarSectionHeader(title: "TRIGGERS (\(triggers.count))", isExpanded: $isTriggersExpanded, icon: "bolt.badge.clock")
                                if isTriggersExpanded {
                                    ForEach(triggers) { trg in
                                        triggerRow(trg)
                                    }
                                }
                            }

                            // Sequences
                            let sequences = meta.sequences.filter { filter.isEmpty || $0.name.lowercased().contains(filter) }
                            if !sequences.isEmpty {
                                sidebarSectionHeader(title: "SEQUENCES (\(sequences.count))", isExpanded: $isSequencesExpanded, icon: "number")
                                if isSequencesExpanded {
                                    ForEach(sequences) { s in
                                        sequenceRow(s)
                                    }
                                }
                            }
                        }
                    } else {
                        VStack(spacing: 8) {
                            Text("No active connection")
                                .font(ThemeTokens.uiFont(size: 11))
                                .foregroundColor(ThemeTokens.textMuted(for: scheme))

                            Button("Connect...") {
                                isConnectionManagerOpen = true
                            }
                            .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.top, 40)
                    }
                }
                .padding(.vertical, 6)
            }
            .background(ThemeTokens.bgSidebar(for: scheme))
        }
        .frame(minWidth: 200, idealWidth: 230, maxWidth: 280)
        .overlay(
            Rectangle()
                .frame(width: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .trailing
        )
    }

    private func sidebarSectionHeader(title: String, isExpanded: Binding<Bool>, icon: String) -> some View {
        Button(action: { withAnimation(.easeInOut(duration: 0.15)) { isExpanded.wrappedValue.toggle() } }) {
            HStack(spacing: 4) {
                Image(systemName: isExpanded.wrappedValue ? "chevron.down" : "chevron.right")
                    .font(.system(size: 8, weight: .bold))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    .frame(width: 10)

                Image(systemName: icon)
                    .font(.system(size: 9))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))

                Text(title)
                    .font(ThemeTokens.uiFont(size: 10, weight: .bold))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))

                Spacer()
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func tableRow(_ table: TableMetadata) -> some View {
        let isSelected = tabManager.activeTab?.title == table.name
        return HStack(spacing: 6) {
            Image(systemName: table.type.iconName)
                .font(.system(size: 11))
                .foregroundColor(isSelected ? ThemeTokens.accentBlue : ThemeTokens.textSecondary(for: scheme))
                .frame(width: 14)

            Text(table.name)
                .font(ThemeTokens.uiFont(size: 11.5, weight: isSelected ? .medium : .regular))
                .foregroundColor(isSelected ? ThemeTokens.textPrimary(for: scheme) : ThemeTokens.textSecondary(for: scheme))
                .lineLimit(1)

            Spacer()

            if let est = table.estimatedRows {
                Text("\(est)")
                    .font(ThemeTokens.codeFont(size: 9.5))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    .padding(.horizontal, 4)
                    .padding(.vertical, 1)
                    .background(ThemeTokens.bgElevated(for: scheme))
                    .cornerRadius(3)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 4)
        .background(isSelected ? ThemeTokens.accentBlue.opacity(0.12) : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture {
            tabManager.openTableDataTab(schema: table.schemaName, table: table.name)
        }
        .contextMenu {
            Button("Open Data") {
                tabManager.openTableDataTab(schema: table.schemaName, table: table.name)
            }
            Button("Inspect Structure") {
                tabManager.openTableStructureTab(schema: table.schemaName, table: table.name)
            }
            Button("Query Table") {
                tabManager.openQueryTab(initialSQL: "SELECT * FROM \"\(table.schemaName)\".\"\(table.name)\" LIMIT 100;")
            }
            Divider()
            Button("Copy Table Name") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(table.name, forType: .string)
            }
            Divider()
            Button("Truncate Table...") {
                if let driver = connectionManager.activeDriver {
                    Task { try? await driver.truncateTable(schema: table.schemaName, table: table.name) }
                }
            }
            Button("Drop Table...") {
                if let driver = connectionManager.activeDriver {
                    Task { try? await driver.dropTable(schema: table.schemaName, table: table.name) }
                }
            }
        }
    }

    private func collectionRow(_ coll: TableMetadata) -> some View {
        let isSelected = tabManager.activeTab?.title == coll.name
        return HStack(spacing: 6) {
            Image(systemName: "leaf.fill")
                .font(.system(size: 11))
                .foregroundColor(ThemeTokens.accentEmerald)
                .frame(width: 14)

            Text(coll.name)
                .font(ThemeTokens.uiFont(size: 11.5, weight: isSelected ? .medium : .regular))
                .foregroundColor(isSelected ? ThemeTokens.textPrimary(for: scheme) : ThemeTokens.textSecondary(for: scheme))
                .lineLimit(1)

            Spacer()

            if let est = coll.estimatedRows {
                Text("\(est)")
                    .font(ThemeTokens.codeFont(size: 9.5))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    .padding(.horizontal, 4)
                    .padding(.vertical, 1)
                    .background(ThemeTokens.bgElevated(for: scheme))
                    .cornerRadius(3)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 4)
        .background(isSelected ? ThemeTokens.accentBlue.opacity(0.12) : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture {
            tabManager.openMongoTab(collection: coll.name)
        }
        .contextMenu {
            Button("Open Documents") {
                tabManager.openMongoTab(collection: coll.name)
            }
            Button("Aggregation Pipeline") {
                tabManager.openMongoAggregationTab(collection: coll.name)
            }
            Divider()
            Button("Copy Collection Name") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(coll.name, forType: .string)
            }
        }
    }

    private func procedureRow(_ p: RoutineMetadata) -> some View {
        let isSelected = tabManager.activeTab?.title == p.name
        return HStack(spacing: 6) {
            Image(systemName: "gearshape.2.fill")
                .font(.system(size: 10))
                .foregroundColor(isSelected ? ThemeTokens.accentPurple : ThemeTokens.accentPurple.opacity(0.85))
                .frame(width: 14)

            Text(p.name)
                .font(ThemeTokens.uiFont(size: 11, weight: isSelected ? .medium : .regular))
                .foregroundColor(isSelected ? ThemeTokens.textPrimary(for: scheme) : ThemeTokens.textSecondary(for: scheme))
                .lineLimit(1)

            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 3.5)
        .background(isSelected ? ThemeTokens.accentPurple.opacity(0.12) : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture {
            tabManager.openRoutineTab(schema: p.schemaName, name: p.name, isProcedure: true)
        }
        .contextMenu {
            Button("Inspect Procedure") {
                tabManager.openRoutineTab(schema: p.schemaName, name: p.name, isProcedure: true)
            }
            Button("Call Procedure (SQL Editor)...") {
                tabManager.openQueryTab(initialSQL: p.callSyntaxTemplate)
            }
            Divider()
            Button("Copy Procedure Name") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(p.name, forType: .string)
            }
            Button("Copy CALL Template") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(p.callSyntaxTemplate, forType: .string)
            }
        }
    }

    private func functionRow(_ f: RoutineMetadata) -> some View {
        let isSelected = tabManager.activeTab?.title == f.name
        return HStack(spacing: 6) {
            Image(systemName: "function")
                .font(.system(size: 10))
                .foregroundColor(isSelected ? ThemeTokens.accentBlue : ThemeTokens.accentBlue.opacity(0.85))
                .frame(width: 14)

            Text(f.name)
                .font(ThemeTokens.uiFont(size: 11, weight: isSelected ? .medium : .regular))
                .foregroundColor(isSelected ? ThemeTokens.textPrimary(for: scheme) : ThemeTokens.textSecondary(for: scheme))
                .lineLimit(1)

            Spacer()

            if !f.returnType.isEmpty && f.returnType != "void" {
                Text(f.returnType)
                    .font(ThemeTokens.codeFont(size: 9))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    .padding(.horizontal, 3)
                    .padding(.vertical, 1)
                    .background(ThemeTokens.bgElevated(for: scheme))
                    .cornerRadius(3)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 3.5)
        .background(isSelected ? ThemeTokens.accentBlue.opacity(0.12) : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture {
            tabManager.openRoutineTab(schema: f.schemaName, name: f.name, isProcedure: false)
        }
        .contextMenu {
            Button("Inspect Function") {
                tabManager.openRoutineTab(schema: f.schemaName, name: f.name, isProcedure: false)
            }
            Button("Query Function (SQL Editor)...") {
                tabManager.openQueryTab(initialSQL: f.callSyntaxTemplate)
            }
            Divider()
            Button("Copy Function Name") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(f.name, forType: .string)
            }
        }
    }

    private func triggerRow(_ trg: TriggerMetadata) -> some View {
        HStack(spacing: 6) {
            Image(systemName: "bolt.badge.clock")
                .font(.system(size: 10))
                .foregroundColor(ThemeTokens.accentAmber)
                .frame(width: 14)

            Text(trg.name)
                .font(ThemeTokens.uiFont(size: 11))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .lineLimit(1)

            Spacer()

            Text(trg.tableName)
                .font(ThemeTokens.codeFont(size: 9))
                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                .padding(.horizontal, 3)
                .padding(.vertical, 1)
                .background(ThemeTokens.bgElevated(for: scheme))
                .cornerRadius(3)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 3)
        .contentShape(Rectangle())
        .onTapGesture {
            tabManager.openQueryTab(initialSQL: "-- Trigger: \(trg.name) ON \(trg.tableName)\n\(trg.definition)")
        }
        .contextMenu {
            Button("Inspect Trigger Definition") {
                tabManager.openQueryTab(initialSQL: "-- Trigger: \(trg.name) ON \(trg.tableName)\n\(trg.definition)")
            }
            Button("Copy Trigger Name") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(trg.name, forType: .string)
            }
        }
    }

    private func sequenceRow(_ s: SequenceMetadata) -> some View {
        HStack(spacing: 6) {
            Image(systemName: "number")
                .font(.system(size: 10))
                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                .frame(width: 14)

            Text(s.name)
                .font(ThemeTokens.uiFont(size: 11))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .lineLimit(1)

            Spacer()

            Text("\(s.currentValue)")
                .font(ThemeTokens.codeFont(size: 9.5))
                .foregroundColor(ThemeTokens.textMuted(for: scheme))
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 3)
    }
}
