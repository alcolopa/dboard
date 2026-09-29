import Foundation

public enum TableType: String, Codable, CaseIterable {
    case table = "Table"
    case view = "View"
    case materializedView = "Materialized View"
    case collection = "Collection"

    public var iconName: String {
        switch self {
        case .table: return "tablecells"
        case .view: return "eye"
        case .materializedView: return "opticaldisc"
        case .collection: return "folder.badge.gearshape"
        }
    }
}

public struct TableMetadata: Identifiable, Codable, Equatable, Hashable {
    public var id: String { "\(schemaName).\(name)" }
    public var schemaName: String
    public var name: String
    public var type: TableType
    public var estimatedRows: Int64?
    public var sizeBytes: Int64?
    public var comment: String?
    public var columns: [ColumnDefinition]
    public var primaryKeyColumnNames: [String]

    public init(
        schemaName: String = "public",
        name: String,
        type: TableType = .table,
        estimatedRows: Int64? = nil,
        sizeBytes: Int64? = nil,
        comment: String? = nil,
        columns: [ColumnDefinition] = [],
        primaryKeyColumnNames: [String] = []
    ) {
        self.schemaName = schemaName
        self.name = name
        self.type = type
        self.estimatedRows = estimatedRows
        self.sizeBytes = sizeBytes
        self.comment = comment
        self.columns = columns
        self.primaryKeyColumnNames = primaryKeyColumnNames
    }

    public var hasPrimaryKey: Bool {
        !primaryKeyColumnNames.isEmpty
    }
}

public struct IndexMetadata: Identifiable, Codable, Equatable {
    public var id: String { "\(tableName).\(name)" }
    public var name: String
    public var schemaName: String
    public var tableName: String
    public var isUnique: Bool
    public var isPrimary: Bool
    public var method: String // BTREE, GIN, HASH, etc.
    public var columnNames: [String]
    public var definition: String

    public init(
        name: String,
        schemaName: String = "public",
        tableName: String,
        isUnique: Bool = false,
        isPrimary: Bool = false,
        method: String = "BTREE",
        columnNames: [String] = [],
        definition: String = ""
    ) {
        self.name = name
        self.schemaName = schemaName
        self.tableName = tableName
        self.isUnique = isUnique
        self.isPrimary = isPrimary
        self.method = method
        self.columnNames = columnNames
        self.definition = definition
    }
}

public struct ConstraintMetadata: Identifiable, Codable, Equatable {
    public var id: String { "\(tableName).\(name)" }
    public var name: String
    public var schemaName: String
    public var tableName: String
    public var type: ConstraintType
    public var definition: String
    public var foreignTable: String?
    public var foreignColumns: [String]?
    public var referencedColumns: [String]?

    public enum ConstraintType: String, Codable {
        case primaryKey = "PRIMARY KEY"
        case foreignKey = "FOREIGN KEY"
        case unique = "UNIQUE"
        case check = "CHECK"
    }

    public init(
        name: String,
        schemaName: String = "public",
        tableName: String,
        type: ConstraintType,
        definition: String,
        foreignTable: String? = nil,
        foreignColumns: [String]? = nil,
        referencedColumns: [String]? = nil
    ) {
        self.name = name
        self.schemaName = schemaName
        self.tableName = tableName
        self.type = type
        self.definition = definition
        self.foreignTable = foreignTable
        self.foreignColumns = foreignColumns
        self.referencedColumns = referencedColumns
    }
}

public struct TriggerMetadata: Identifiable, Codable, Equatable {
    public var id: String { "\(tableName).\(name)" }
    public var name: String
    public var schemaName: String
    public var tableName: String
    public var timing: String // BEFORE, AFTER
    public var event: String  // INSERT, UPDATE, DELETE
    public var functionName: String
    public var definition: String

    public init(
        name: String,
        schemaName: String = "public",
        tableName: String,
        timing: String = "AFTER",
        event: String = "UPDATE",
        functionName: String = "",
        definition: String = ""
    ) {
        self.name = name
        self.schemaName = schemaName
        self.tableName = tableName
        self.timing = timing
        self.event = event
        self.functionName = functionName
        self.definition = definition
    }
}

public struct RoutineMetadata: Identifiable, Codable, Equatable {
    public var id: String { "\(schemaName).\(name)" }
    public var schemaName: String
    public var name: String
    public var routineType: RoutineType
    public var returnType: String
    public var arguments: String
    public var language: String
    public var definition: String

    public enum RoutineType: String, Codable {
        case function = "FUNCTION"
        case procedure = "PROCEDURE"
    }

    public init(
        schemaName: String = "public",
        name: String,
        routineType: RoutineType = .function,
        returnType: String = "void",
        arguments: String = "",
        language: String = "plpgsql",
        definition: String = ""
    ) {
        self.schemaName = schemaName
        self.name = name
        self.routineType = routineType
        self.returnType = returnType
        self.arguments = arguments
        self.language = language
        self.definition = definition
    }

    public var isProcedure: Bool {
        routineType == .procedure
    }

    public var isFunction: Bool {
        routineType == .function
    }

    public var callSyntaxTemplate: String {
        let args = arguments.isEmpty ? "" : arguments
        if isProcedure {
            return "CALL \"\(schemaName)\".\"\(name)\"(\(args));"
        } else {
            return "SELECT \"\(schemaName)\".\"\(name)\"(\(args));"
        }
    }
}

public struct SequenceMetadata: Identifiable, Codable, Equatable {
    public var id: String { "\(schemaName).\(name)" }
    public var schemaName: String
    public var name: String
    public var dataType: String
    public var startValue: Int64
    public var currentValue: Int64

    public init(
        schemaName: String = "public",
        name: String,
        dataType: String = "bigint",
        startValue: Int64 = 1,
        currentValue: Int64 = 1
    ) {
        self.schemaName = schemaName
        self.name = name
        self.dataType = dataType
        self.startValue = startValue
        self.currentValue = currentValue
    }
}

public struct MongoCollectionStats: Codable, Equatable {
    public var documentCount: Int64
    public var avgDocumentSizeBytes: Double
    public var totalStorageSizeBytes: Int64
    public var indexCount: Int
    public var totalIndexSizeBytes: Int64

    public init(
        documentCount: Int64 = 0,
        avgDocumentSizeBytes: Double = 0.0,
        totalStorageSizeBytes: Int64 = 0,
        indexCount: Int = 1,
        totalIndexSizeBytes: Int64 = 0
    ) {
        self.documentCount = documentCount
        self.avgDocumentSizeBytes = avgDocumentSizeBytes
        self.totalStorageSizeBytes = totalStorageSizeBytes
        self.indexCount = indexCount
        self.totalIndexSizeBytes = totalIndexSizeBytes
    }
}

public struct DatabaseMetadata: Equatable {
    public var databaseName: String
    public var schemas: [String]
    public var tables: [TableMetadata]
    public var views: [TableMetadata]
    public var routines: [RoutineMetadata]
    public var sequences: [SequenceMetadata]
    public var indexes: [IndexMetadata]
    public var constraints: [ConstraintMetadata]
    public var triggers: [TriggerMetadata]
    public var mongoStats: [String: MongoCollectionStats]

    public init(
        databaseName: String = "",
        schemas: [String] = ["public"],
        tables: [TableMetadata] = [],
        views: [TableMetadata] = [],
        routines: [RoutineMetadata] = [],
        sequences: [SequenceMetadata] = [],
        indexes: [IndexMetadata] = [],
        constraints: [ConstraintMetadata] = [],
        triggers: [TriggerMetadata] = [],
        mongoStats: [String: MongoCollectionStats] = [:]
    ) {
        self.databaseName = databaseName
        self.schemas = schemas
        self.tables = tables
        self.views = views
        self.routines = routines
        self.sequences = sequences
        self.indexes = indexes
        self.constraints = constraints
        self.triggers = triggers
        self.mongoStats = mongoStats
    }

    public func table(named name: String, schema: String = "public") -> TableMetadata? {
        tables.first { $0.name == name && $0.schemaName == schema }
    }
}
