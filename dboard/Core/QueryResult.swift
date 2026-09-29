import Foundation

public enum DataValue: Codable, Equatable, Hashable, CustomStringConvertible {
    case null
    case string(String)
    case integer(Int64)
    case double(Double)
    case boolean(Bool)
    case date(Date)
    case json(String)
    case array([DataValue])
    case binary(Data)
    case objectId(String)

    public var isNull: Bool {
        if case .null = self { return true }
        return false
    }

    public var isBoolean: Bool {
        if case .boolean = self { return true }
        return false
    }

    public var isNumeric: Bool {
        switch self {
        case .integer, .double: return true
        default: return false
        }
    }

    public var isJSON: Bool {
        if case .json = self { return true }
        return false
    }

    public var displayText: String {
        switch self {
        case .null:
            return "NULL"
        case .string(let str):
            return str
        case .integer(let val):
            return "\(val)"
        case .double(let val):
            return String(format: "%g", val)
        case .boolean(let bool):
            return bool ? "TRUE" : "FALSE"
        case .date(let date):
            let formatter = ISO8601DateFormatter()
            formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            return formatter.string(from: date)
        case .json(let jsonStr):
            return jsonStr
        case .array(let elements):
            return "[" + elements.map { $0.displayText }.joined(separator: ", ") + "]"
        case .binary(let data):
            return "<Binary \(data.count) bytes>"
        case .objectId(let oid):
            return oid
        }
    }

    public var rawStringValue: String {
        switch self {
        case .null: return ""
        case .string(let s): return s
        case .integer(let i): return "\(i)"
        case .double(let d): return "\(d)"
        case .boolean(let b): return b ? "true" : "false"
        case .date(let d):
            let formatter = ISO8601DateFormatter()
            return formatter.string(from: d)
        case .json(let j): return j
        case .array(let a): return "[" + a.map { $0.rawStringValue }.joined(separator: ",") + "]"
        case .binary(let b): return b.base64EncodedString()
        case .objectId(let oid): return oid
        }
    }

    public var sqlLiteral: String {
        switch self {
        case .null:
            return "NULL"
        case .string(let s):
            let escaped = s.replacingOccurrences(of: "'", with: "''")
            return "'\(escaped)'"
        case .integer(let i):
            return "\(i)"
        case .double(let d):
            return "\(d)"
        case .boolean(let b):
            return b ? "TRUE" : "FALSE"
        case .date(let d):
            let formatter = ISO8601DateFormatter()
            return "'\(formatter.string(from: d))'"
        case .json(let j):
            let escaped = j.replacingOccurrences(of: "'", with: "''")
            return "'\(escaped)'"
        case .array(let arr):
            return "ARRAY[" + arr.map { $0.sqlLiteral }.joined(separator: ", ") + "]"
        case .binary(let b):
            return "'\\x\(b.map { String(format: "%02hhx", $0) }.joined())'"
        case .objectId(let oid):
            return "'\(oid)'"
        }
    }

    public var description: String { displayText }

    public static func parseFromInput(_ input: String, targetType: String) -> DataValue {
        let trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.uppercased() == "NULL" || trimmed.isEmpty {
            return .null
        }

        let lowerType = targetType.lowercased()
        if lowerType.contains("int") || lowerType.contains("serial") {
            if let intVal = Int64(trimmed) {
                return .integer(intVal)
            }
        } else if lowerType.contains("float") || lowerType.contains("double") || lowerType.contains("numeric") || lowerType.contains("decimal") {
            if let dblVal = Double(trimmed) {
                return .double(dblVal)
            }
        } else if lowerType.contains("bool") {
            if trimmed.lowercased() == "true" || trimmed == "1" || trimmed.lowercased() == "t" {
                return .boolean(true)
            } else if trimmed.lowercased() == "false" || trimmed == "0" || trimmed.lowercased() == "f" {
                return .boolean(false)
            }
        } else if lowerType.contains("json") {
            return .json(trimmed)
        } else if lowerType.contains("date") || lowerType.contains("time") {
            let formatter = ISO8601DateFormatter()
            if let d = formatter.date(from: trimmed) {
                return .date(d)
            }
        } else if lowerType.contains("objectid") {
            return .objectId(trimmed)
        }

        return .string(trimmed)
    }
}

public struct ColumnDefinition: Identifiable, Codable, Equatable, Hashable {
    public var id: String { name }
    public var name: String
    public var ordinalPosition: Int
    public var dataTypeName: String
    public var isPrimaryKey: Bool
    public var isForeignKey: Bool
    public var isNullable: Bool
    public var defaultValue: String?
    public var foreignTable: String?
    public var foreignColumn: String?
    public var comment: String?
    public var characterMaximumLength: Int?

    public init(
        name: String,
        ordinalPosition: Int,
        dataTypeName: String,
        isPrimaryKey: Bool = false,
        isForeignKey: Bool = false,
        isNullable: Bool = true,
        defaultValue: String? = nil,
        foreignTable: String? = nil,
        foreignColumn: String? = nil,
        comment: String? = nil,
        characterMaximumLength: Int? = nil
    ) {
        self.name = name
        self.ordinalPosition = ordinalPosition
        self.dataTypeName = dataTypeName
        self.isPrimaryKey = isPrimaryKey
        self.isForeignKey = isForeignKey
        self.isNullable = isNullable
        self.defaultValue = defaultValue
        self.foreignTable = foreignTable
        self.foreignColumn = foreignColumn
        self.comment = comment
        self.characterMaximumLength = characterMaximumLength
    }
}

public struct DataRow: Identifiable, Equatable {
    public let id: UUID
    public var values: [String: DataValue]
    public var originalValues: [String: DataValue]

    public init(id: UUID = UUID(), values: [String: DataValue], originalValues: [String: DataValue]? = nil) {
        self.id = id
        self.values = values
        self.originalValues = originalValues ?? values
    }

    public subscript(column: String) -> DataValue {
        get { values[column] ?? .null }
        set { values[column] = newValue }
    }

    public func isColumnModified(_ column: String) -> Bool {
        return values[column] != originalValues[column]
    }
}

public struct ExplainPlanNode: Identifiable, Codable, Equatable {
    public var id: UUID = UUID()
    public var nodeType: String
    public var relationName: String?
    public var startupCost: Double
    public var totalCost: Double
    public var planRows: Int
    public var actualStartupTime: Double?
    public var actualTotalTime: Double?
    public var actualRows: Int?
    public var filter: String?
    public var indexName: String?
    public var children: [ExplainPlanNode]

    public init(
        id: UUID = UUID(),
        nodeType: String,
        relationName: String? = nil,
        startupCost: Double = 0.0,
        totalCost: Double = 0.0,
        planRows: Int = 0,
        actualStartupTime: Double? = nil,
        actualTotalTime: Double? = nil,
        actualRows: Int? = nil,
        filter: String? = nil,
        indexName: String? = nil,
        children: [ExplainPlanNode] = []
    ) {
        self.id = id
        self.nodeType = nodeType
        self.relationName = relationName
        self.startupCost = startupCost
        self.totalCost = totalCost
        self.planRows = planRows
        self.actualStartupTime = actualStartupTime
        self.actualTotalTime = actualTotalTime
        self.actualRows = actualRows
        self.filter = filter
        self.indexName = indexName
        self.children = children
    }
}

public struct QueryResult: Identifiable, Equatable {
    public let id: UUID
    public var columns: [ColumnDefinition]
    public var rows: [DataRow]
    public var executionDurationMs: Double
    public var affectedRows: Int
    public var totalRowCount: Int
    public var sqlStatement: String
    public var isTruncated: Bool
    public var explainPlan: ExplainPlanNode?
    public var errorMessage: String?

    public init(
        id: UUID = UUID(),
        columns: [ColumnDefinition] = [],
        rows: [DataRow] = [],
        executionDurationMs: Double = 0,
        affectedRows: Int = 0,
        totalRowCount: Int = 0,
        sqlStatement: String = "",
        isTruncated: Bool = false,
        explainPlan: ExplainPlanNode? = nil,
        errorMessage: String? = nil
    ) {
        self.id = id
        self.columns = columns
        self.rows = rows
        self.executionDurationMs = executionDurationMs
        self.affectedRows = affectedRows
        self.totalRowCount = totalRowCount
        self.sqlStatement = sqlStatement
        self.isTruncated = isTruncated
        self.explainPlan = explainPlan
        self.errorMessage = errorMessage
    }

    public var isSuccess: Bool { errorMessage == nil }
}
