import Foundation

public final class MongoDBDriver: DatabaseDriver {
    public let config: ConnectionConfig
    public private(set) var connectionStatus: ConnectionStatus = .disconnected
    public private(set) var metadata: DatabaseMetadata

    public var supportsMongoDocuments: Bool { true }
    private var collectionStore: [String: [DataRow]] = [:]

    public init(config: ConnectionConfig) {
        self.config = config
        let sample = MockDatabaseGenerator.makeMongoSample()
        self.metadata = sample.0
        self.collectionStore = sample.1
    }

    public func connect() async throws {
        connectionStatus = .connecting
        try await Task.sleep(nanoseconds: 220_000_000)
        connectionStatus = .connected
        await ActivityLogger.shared.log(
            statement: "// Connected to MongoDB cluster: \(config.databaseName) [URI: \(config.displayURI)]",
            database: config.databaseName,
            durationMs: 22.0,
            isSuccess: true
        )
    }

    public func disconnect() async throws {
        connectionStatus = .disconnected
    }

    public func testConnection() async throws -> String {
        try await Task.sleep(nanoseconds: 150_000_000)
        return "MongoDB v7.0.5 (wire version 21, OpenSSL 3.0.2)"
    }

    public func fetchDatabases() async throws -> [String] {
        return ["admin", "config", "local", "ecom_nosql"]
    }

    public func refreshMetadata(database: String) async throws -> DatabaseMetadata {
        return metadata
    }

    public func fetchTableRows(
        schema: String,
        table: String,
        limit: Int,
        offset: Int,
        sortColumn: String?,
        sortAscending: Bool,
        filterClause: String?
    ) async throws -> QueryResult {
        return try await fetchMongoDocuments(
            collection: table,
            filterJSON: filterClause ?? "{}",
            sortJSON: "{}",
            limit: limit,
            skip: offset
        )
    }

    public func executeCellEdit(payload: CellEditPayload) async throws -> CellEditResult {
        let startTime = CFAbsoluteTimeGetCurrent()
        guard let docId = payload.primaryKeys["_id"]?.displayText else {
            throw NSError(domain: "MongoDBDriver", code: 400, userInfo: [NSLocalizedDescriptionKey: "MongoDB document missing _id field"])
        }

        let mongoCmd = "db.\(payload.tableName).updateOne({ _id: ObjectId(\"\(docId)\") }, { $set: { \"\(payload.columnName)\": \(payload.newValue.displayText) } });"
        let forwardOp = GeneratedDatabaseOperation(
            statement: mongoCmd,
            displaySQL: mongoCmd,
            isReversible: true,
            targetTable: payload.tableName,
            targetSchema: payload.schema
        )

        let reverseCmd = "db.\(payload.tableName).updateOne({ _id: ObjectId(\"\(docId)\") }, { $set: { \"\(payload.columnName)\": \(payload.oldValue.displayText) } });"
        let reverseOp = GeneratedDatabaseOperation(
            statement: reverseCmd,
            displaySQL: reverseCmd,
            isReversible: true,
            targetTable: payload.tableName,
            targetSchema: payload.schema
        )

        try await Task.sleep(nanoseconds: 15_000_000)

        if var rows = collectionStore[payload.tableName] {
            if let idx = rows.firstIndex(where: { $0.values["_id"] == payload.primaryKeys["_id"] }) {
                rows[idx].values[payload.columnName] = payload.newValue
                collectionStore[payload.tableName] = rows
            }
        }

        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

        await ActivityLogger.shared.log(
            statement: mongoCmd,
            database: config.databaseName,
            durationMs: duration,
            affectedRows: 1,
            isSuccess: true
        )

        await EditHistoryManager.shared.recordCellUpdate(
            payload: payload,
            forwardOp: forwardOp,
            reverseOp: reverseOp
        )

        return CellEditResult(
            success: true,
            affectedRows: 1,
            durationMs: duration,
            operation: forwardOp,
            userFriendlyMessage: "Document '\(docId)' updated successfully."
        )
    }

    public func executeQuery(sql: String, database: String) async throws -> QueryResult {
        // Evaluate as Mongo shell command (e.g., db.collection.find(...))
        let startTime = CFAbsoluteTimeGetCurrent()
        try await Task.sleep(nanoseconds: 30_000_000)

        for (collName, rows) in collectionStore {
            if sql.contains(collName) {
                let cols = metadata.table(named: collName, schema: "ecom_nosql")?.columns ?? []
                let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000
                await ActivityLogger.shared.log(statement: sql, database: database, durationMs: duration, affectedRows: rows.count, isSuccess: true)
                return QueryResult(columns: cols, rows: rows, executionDurationMs: duration, affectedRows: rows.count, totalRowCount: rows.count, sqlStatement: sql)
            }
        }

        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000
        await ActivityLogger.shared.log(statement: sql, database: database, durationMs: duration, affectedRows: 1, isSuccess: true)
        return QueryResult(
            columns: [ColumnDefinition(name: "acknowledged", ordinalPosition: 1, dataTypeName: "Boolean")],
            rows: [DataRow(values: ["acknowledged": .boolean(true)])],
            executionDurationMs: duration,
            affectedRows: 1,
            totalRowCount: 1,
            sqlStatement: sql
        )
    }

    public func explainQuery(sql: String, database: String, analyze: Bool) async throws -> ExplainPlanNode {
        return ExplainPlanNode(
            nodeType: "COLLSCAN (Collection Scan)",
            relationName: "user_profiles",
            startupCost: 0.0,
            totalCost: 1.0,
            planRows: 3,
            actualStartupTime: 0.002,
            actualTotalTime: 0.009,
            actualRows: 3,
            filter: "{ score: { $gt: 80 } }"
        )
    }

    public func generateTableDDL(schema: String, table: String) async throws -> String {
        return """
        // MongoDB Collection Creation Script
        db.createCollection("\(table)", {
            validator: {
                $jsonSchema: {
                    bsonType: "object",
                    required: ["_id", "username", "email"],
                    properties: {
                        _id: { bsonType: "objectId" },
                        username: { bsonType: "string" },
                        email: { bsonType: "string" }
                    }
                }
            }
        });
        db.\(table).createIndex({ email: 1 }, { unique: true });
        """
    }

    public func dropTable(schema: String, table: String) async throws {
        collectionStore.removeValue(forKey: table)
        metadata.tables.removeAll { $0.name == table }
        await ActivityLogger.shared.log(statement: "db.\(table).drop();", database: config.databaseName, durationMs: 14.0, isSuccess: true)
    }

    public func truncateTable(schema: String, table: String) async throws {
        collectionStore[table] = []
        await ActivityLogger.shared.log(statement: "db.\(table).deleteMany({});", database: config.databaseName, durationMs: 8.0, isSuccess: true)
    }

    public func deleteRow(schema: String, table: String, primaryKeys: [String: DataValue]) async throws {
        if var rows = collectionStore[table] {
            rows.removeAll { row in
                primaryKeys.allSatisfy { row.values[$0.key] == $0.value }
            }
            collectionStore[table] = rows
        }
        let idStr = primaryKeys["_id"]?.displayText ?? ""
        await ActivityLogger.shared.log(statement: "db.\(table).deleteOne({ _id: ObjectId(\"\(idStr)\") });", database: config.databaseName, durationMs: 8.5, affectedRows: 1, isSuccess: true)
    }

    public func insertRow(schema: String, table: String, values: [String: DataValue]) async throws -> DataRow {
        let newRow = DataRow(values: values)
        var rows = collectionStore[table] ?? []
        rows.append(newRow)
        collectionStore[table] = rows
        await ActivityLogger.shared.log(statement: "db.\(table).insertOne({ ... });", database: config.databaseName, durationMs: 11.0, affectedRows: 1, isSuccess: true)
        return newRow
    }

    // MARK: - Native MongoDB Operations

    public func fetchMongoDocuments(
        collection: String,
        filterJSON: String,
        sortJSON: String,
        limit: Int,
        skip: Int
    ) async throws -> QueryResult {
        let startTime = CFAbsoluteTimeGetCurrent()
        guard let allRows = collectionStore[collection] else {
            return QueryResult(columns: [], rows: [], sqlStatement: "db.\(collection).find()")
        }

        var filtered = allRows
        let cleanFilter = filterJSON.trimmingCharacters(in: .whitespacesAndNewlines)
        if cleanFilter != "{}" && !cleanFilter.isEmpty {
            let lower = cleanFilter.lowercased()
            filtered = filtered.filter { row in
                row.values.values.contains { $0.displayText.lowercased().contains(lower) }
            }
        }

        let total = filtered.count
        let start = min(skip, total)
        let end = min(start + limit, total)
        let page = Array(filtered[start..<end])

        let cols = metadata.table(named: collection, schema: "ecom_nosql")?.columns ?? []
        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

        let statement = "db.\(collection).find(\(filterJSON)).sort(\(sortJSON)).skip(\(skip)).limit(\(limit));"

        return QueryResult(
            columns: cols,
            rows: page,
            executionDurationMs: duration,
            affectedRows: page.count,
            totalRowCount: total,
            sqlStatement: statement
        )
    }

    public func updateMongoDocument(collection: String, documentId: String, newDocumentJSON: String) async throws -> CellEditResult {
        let startTime = CFAbsoluteTimeGetCurrent()

        // Validate JSON
        guard let data = newDocumentJSON.data(using: .utf8),
              (try? JSONSerialization.jsonObject(with: data)) != nil else {
            throw NSError(domain: "MongoDBDriver", code: 400, userInfo: [NSLocalizedDescriptionKey: "Invalid JSON document format. Please check syntax."])
        }

        let cmd = "db.\(collection).replaceOne({ _id: ObjectId(\"\(documentId)\") }, \(newDocumentJSON));"
        let op = GeneratedDatabaseOperation(statement: cmd, displaySQL: cmd, isReversible: true, targetTable: collection)

        if var rows = collectionStore[collection] {
            if let idx = rows.firstIndex(where: { $0.values["_id"]?.displayText == documentId }) {
                rows[idx].values["document"] = .json(newDocumentJSON)
                collectionStore[collection] = rows
            }
        }

        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000
        await ActivityLogger.shared.log(statement: cmd, database: config.databaseName, durationMs: duration, affectedRows: 1, isSuccess: true)

        return CellEditResult(
            success: true,
            affectedRows: 1,
            durationMs: duration,
            operation: op,
            userFriendlyMessage: "Document \(documentId) updated."
        )
    }

    public func insertMongoDocument(collection: String, documentJSON: String) async throws -> String {
        guard let data = documentJSON.data(using: .utf8),
              (try? JSONSerialization.jsonObject(with: data)) != nil else {
            throw NSError(domain: "MongoDBDriver", code: 400, userInfo: [NSLocalizedDescriptionKey: "Invalid JSON syntax."])
        }

        let newId = UUID().uuidString.prefix(24).lowercased()
        let newRow = DataRow(values: [
            "_id": .objectId(newId),
            "username": .string("new_user_\(newId.prefix(4))"),
            "email": .string("user@example.com"),
            "score": .double(10.0),
            "verified": .boolean(true),
            "document": .json(documentJSON)
        ])

        var rows = collectionStore[collection] ?? []
        rows.insert(newRow, at: 0)
        collectionStore[collection] = rows

        let cmd = "db.\(collection).insertOne(\(documentJSON));"
        await ActivityLogger.shared.log(statement: cmd, database: config.databaseName, durationMs: 12.0, affectedRows: 1, isSuccess: true)
        return newId
    }

    public func deleteMongoDocument(collection: String, documentId: String) async throws {
        if var rows = collectionStore[collection] {
            rows.removeAll { $0.values["_id"]?.displayText == documentId }
            collectionStore[collection] = rows
        }
        let cmd = "db.\(collection).deleteOne({ _id: ObjectId(\"\(documentId)\") });"
        await ActivityLogger.shared.log(statement: cmd, database: config.databaseName, durationMs: 9.0, affectedRows: 1, isSuccess: true)
    }

    public func runMongoAggregation(collection: String, pipelineJSON: String) async throws -> QueryResult {
        let startTime = CFAbsoluteTimeGetCurrent()
        try await Task.sleep(nanoseconds: 30_000_000)

        guard let allRows = collectionStore[collection] else {
            return QueryResult()
        }

        let cmd = "db.\(collection).aggregate(\(pipelineJSON));"
        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000
        await ActivityLogger.shared.log(statement: cmd, database: config.databaseName, durationMs: duration, affectedRows: allRows.count, isSuccess: true)

        let cols = [
            ColumnDefinition(name: "_id", ordinalPosition: 1, dataTypeName: "String"),
            ColumnDefinition(name: "count", ordinalPosition: 2, dataTypeName: "Int"),
            ColumnDefinition(name: "avgScore", ordinalPosition: 3, dataTypeName: "Double")
        ]

        let aggRows = [
            DataRow(values: ["_id": .string("verified_users"), "count": .integer(2), "avgScore": .double(92.8)]),
            DataRow(values: ["_id": .string("unverified_users"), "count": .integer(1), "avgScore": .double(45.0)])
        ]

        return QueryResult(
            columns: cols,
            rows: aggRows,
            executionDurationMs: duration,
            affectedRows: 2,
            totalRowCount: 2,
            sqlStatement: cmd
        )
    }
}
