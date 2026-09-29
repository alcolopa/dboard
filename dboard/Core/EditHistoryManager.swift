import Foundation
import Combine

public enum DatabaseOperationKind: String, Codable, CaseIterable {
    case cellUpdate = "Cell Update"
    case rowInsert = "Row Insert"
    case rowDelete = "Row Delete"
    case ddlStatement = "Schema DDL"
    case customSQL = "Custom Query"
}

public struct EditHistoryEntry: Identifiable, Equatable {
    public let id: UUID
    public var timestamp: Date
    public var kind: DatabaseOperationKind
    public var schema: String
    public var table: String
    public var column: String?
    public var primaryKeys: [String: DataValue]
    public var previousValue: DataValue?
    public var appliedValue: DataValue?
    public var canBeRevertedInDatabase: Bool
    public var reversibilityDescription: String
    public var isUndone: Bool
    public var forwardOperation: GeneratedDatabaseOperation
    public var reverseOperation: GeneratedDatabaseOperation?

    public init(
        id: UUID = UUID(),
        timestamp: Date = Date(),
        kind: DatabaseOperationKind,
        schema: String,
        table: String,
        column: String? = nil,
        primaryKeys: [String: DataValue] = [:],
        previousValue: DataValue? = nil,
        appliedValue: DataValue? = nil,
        canBeRevertedInDatabase: Bool,
        reversibilityDescription: String,
        isUndone: Bool = false,
        forwardOperation: GeneratedDatabaseOperation,
        reverseOperation: GeneratedDatabaseOperation? = nil
    ) {
        self.id = id
        self.timestamp = timestamp
        self.kind = kind
        self.schema = schema
        self.table = table
        self.column = column
        self.primaryKeys = primaryKeys
        self.previousValue = previousValue
        self.appliedValue = appliedValue
        self.canBeRevertedInDatabase = canBeRevertedInDatabase
        self.reversibilityDescription = reversibilityDescription
        self.isUndone = isUndone
        self.forwardOperation = forwardOperation
        self.reverseOperation = reverseOperation
    }

    public var summaryText: String {
        switch kind {
        case .cellUpdate:
            if let col = column, let prev = previousValue, let applied = appliedValue {
                return "\(table).\(col): '\(prev.displayText)' → '\(applied.displayText)'"
            }
            return "Update \(table)"
        case .rowInsert:
            return "Insert row into \(table)"
        case .rowDelete:
            return "Delete row from \(table)"
        case .ddlStatement:
            return "DDL Schema modification on \(table)"
        case .customSQL:
            return "Custom SQL query executed"
        }
    }
}

@MainActor
public final class EditHistoryManager: ObservableObject {
    public static let shared = EditHistoryManager()

    @Published public private(set) var history: [EditHistoryEntry] = []
    @Published public private(set) var undoStack: [EditHistoryEntry] = []
    @Published public private(set) var redoStack: [EditHistoryEntry] = []

    public var canUndo: Bool {
        guard let top = undoStack.last else { return false }
        return top.canBeRevertedInDatabase
    }

    public var canRedo: Bool {
        !redoStack.isEmpty
    }

    public var mostRecentUndoDescription: String? {
        undoStack.last?.summaryText
    }

    public init() {}

    public func recordCellUpdate(
        payload: CellEditPayload,
        forwardOp: GeneratedDatabaseOperation,
        reverseOp: GeneratedDatabaseOperation?
    ) {
        let entry = EditHistoryEntry(
            kind: .cellUpdate,
            schema: payload.schema,
            table: payload.tableName,
            column: payload.columnName,
            primaryKeys: payload.primaryKeys,
            previousValue: payload.oldValue,
            appliedValue: payload.newValue,
            canBeRevertedInDatabase: reverseOp != nil,
            reversibilityDescription: reverseOp != nil
                ? "Reversible: Can execute reverse UPDATE using primary key"
                : "Cannot safely revert: Table has no primary key",
            forwardOperation: forwardOp,
            reverseOperation: reverseOp
        )

        history.insert(entry, at: 0)
        undoStack.append(entry)
        redoStack.removeAll() // Clear redo on new action
    }

    public func popUndoEntry() -> EditHistoryEntry? {
        guard let entry = undoStack.popLast() else { return nil }
        var undoneEntry = entry
        undoneEntry.isUndone = true
        redoStack.append(undoneEntry)
        return undoneEntry
    }

    public func popRedoEntry() -> EditHistoryEntry? {
        guard let entry = redoStack.popLast() else { return nil }
        var redoneEntry = entry
        redoneEntry.isUndone = false
        undoStack.append(redoneEntry)
        return redoneEntry
    }

    public func clear() {
        history.removeAll()
        undoStack.removeAll()
        redoStack.removeAll()
    }
}
