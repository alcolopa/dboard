import Foundation

public enum CellEditStatus: Equatable {
    case idle
    case saving
    case saved
    case error(message: String)

    public var isSaving: Bool {
        if case .saving = self { return true }
        return false
    }

    public var isSaved: Bool {
        if case .saved = self { return true }
        return false
    }

    public var errorMessage: String? {
        if case .error(let msg) = self { return msg }
        return nil
    }
}

public struct CellEditPayload: Identifiable, Equatable {
    public let id: UUID
    public var rowId: UUID
    public var schema: String
    public var tableName: String
    public var columnName: String
    public var oldValue: DataValue
    public var newValue: DataValue
    public var primaryKeys: [String: DataValue]
    public var originalRowSnapshot: [String: DataValue]?
    public var timestamp: Date

    public init(
        id: UUID = UUID(),
        rowId: UUID,
        schema: String = "public",
        tableName: String,
        columnName: String,
        oldValue: DataValue,
        newValue: DataValue,
        primaryKeys: [String: DataValue],
        originalRowSnapshot: [String: DataValue]? = nil,
        timestamp: Date = Date()
    ) {
        self.id = id
        self.rowId = rowId
        self.schema = schema
        self.tableName = tableName
        self.columnName = columnName
        self.oldValue = oldValue
        self.newValue = newValue
        self.primaryKeys = primaryKeys
        self.originalRowSnapshot = originalRowSnapshot
        self.timestamp = timestamp
    }

    public var canBeSafelyExecuted: Bool {
        !primaryKeys.isEmpty
    }
}

public struct GeneratedDatabaseOperation: Identifiable, Equatable {
    public let id: UUID = UUID()
    public var statement: String
    public var parameters: [DataValue]
    public var displaySQL: String
    public var isReversible: Bool
    public var targetTable: String
    public var targetSchema: String

    public init(
        statement: String,
        parameters: [DataValue] = [],
        displaySQL: String,
        isReversible: Bool = true,
        targetTable: String = "",
        targetSchema: String = "public"
    ) {
        self.statement = statement
        self.parameters = parameters
        self.displaySQL = displaySQL
        self.isReversible = isReversible
        self.targetTable = targetTable
        self.targetSchema = targetSchema
    }
}

public struct CellEditResult: Equatable {
    public var success: Bool
    public var affectedRows: Int
    public var durationMs: Double
    public var operation: GeneratedDatabaseOperation
    public var userFriendlyMessage: String
    public var rawDatabaseError: String?

    public init(
        success: Bool,
        affectedRows: Int = 0,
        durationMs: Double = 0.0,
        operation: GeneratedDatabaseOperation,
        userFriendlyMessage: String = "",
        rawDatabaseError: String? = nil
    ) {
        self.success = success
        self.affectedRows = affectedRows
        self.durationMs = durationMs
        self.operation = operation
        self.userFriendlyMessage = userFriendlyMessage
        self.rawDatabaseError = rawDatabaseError
    }
}
