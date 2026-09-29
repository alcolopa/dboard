import Foundation
import Combine

public struct SavedQueryFolder: Identifiable, Codable, Equatable {
    public var id: String { name }
    public var name: String
    public var queries: [SavedQueryItem]

    public init(name: String, queries: [SavedQueryItem] = []) {
        self.name = name
        self.queries = queries
    }
}

public struct SavedQueryItem: Identifiable, Codable, Equatable {
    public var id: UUID
    public var title: String
    public var query: String
    public var folderName: String
    public var databaseType: DatabaseType
    public var createdAt: Date

    public init(
        id: UUID = UUID(),
        title: String,
        query: String,
        folderName: String = "Favorites",
        databaseType: DatabaseType = .postgresql,
        createdAt: Date = Date()
    ) {
        self.id = id
        self.title = title
        self.query = query
        self.folderName = folderName
        self.databaseType = databaseType
        self.createdAt = createdAt
    }
}

public struct QueryHistoryItem: Identifiable, Codable, Equatable {
    public var id: UUID
    public var query: String
    public var database: String
    public var connectionName: String
    public var timestamp: Date
    public var durationMs: Double
    public var isSuccess: Bool
    public var affectedRows: Int
    public var errorMessage: String?
    public var isFavorite: Bool

    public init(
        id: UUID = UUID(),
        query: String,
        database: String,
        connectionName: String,
        timestamp: Date = Date(),
        durationMs: Double,
        isSuccess: Bool,
        affectedRows: Int = 0,
        errorMessage: String? = nil,
        isFavorite: Bool = false
    ) {
        self.id = id
        self.query = query
        self.database = database
        self.connectionName = connectionName
        self.timestamp = timestamp
        self.durationMs = durationMs
        self.isSuccess = isSuccess
        self.affectedRows = affectedRows
        self.errorMessage = errorMessage
        self.isFavorite = isFavorite
    }
}

@MainActor
public final class QueryHistoryManager: ObservableObject {
    public static let shared = QueryHistoryManager()

    @Published public var history: [QueryHistoryItem] = []
    @Published public var folders: [SavedQueryFolder] = []
    @Published public var searchQuery: String = ""

    private init() {
        seedSampleSavedQueries()
    }

    private func seedSampleSavedQueries() {
        folders = [
            SavedQueryFolder(name: "Users", queries: [
                SavedQueryItem(title: "Active Verified Users", query: "SELECT id, email, name, balance\nFROM users\nWHERE status = 'active' AND is_verified = true\nORDER BY balance DESC;"),
                SavedQueryItem(title: "Recent User Registrations", query: "SELECT date_trunc('day', created_at) AS signup_day, count(*)\nFROM users\nGROUP BY 1\nORDER BY 1 DESC\nLIMIT 30;")
            ]),
            SavedQueryFolder(name: "Analytics", queries: [
                SavedQueryItem(title: "Daily Order Volume & GMV", query: "SELECT \n  date_trunc('day', placed_at) AS day,\n  count(*) AS total_orders,\n  sum(total_amount) AS gmv\nFROM orders\nWHERE status != 'cancelled'\nGROUP BY 1\nORDER BY 1 DESC;"),
                SavedQueryItem(title: "Top Products by Inventory Value", query: "SELECT sku, title, price, stock_quantity, (price * stock_quantity) AS inventory_value\nFROM products\nORDER BY inventory_value DESC;")
            ]),
            SavedQueryFolder(name: "Production", queries: [
                SavedQueryItem(title: "Table Cache Hit Ratios", query: "SELECT \n  relname,\n  heap_blks_read,\n  heap_blks_hit,\n  round((heap_blks_hit::numeric / nullif(heap_blks_hit + heap_blks_read, 0)) * 100, 2) AS ratio\nFROM pg_statio_user_tables\nORDER BY heap_blks_read DESC;"),
                SavedQueryItem(title: "Lock Wait & Blocking Queries", query: "SELECT pid, usename, pg_blocking_pids(pid) AS blocked_by, query, query_start\nFROM pg_stat_activity\nWHERE cardinality(pg_blocking_pids(pid)) > 0;")
            ]),
            SavedQueryFolder(name: "Debugging", queries: [
                SavedQueryItem(title: "Slow Queries (>50ms)", query: "SELECT query, calls, total_exec_time, mean_exec_time\nFROM pg_stat_statements\nORDER BY mean_exec_time DESC\nLIMIT 20;")
            ])
        ]

        history = []
    }

    public func recordExecution(
        query: String,
        database: String,
        connectionName: String,
        durationMs: Double,
        isSuccess: Bool,
        affectedRows: Int = 0,
        errorMessage: String? = nil
    ) {
        let item = QueryHistoryItem(
            query: query,
            database: database,
            connectionName: connectionName,
            durationMs: durationMs,
            isSuccess: isSuccess,
            affectedRows: affectedRows,
            errorMessage: errorMessage
        )
        history.insert(item, at: 0)
        if history.count > 500 {
            history.removeLast()
        }
    }

    public func saveQuery(title: String, query: String, folderName: String) {
        let item = SavedQueryItem(title: title, query: query, folderName: folderName)
        if let idx = folders.firstIndex(where: { $0.name == folderName }) {
            folders[idx].queries.append(item)
        } else {
            folders.append(SavedQueryFolder(name: folderName, queries: [item]))
        }
    }

    public func toggleFavorite(id: UUID) {
        if let idx = history.firstIndex(where: { $0.id == id }) {
            history[idx].isFavorite.toggle()
        }
    }

    public func clearHistory() {
        history.removeAll()
    }
}
