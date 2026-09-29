import Foundation
import Combine

public struct ActivityLogEntry: Identifiable, Equatable {
    public let id: UUID
    public var timestamp: Date
    public var statement: String
    public var database: String
    public var durationMs: Double
    public var affectedRows: Int
    public var isSuccess: Bool
    public var userFriendlyMessage: String?
    public var rawError: String?

    public init(
        id: UUID = UUID(),
        timestamp: Date = Date(),
        statement: String,
        database: String = "",
        durationMs: Double = 0.0,
        affectedRows: Int = 0,
        isSuccess: Bool = true,
        userFriendlyMessage: String? = nil,
        rawError: String? = nil
    ) {
        self.id = id
        self.timestamp = timestamp
        self.statement = statement
        self.database = database
        self.durationMs = durationMs
        self.affectedRows = affectedRows
        self.isSuccess = isSuccess
        self.userFriendlyMessage = userFriendlyMessage
        self.rawError = rawError
    }
}

@MainActor
public final class ActivityLogger: ObservableObject {
    public static let shared = ActivityLogger()

    @Published public private(set) var entries: [ActivityLogEntry] = []

    public init() {}

    public func log(
        statement: String,
        database: String = "",
        durationMs: Double,
        affectedRows: Int = 0,
        isSuccess: Bool,
        rawError: String? = nil
    ) {
        let friendly = rawError.map { ActivityLogger.humanizeError($0) }
        let entry = ActivityLogEntry(
            statement: statement,
            database: database,
            durationMs: durationMs,
            affectedRows: affectedRows,
            isSuccess: isSuccess,
            userFriendlyMessage: friendly,
            rawError: rawError
        )
        entries.insert(entry, at: 0)
        if entries.count > 500 {
            entries.removeLast()
        }
    }

    public func clear() {
        entries.removeAll()
    }

    public static func humanizeError(_ raw: String) -> String {
        let lower = raw.lowercased()

        // PostgreSQL error codes & messages
        if raw.contains("23505") || lower.contains("unique constraint") || lower.contains("duplicate key") {
            return "Could not save changes: A record with this unique value already exists in the table."
        }
        if raw.contains("23503") || lower.contains("foreign key constraint") {
            return "Foreign key constraint failed: The referenced record does not exist or is being used."
        }
        if raw.contains("23502") || lower.contains("not-null constraint") {
            return "Not-null violation: This column cannot be set to NULL."
        }
        if raw.contains("22P02") || lower.contains("invalid input syntax") {
            return "Data type mismatch: The entered value is not valid for this column's type."
        }
        if raw.contains("42P01") || lower.contains("relation does not exist") {
            return "Table not found: The specified table or view does not exist in the active schema."
        }

        // MySQL error codes
        if raw.contains("1062") {
            return "Duplicate entry error: Value violates unique key constraint."
        }
        if raw.contains("1451") || raw.contains("1452") {
            return "Cannot modify row: Foreign key constraint fails."
        }
        if raw.contains("1048") {
            return "Column cannot be null."
        }

        // MongoDB error codes
        if raw.contains("E11000") {
            return "MongoDB duplicate key error: An index constraint was violated."
        }

        return raw
    }
}
