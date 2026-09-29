import Foundation

public enum ConnectionStatus: Equatable {
    case disconnected
    case connecting
    case connected
    case error(String)

    public var isConnected: Bool {
        if case .connected = self { return true }
        return false
    }

    public var isConnecting: Bool {
        if case .connecting = self { return true }
        return false
    }

    public var label: String {
        switch self {
        case .disconnected: return "Disconnected"
        case .connecting: return "Connecting..."
        case .connected: return "Connected"
        case .error(let msg): return "Error: \(msg)"
        }
    }
}

public protocol DatabaseDriver: AnyObject {
    var config: ConnectionConfig { get }
    var connectionStatus: ConnectionStatus { get }
    var metadata: DatabaseMetadata { get }

    func connect() async throws
    func disconnect() async throws
    func testConnection() async throws -> String

    // Metadata
    func fetchDatabases() async throws -> [String]
    func refreshMetadata(database: String) async throws -> DatabaseMetadata

    // Table Data Browser (paginated, sorted, filtered)
    func fetchTableRows(
        schema: String,
        table: String,
        limit: Int,
        offset: Int,
        sortColumn: String?,
        sortAscending: Bool,
        filterClause: String?
    ) async throws -> QueryResult

    // Safe Instant Cell Update
    func executeCellEdit(payload: CellEditPayload) async throws -> CellEditResult

    // Raw Query & Explain
    func executeQuery(sql: String, database: String) async throws -> QueryResult
    func explainQuery(sql: String, database: String, analyze: Bool) async throws -> ExplainPlanNode

    // Schema & DDL
    func generateTableDDL(schema: String, table: String) async throws -> String
    func dropTable(schema: String, table: String) async throws
    func truncateTable(schema: String, table: String) async throws
    func deleteRow(schema: String, table: String, primaryKeys: [String: DataValue]) async throws
    func insertRow(schema: String, table: String, values: [String: DataValue]) async throws -> DataRow

    // MongoDB operations
    var supportsMongoDocuments: Bool { get }
    func fetchMongoDocuments(collection: String, filterJSON: String, sortJSON: String, limit: Int, skip: Int) async throws -> QueryResult
    func updateMongoDocument(collection: String, documentId: String, newDocumentJSON: String) async throws -> CellEditResult
    func insertMongoDocument(collection: String, documentJSON: String) async throws -> String
    func deleteMongoDocument(collection: String, documentId: String) async throws
    func runMongoAggregation(collection: String, pipelineJSON: String) async throws -> QueryResult
}

// Default extension for drivers where Mongo methods are not applicable
public extension DatabaseDriver {
    var supportsMongoDocuments: Bool { false }

    func fetchMongoDocuments(collection: String, filterJSON: String, sortJSON: String, limit: Int, skip: Int) async throws -> QueryResult {
        throw NSError(domain: "DatabaseDriver", code: 400, userInfo: [NSLocalizedDescriptionKey: "MongoDB document operations not supported on relational SQL drivers."])
    }

    func updateMongoDocument(collection: String, documentId: String, newDocumentJSON: String) async throws -> CellEditResult {
        throw NSError(domain: "DatabaseDriver", code: 400, userInfo: [NSLocalizedDescriptionKey: "MongoDB document operations not supported on relational SQL drivers."])
    }

    func insertMongoDocument(collection: String, documentJSON: String) async throws -> String {
        throw NSError(domain: "DatabaseDriver", code: 400, userInfo: [NSLocalizedDescriptionKey: "MongoDB document operations not supported on relational SQL drivers."])
    }

    func deleteMongoDocument(collection: String, documentId: String) async throws {
        throw NSError(domain: "DatabaseDriver", code: 400, userInfo: [NSLocalizedDescriptionKey: "MongoDB document operations not supported on relational SQL drivers."])
    }

    func runMongoAggregation(collection: String, pipelineJSON: String) async throws -> QueryResult {
        throw NSError(domain: "DatabaseDriver", code: 400, userInfo: [NSLocalizedDescriptionKey: "MongoDB document operations not supported on relational SQL drivers."])
    }
}
