import SwiftUI

public struct MongoWorkspaceView: View {
    public let collectionName: String

    @ObservedObject var connectionManager = ConnectionManager.shared
    @ObservedObject var tabManager = TabManager.shared

    @State private var queryResult: QueryResult = QueryResult()
    @State private var isLoading: Bool = false
    @State private var filterQuery: String = "{}"
    @State private var viewMode: Int = 0 // 0 = Documents Card / Tree, 1 = Raw JSON, 2 = Table View
    @State private var isInsertDocOpen: Bool = false
    @State private var newDocJSON: String = "{\n  \"name\": \"New Item\",\n  \"status\": \"active\",\n  \"score\": 100\n}"
    @State private var activeDocForEdit: DataRow? = nil
    @State private var editDocJSON: String = ""
    @Environment(\.colorScheme) var scheme

    private var collStats: MongoCollectionStats? {
        connectionManager.activeDriver?.metadata.mongoStats[collectionName]
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Collection Stats Bar
            statsHeaderBar

            // Filter Bar
            mongoFilterBar

            // Main Content depending on View Mode
            if isLoading && queryResult.rows.isEmpty {
                VStack(spacing: 12) {
                    ProgressView()
                    Text("Fetching documents from \(collectionName)...")
                        .font(ThemeTokens.uiFont(size: 12))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if queryResult.rows.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: "leaf")
                        .font(.system(size: 28))
                        .foregroundColor(ThemeTokens.accentEmerald)
                    Text("No documents in collection \(collectionName)")
                        .font(ThemeTokens.uiFont(size: 13, weight: .medium))
                    Button("Insert First Document") {
                        isInsertDocOpen = true
                    }
                    .buttonStyle(.borderedProminent)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                if viewMode == 0 {
                    documentsCardList
                } else if viewMode == 1 {
                    rawJSONList
                } else {
                    TableDataBrowserView(schema: "ecom_nosql", tableName: collectionName)
                }
            }
        }
        .background(ThemeTokens.bgPrimary(for: scheme))
        .onAppear {
            Task { await loadDocuments() }
        }
        .onChange(of: collectionName) { _, _ in
            Task { await loadDocuments() }
        }
        .sheet(isPresented: $isInsertDocOpen) {
            insertDocModal
        }
        .sheet(item: $activeDocForEdit) { row in
            editDocModal(row: row)
        }
    }

    private var statsHeaderBar: some View {
        HStack(spacing: 12) {
            HStack(spacing: 5) {
                Image(systemName: "leaf.fill")
                    .foregroundColor(ThemeTokens.accentEmerald)
                    .font(.system(size: 12))
                Text(collectionName)
                    .font(ThemeTokens.uiFont(size: 13, weight: .bold))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))
            }

            if let stats = collStats {
                HStack(spacing: 10) {
                    Text("•")
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    Text("\(stats.documentCount) documents")
                        .font(ThemeTokens.codeFont(size: 11))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))

                    Text("•")
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    Text("Storage: \(stats.totalStorageSizeBytes / 1024) KB")
                        .font(ThemeTokens.codeFont(size: 11))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))

                    Text("•")
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    Text("\(stats.indexCount) indexes")
                        .font(ThemeTokens.codeFont(size: 11))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                }
            }

            Spacer()

            // View Mode Switcher
            Picker("", selection: $viewMode) {
                Label("Cards", systemImage: "rectangle.grid.1x2").tag(0)
                Label("JSON", systemImage: "curlybraces").tag(1)
                Label("Table", systemImage: "tablecells").tag(2)
            }
            .pickerStyle(.segmented)
            .frame(width: 220)

            Button(action: { isInsertDocOpen = true }) {
                HStack(spacing: 4) {
                    Image(systemName: "plus")
                        .font(.system(size: 10, weight: .bold))
                    Text("Insert Document")
                        .font(ThemeTokens.uiFont(size: 11, weight: .semibold))
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(ThemeTokens.accentEmerald)
                .foregroundColor(.white)
                .cornerRadius(5)
            }
            .buttonStyle(.plain)

            Button(action: {
                tabManager.openMongoAggregationTab(collection: collectionName)
            }) {
                HStack(spacing: 4) {
                    Image(systemName: "arrow.triangle.merge")
                        .font(.system(size: 10))
                    Text("Aggregation")
                        .font(ThemeTokens.uiFont(size: 11))
                }
                .padding(.horizontal, 7)
                .padding(.vertical, 4)
                .background(ThemeTokens.bgSecondary(for: scheme))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .cornerRadius(5)
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(ThemeTokens.bgElevated(for: scheme))
        .overlay(
            Rectangle()
                .frame(height: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .bottom
        )
    }

    private var mongoFilterBar: some View {
        HStack(spacing: 8) {
            Text("Filter:")
                .font(ThemeTokens.codeBoldFont(size: 11))
                .foregroundColor(ThemeTokens.textMuted(for: scheme))

            TextField("e.g. { \"verified\": true, \"score\": { \"$gt\": 80 } }", text: $filterQuery, onCommit: {
                Task { await loadDocuments() }
            })
            .textFieldStyle(.plain)
            .font(ThemeTokens.codeFont(size: 11.5))
            .foregroundColor(ThemeTokens.textPrimary(for: scheme))
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(ThemeTokens.bgPrimary(for: scheme))
            .cornerRadius(5)
            .overlay(
                RoundedRectangle(cornerRadius: 5)
                    .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.8)
            )

            Button("Find") {
                Task { await loadDocuments() }
            }
            .buttonStyle(.borderedProminent)
            .font(ThemeTokens.uiFont(size: 11, weight: .semibold))
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(ThemeTokens.bgSecondary(for: scheme))
        .overlay(
            Rectangle()
                .frame(height: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .bottom
        )
    }

    private var documentsCardList: some View {
        ScrollView {
            LazyVStack(spacing: 10) {
                ForEach(queryResult.rows) { row in
                    let docId = row.values["_id"]?.displayText ?? "Unknown"
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Text("ObjectId(\"\(docId)\")")
                                .font(ThemeTokens.codeBoldFont(size: 11))
                                .foregroundColor(ThemeTokens.accentBlue)

                            Spacer()

                            Button("Edit Document") {
                                activeDocForEdit = row
                                editDocJSON = row.values["document"]?.rawStringValue ?? "{}"
                            }
                            .font(ThemeTokens.uiFont(size: 11))

                            Button(action: {
                                deleteDoc(docId: docId)
                            }) {
                                Image(systemName: "trash")
                                    .font(.system(size: 10))
                                    .foregroundColor(ThemeTokens.accentCrimson)
                            }
                            .buttonStyle(.plain)
                        }

                        // Preview Fields
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(Array(row.values.keys.sorted()), id: \.self) { key in
                                if key != "document" {
                                    HStack(spacing: 8) {
                                        Text("\"\(key)\":")
                                            .font(ThemeTokens.codeBoldFont(size: 11))
                                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                                            .frame(width: 90, alignment: .leading)

                                        Text(row.values[key]?.displayText ?? "null")
                                            .font(ThemeTokens.codeFont(size: 11))
                                            .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                                    }
                                }
                            }
                        }

                        // Document snippet
                        if let docJson = row.values["document"]?.rawStringValue {
                            Text(docJson)
                                .font(ThemeTokens.codeFont(size: 10))
                                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                .lineLimit(3)
                                .padding(6)
                                .background(ThemeTokens.bgPrimary(for: scheme))
                                .cornerRadius(4)
                        }
                    }
                    .padding(12)
                    .background(ThemeTokens.bgElevated(for: scheme))
                    .cornerRadius(8)
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.8)
                    )
                }
            }
            .padding(12)
        }
    }

    private var rawJSONList: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                ForEach(queryResult.rows) { row in
                    let json = row.values["document"]?.rawStringValue ?? "{}"
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Text("_id: \(row.values["_id"]?.displayText ?? "")")
                                .font(ThemeTokens.codeBoldFont(size: 11))
                                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                            Spacer()
                            Button("Edit JSON") {
                                activeDocForEdit = row
                                editDocJSON = json
                            }
                            .font(ThemeTokens.uiFont(size: 10.5))
                        }
                        Text(json)
                            .font(ThemeTokens.codeFont(size: 11))
                            .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                            .padding(8)
                            .background(ThemeTokens.bgPrimary(for: scheme))
                            .cornerRadius(6)
                    }
                    .padding(10)
                    .background(ThemeTokens.bgElevated(for: scheme))
                    .cornerRadius(8)
                }
            }
            .padding(12)
        }
    }

    private var insertDocModal: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: "plus.circle.fill")
                    .foregroundColor(ThemeTokens.accentEmerald)
                Text("Insert Document into \(collectionName)")
                    .font(ThemeTokens.uiFont(size: 13, weight: .bold))
                Spacer()
                Button(action: { isInsertDocOpen = false }) {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.plain)
            }

            Text("Enter document JSON with valid BSON fields:")
                .font(ThemeTokens.uiFont(size: 11))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))

            TextEditor(text: $newDocJSON)
                .font(ThemeTokens.codeFont(size: 12))
                .padding(8)
                .background(ThemeTokens.bgPrimary(for: scheme))
                .cornerRadius(6)
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 1)
                )

            HStack {
                Spacer()
                Button("Cancel") { isInsertDocOpen = false }
                Button("Insert Document") {
                    Task {
                        if let driver = connectionManager.activeDriver {
                            _ = try? await driver.insertMongoDocument(collection: collectionName, documentJSON: newDocJSON)
                            await loadDocuments()
                            ToastManager.shared.show("Document Inserted", style: .success)
                        }
                        isInsertDocOpen = false
                    }
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(16)
        .frame(width: 480, height: 380)
    }

    private func editDocModal(row: DataRow) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: "pencil")
                    .foregroundColor(ThemeTokens.accentBlue)
                Text("Edit Document: \(row.values["_id"]?.displayText ?? "")")
                    .font(ThemeTokens.uiFont(size: 13, weight: .bold))
                Spacer()
                Button(action: { activeDocForEdit = nil }) {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.plain)
            }

            TextEditor(text: $editDocJSON)
                .font(ThemeTokens.codeFont(size: 12))
                .padding(8)
                .background(ThemeTokens.bgPrimary(for: scheme))
                .cornerRadius(6)
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 1)
                )

            HStack {
                Spacer()
                Button("Cancel") { activeDocForEdit = nil }
                Button("Save (Instant Update)") {
                    let docId = row.values["_id"]?.displayText ?? ""
                    Task {
                        if let driver = connectionManager.activeDriver {
                            _ = try? await driver.updateMongoDocument(collection: collectionName, documentId: docId, newDocumentJSON: editDocJSON)
                            await loadDocuments()
                            ToastManager.shared.show("Document Updated", style: .success)
                        }
                        activeDocForEdit = nil
                    }
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(16)
        .frame(width: 500, height: 420)
    }

    private func loadDocuments() async {
        guard let driver = connectionManager.activeDriver else { return }
        isLoading = true
        do {
            let res = try await driver.fetchMongoDocuments(
                collection: collectionName,
                filterJSON: filterQuery,
                sortJSON: "{}",
                limit: 50,
                skip: 0
            )
            self.queryResult = res
            self.isLoading = false
        } catch {
            self.isLoading = false
            ToastManager.shared.show("Failed to load documents", subtitle: error.localizedDescription, style: .error)
        }
    }

    private func deleteDoc(docId: String) {
        guard let driver = connectionManager.activeDriver else { return }
        Task {
            try? await driver.deleteMongoDocument(collection: collectionName, documentId: docId)
            await loadDocuments()
            ToastManager.shared.show("Document Deleted", style: .warning)
        }
    }
}
