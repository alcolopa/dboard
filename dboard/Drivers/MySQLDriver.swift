import Foundation

public final class MySQLDriver: DatabaseDriver {
    public let config: ConnectionConfig
    public private(set) var connectionStatus: ConnectionStatus = .disconnected
    public private(set) var metadata: DatabaseMetadata

    private var tableDataStore: [String: [DataRow]] = [:]
    public var isDemoData: Bool { true }

    public init(config: ConnectionConfig) {
        self.config = config
        let sample = MockDatabaseGenerator.makeMySQLSample()
        self.metadata = sample.0
        self.tableDataStore = sample.1
    }

    public func connect() async throws {
        connectionStatus = .connecting
        try await Task.sleep(nanoseconds: 200_000_000)
        connectionStatus = .connected
        await ActivityLogger.shared.log(
            statement: "-- Connected to MySQL 8.0 server: \(config.databaseName) on \(config.host):\(config.port)",
            database: config.databaseName,
            durationMs: 18.2,
            isSuccess: true
        )
    }

    public func disconnect() async throws {
        connectionStatus = .disconnected
    }

    public func testConnection() async throws -> String {
        try await Task.sleep(nanoseconds: 180_000_000)
        return "8.0.36-MySQL Community Server - GPL"
    }

    public func fetchDatabases() async throws -> [String] {
        return ["information_schema", "mysql", "performance_schema", "sys", "shop_staging"]
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
        let startTime = CFAbsoluteTimeGetCurrent()

        if table == "audit_events_10m" {
            let total = 10_000_000
            let page = MockDatabaseGenerator.generateAuditEvents(offset: offset, limit: limit, totalRows: total)
            let cols = metadata.table(named: table, schema: schema)?.columns ?? []
            let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

            var sql = "SELECT * FROM `\(table)`"
            if let f = filterClause, !f.isEmpty { sql += " WHERE \(f)" }
            if let s = sortColumn { sql += " ORDER BY `\(s)` \(sortAscending ? "ASC" : "DESC")" }
            sql += " LIMIT \(offset), \(limit);"

            return QueryResult(
                columns: cols,
                rows: page,
                executionDurationMs: duration,
                affectedRows: page.count,
                totalRowCount: total,
                sqlStatement: sql
            )
        }

        guard let allRows = tableDataStore[table] else {
            return QueryResult(columns: [], rows: [], sqlStatement: "SELECT * FROM `\(table)`")
        }

        var filtered = allRows
        if let filter = filterClause, !filter.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let lower = filter.lowercased()
            filtered = filtered.filter { row in
                row.values.values.contains { $0.displayText.lowercased().contains(lower) }
            }
        }

        if let sortCol = sortColumn {
            filtered.sort { r1, r2 in
                let v1 = r1[sortCol].displayText
                let v2 = r2[sortCol].displayText
                return sortAscending ? v1 < v2 : v1 > v2
            }
        }

        let total = filtered.count
        let start = min(offset, total)
        let end = min(start + limit, total)
        let page = Array(filtered[start..<end])

        let cols = metadata.table(named: table, schema: schema)?.columns ?? []
        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

        var sql = "SELECT * FROM `\(table)`"
        if let f = filterClause, !f.isEmpty { sql += " WHERE \(f)" }
        if let s = sortColumn { sql += " ORDER BY `\(s)` \(sortAscending ? "ASC" : "DESC")" }
        sql += " LIMIT \(offset), \(limit);"

        return QueryResult(
            columns: cols,
            rows: page,
            executionDurationMs: duration,
            affectedRows: page.count,
            totalRowCount: total,
            sqlStatement: sql
        )
    }

    public func executeCellEdit(payload: CellEditPayload) async throws -> CellEditResult {
        let startTime = CFAbsoluteTimeGetCurrent()

        guard payload.canBeSafelyExecuted else {
            let err = "Cannot update row: MySQL table '\(payload.tableName)' has no primary key."
            await ActivityLogger.shared.log(
                statement: "-- [REJECTED] Unsafe MySQL update without primary key",
                database: config.databaseName,
                durationMs: 0.0,
                isSuccess: false,
                rawError: err
            )
            throw NSError(domain: "MySQLDriver", code: 400, userInfo: [NSLocalizedDescriptionKey: err])
        }

        let setClause = "`\(payload.columnName)` = ?"
        var whereClauses: [String] = []
        var queryParams: [DataValue] = [payload.newValue]

        for (pkCol, pkVal) in payload.primaryKeys.sorted(by: { $0.key < $1.key }) {
            whereClauses.append("`\(pkCol)` = ?")
            queryParams.append(pkVal)
        }

        let sql = "UPDATE `\(payload.tableName)` SET \(setClause) WHERE \(whereClauses.joined(separator: " AND "));"
        let displaySQL = "UPDATE `\(payload.tableName)` SET `\(payload.columnName)` = \(payload.newValue.sqlLiteral) WHERE \(payload.primaryKeys.map { "`\($0.key)` = \($0.value.sqlLiteral)" }.joined(separator: " AND "));"

        let forwardOp = GeneratedDatabaseOperation(
            statement: sql,
            parameters: queryParams,
            displaySQL: displaySQL,
            isReversible: true,
            targetTable: payload.tableName,
            targetSchema: payload.schema
        )

        let reverseParams: [DataValue] = [payload.oldValue] + queryParams.dropFirst()
        let reverseDisplaySQL = "UPDATE `\(payload.tableName)` SET `\(payload.columnName)` = \(payload.oldValue.sqlLiteral) WHERE \(payload.primaryKeys.map { "`\($0.key)` = \($0.value.sqlLiteral)" }.joined(separator: " AND "));"
        let reverseOp = GeneratedDatabaseOperation(
            statement: sql,
            parameters: reverseParams,
            displaySQL: reverseDisplaySQL,
            isReversible: true,
            targetTable: payload.tableName,
            targetSchema: payload.schema
        )

        try await Task.sleep(nanoseconds: 20_000_000)

        if var rows = tableDataStore[payload.tableName] {
            if let rowIndex = rows.firstIndex(where: { row in
                payload.primaryKeys.allSatisfy { row.values[$0.key] == $0.value }
            }) {
                rows[rowIndex].values[payload.columnName] = payload.newValue
                tableDataStore[payload.tableName] = rows
            }
        }

        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

        await ActivityLogger.shared.log(
            statement: displaySQL,
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
            userFriendlyMessage: "Updated `\(payload.columnName)` successfully."
        )
    }

    public func executeQuery(sql: String, database: String) async throws -> QueryResult {
        let startTime = CFAbsoluteTimeGetCurrent()
        try await Task.sleep(nanoseconds: 35_000_000)

        let upper = sql.trimmingCharacters(in: .whitespacesAndNewlines).uppercased()

        if upper.hasPrefix("EXPLAIN") {
            let plan = try await explainQuery(sql: sql, database: database, analyze: false)
            let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000
            await ActivityLogger.shared.log(statement: sql, database: database, durationMs: duration, isSuccess: true)
            return QueryResult(executionDurationMs: duration, sqlStatement: sql, explainPlan: plan)
        }

        if upper.hasPrefix("SELECT") {
            for (tblName, rows) in tableDataStore {
                if upper.contains(tblName.uppercased()) {
                    let cols = metadata.table(named: tblName, schema: "shop_staging")?.columns ?? []
                    let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000
                    await ActivityLogger.shared.log(statement: sql, database: database, durationMs: duration, affectedRows: rows.count, isSuccess: true)
                    return QueryResult(columns: cols, rows: rows, executionDurationMs: duration, affectedRows: rows.count, totalRowCount: rows.count, sqlStatement: sql)
                }
            }
        }

        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000
        await ActivityLogger.shared.log(statement: sql, database: database, durationMs: duration, affectedRows: 1, isSuccess: true)
        return QueryResult(
            columns: [ColumnDefinition(name: "result", ordinalPosition: 1, dataTypeName: "varchar(255)")],
            rows: [DataRow(values: ["result": .string("MySQL statement executed successfully")])],
            executionDurationMs: duration,
            affectedRows: 1,
            totalRowCount: 1,
            sqlStatement: sql
        )
    }

    public func explainQuery(sql: String, database: String, analyze: Bool) async throws -> ExplainPlanNode {
        return ExplainPlanNode(
            nodeType: "Index Lookup (eq_ref)",
            relationName: "customers",
            startupCost: 1.0,
            totalCost: 1.0,
            planRows: 1,
            actualStartupTime: 0.008,
            actualTotalTime: 0.012,
            actualRows: 1,
            filter: "customer_id = 101",
            indexName: "PRIMARY"
        )
    }

    public func generateTableDDL(schema: String, table: String) async throws -> String {
        guard let meta = metadata.table(named: table, schema: schema) else {
            return "-- Table `\(table)` not found"
        }

        var lines: [String] = []
        lines.append("CREATE TABLE `\(table)` (")

        var colDefs: [String] = []
        for col in meta.columns {
            var colDef = "  `\(col.name)` \(col.dataTypeName.uppercased())"
            if !col.isNullable { colDef += " NOT NULL" }
            if let def = col.defaultValue { colDef += " DEFAULT \(def)" }
            colDefs.append(colDef)
        }

        if !meta.primaryKeyColumnNames.isEmpty {
            let pks = meta.primaryKeyColumnNames.map { "`\($0)`" }.joined(separator: ", ")
            colDefs.append("  PRIMARY KEY (\(pks))")
        }

        lines.append(colDefs.joined(separator: ",\n"))
        lines.append(") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;")
        return lines.joined(separator: "\n")
    }

    public func dropTable(schema: String, table: String) async throws {
        tableDataStore.removeValue(forKey: table)
        metadata.tables.removeAll { $0.name == table }
        await ActivityLogger.shared.log(statement: "DROP TABLE `\(table)`;", database: config.databaseName, durationMs: 11.0, isSuccess: true)
    }

    public func truncateTable(schema: String, table: String) async throws {
        tableDataStore[table] = []
        await ActivityLogger.shared.log(statement: "TRUNCATE TABLE `\(table)`;", database: config.databaseName, durationMs: 9.0, isSuccess: true)
    }

    public func deleteRow(schema: String, table: String, primaryKeys: [String: DataValue]) async throws {
        if var rows = tableDataStore[table] {
            rows.removeAll { row in
                primaryKeys.allSatisfy { row.values[$0.key] == $0.value }
            }
            tableDataStore[table] = rows
        }
        let whereStr = primaryKeys.map { "`\($0.key)` = \($0.value.sqlLiteral)" }.joined(separator: " AND ")
        await ActivityLogger.shared.log(statement: "DELETE FROM `\(table)` WHERE \(whereStr);", database: config.databaseName, durationMs: 9.8, affectedRows: 1, isSuccess: true)
    }

    public func insertRow(schema: String, table: String, values: [String: DataValue]) async throws -> DataRow {
        let newRow = DataRow(values: values)
        var rows = tableDataStore[table] ?? []
        rows.append(newRow)
        tableDataStore[table] = rows

        let cols = values.keys.map { "`\($0)`" }.joined(separator: ", ")
        let vals = values.values.map { $0.sqlLiteral }.joined(separator: ", ")
        await ActivityLogger.shared.log(statement: "INSERT INTO `\(table)` (\(cols)) VALUES (\(vals));", database: config.databaseName, durationMs: 12.0, affectedRows: 1, isSuccess: true)
        return newRow
    }
}
