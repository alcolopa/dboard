import Foundation

public final class PostgreSQLClient {
    public let host: String
    public let port: Int
    public let database: String
    public let user: String
    public let password: String?
    public let sslMode: SSLMode

    private static let isoDateFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static let fallbackDateFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    public init(config: ConnectionConfig, password: String?) {
        self.host = config.host
        self.port = config.port
        self.database = config.databaseName
        self.user = config.username
        self.password = password
        self.sslMode = config.sslMode
    }

    public static func resolvePsqlPath() -> String {
        let candidates = [
            "/opt/homebrew/bin/psql",
            "/opt/homebrew/opt/libpq/bin/psql",
            "/usr/local/bin/psql",
            "/usr/bin/psql"
        ]
        for path in candidates {
            if FileManager.default.isExecutableFile(atPath: path) {
                return path
            }
        }

        // Check PATH
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: "/usr/bin/which")
        proc.arguments = ["psql"]
        let pipe = Pipe()
        proc.standardOutput = pipe
        try? proc.run()
        proc.waitUntilExit()
        if proc.terminationStatus == 0 {
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            if let out = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
               !out.isEmpty, FileManager.default.isExecutableFile(atPath: out) {
                return out
            }
        }

        return "psql"
    }

    public func execute(
        sql: String,
        targetDatabase: String? = nil,
        tupleOnly: Bool = false,
        asCsv: Bool = false
    ) async throws -> (output: String, error: String, exitCode: Int32) {
        let db = targetDatabase ?? self.database
        let psqlPath = Self.resolvePsqlPath()

        let tmpDir = FileManager.default.temporaryDirectory
        let tmpFile = tmpDir.appendingPathComponent("dboard_\(UUID().uuidString).sql")
        try sql.write(to: tmpFile, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: tmpFile) }

        var args = [
            "-h", host,
            "-p", "\(port)",
            "-U", user,
            "-d", db,
            "-f", tmpFile.path
        ]
        if tupleOnly {
            args.append(contentsOf: ["-t", "-A"])
        }
        if asCsv {
            args.append("--csv")
        }

        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: psqlPath)
        proc.arguments = args

        var env = ProcessInfo.processInfo.environment
        if let pw = password, !pw.isEmpty {
            env["PGPASSWORD"] = pw
        }
        env["PGCLIENTENCODING"] = "UTF8"
        proc.environment = env

        let outPipe = Pipe()
        let errPipe = Pipe()
        proc.standardOutput = outPipe
        proc.standardError = errPipe

        try proc.run()
        let outData = outPipe.fileHandleForReading.readDataToEndOfFile()
        let errData = errPipe.fileHandleForReading.readDataToEndOfFile()
        proc.waitUntilExit()

        let outStr = String(data: outData, encoding: .utf8) ?? ""
        let errStr = String(data: errData, encoding: .utf8) ?? ""
        return (outStr, errStr, proc.terminationStatus)
    }

    public static func parseJsonRows(_ jsonString: String) -> [DataRow] {
        let trimmed = jsonString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let data = trimmed.data(using: .utf8) else { return [] }

        guard let jsonArray = (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]] else {
            return []
        }

        return jsonArray.map { dict in
            var values: [String: DataValue] = [:]
            for (k, v) in dict {
                values[k] = convertAnyToDataValue(v)
            }
            return DataRow(values: values)
        }
    }

    public static func convertAnyToDataValue(_ val: Any) -> DataValue {
        if val is NSNull {
            return .null
        }

        if let num = val as? NSNumber {
            if CFGetTypeID(num) == CFBooleanGetTypeID() {
                return .boolean(num.boolValue)
            }
            if num.stringValue.contains(".") {
                return .double(num.doubleValue)
            }
            return .integer(num.int64Value)
        }

        if let str = val as? String {
            if let date = isoDateFormatter.date(from: str) ?? fallbackDateFormatter.date(from: str) {
                return .date(date)
            }
            return .string(str)
        }

        if let arr = val as? [Any] {
            if let d = try? JSONSerialization.data(withJSONObject: arr), let s = String(data: d, encoding: .utf8) {
                return .json(s)
            }
            return .string("\(arr)")
        }

        if let dict = val as? [String: Any] {
            if let d = try? JSONSerialization.data(withJSONObject: dict), let s = String(data: d, encoding: .utf8) {
                return .json(s)
            }
            return .string("\(dict)")
        }

        return .string("\(val)")
    }

    public static func parseExplainPlanJson(_ jsonString: String) -> ExplainPlanNode? {
        guard let data = jsonString.trimmingCharacters(in: .whitespacesAndNewlines).data(using: .utf8),
              let rootArray = (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]],
              let first = rootArray.first,
              let plan = first["Plan"] as? [String: Any] else {
            return nil
        }
        return parseExplainNode(plan)
    }

    private static func parseExplainNode(_ dict: [String: Any]) -> ExplainPlanNode {
        let nodeType = dict["Node Type"] as? String ?? "Scan"
        let relationName = dict["Relation Name"] as? String
        let startupCost = (dict["Startup Cost"] as? NSNumber)?.doubleValue ?? 0.0
        let totalCost = (dict["Total Cost"] as? NSNumber)?.doubleValue ?? 0.0
        let planRows = (dict["Plan Rows"] as? NSNumber)?.intValue ?? 0
        let actualStartup = (dict["Actual Startup Time"] as? NSNumber)?.doubleValue
        let actualTotal = (dict["Actual Total Time"] as? NSNumber)?.doubleValue
        let actualRows = (dict["Actual Rows"] as? NSNumber)?.intValue
        let filter = dict["Filter"] as? String
        let indexName = dict["Index Name"] as? String

        var children: [ExplainPlanNode] = []
        if let plans = dict["Plans"] as? [[String: Any]] {
            children = plans.map { parseExplainNode($0) }
        }

        return ExplainPlanNode(
            nodeType: nodeType,
            relationName: relationName,
            startupCost: startupCost,
            totalCost: totalCost,
            planRows: planRows,
            actualStartupTime: actualStartup,
            actualTotalTime: actualTotal,
            actualRows: actualRows,
            filter: filter,
            indexName: indexName,
            children: children
        )
    }
}
