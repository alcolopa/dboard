import Foundation
import Combine

@MainActor
public final class ConnectionManager: ObservableObject {
    public static let shared = ConnectionManager()

    @Published public var savedConnections: [ConnectionConfig] = []
    @Published public var activeConnection: ConnectionConfig?
    @Published public var activeDriver: (any DatabaseDriver)?
    @Published public var connectionStatus: ConnectionStatus = .disconnected
    @Published public var activeDatabase: String = "safeatlas"
    @Published public var activeSchema: String = "public"
    @Published public var availableDatabases: [String] = ["safeatlas", "postgres", "testing"]
    @Published public var isConnecting: Bool = false
    @Published public var lastErrorMessage: String?

    private init() {
        loadDefaultConnections()
    }

    private func loadDefaultConnections() {
        let pgConn = ConnectionConfig(
            id: UUID(uuidString: "11111111-1111-1111-1111-111111111111")!,
            name: "Local PostgreSQL (safeatlas)",
            type: .postgresql,
            host: "127.0.0.1",
            port: 5432,
            databaseName: "safeatlas",
            username: "safeatlas",
            sslMode: .disable,
            environment: .local,
            colorTag: "#10B981",
            isFavorite: true,
            groupName: "Local"
        )
        _ = KeychainManager.shared.savePassword("secret", for: pgConn.keychainKey)

        let mysqlConn = ConnectionConfig(
            id: UUID(uuidString: "22222222-2222-2222-2222-222222222222")!,
            name: "Staging MySQL Store",
            type: .mysql,
            host: "127.0.0.1",
            port: 3306,
            databaseName: "shop_staging",
            username: "root",
            sslMode: .disable,
            environment: .staging,
            colorTag: "#F59E0B",
            isFavorite: false,
            groupName: "Staging"
        )

        let mongoConn = ConnectionConfig(
            id: UUID(uuidString: "33333333-3333-3333-3333-333333333333")!,
            name: "Local MongoDB Cluster",
            type: .mongodb,
            host: "localhost",
            port: 27017,
            databaseName: "ecom_nosql",
            username: "admin",
            mongoURI: "mongodb://admin:secret@localhost:27017/ecom_nosql",
            sslMode: .disable,
            environment: .local,
            colorTag: "#10B981",
            isFavorite: false,
            groupName: "Local"
        )

        savedConnections = [pgConn, mysqlConn, mongoConn]
    }

    public func autoConnectIfPossible() async {
        guard activeDriver == nil, let first = savedConnections.first else { return }
        await connect(to: first)
    }

    public func connect(to config: ConnectionConfig) async {
        isConnecting = true
        connectionStatus = .connecting
        lastErrorMessage = nil

        let driver: any DatabaseDriver
        switch config.type {
        case .postgresql:
            driver = PostgreSQLDriver(config: config)
        case .mysql:
            driver = MySQLDriver(config: config)
        case .mongodb:
            driver = MongoDBDriver(config: config)
        }

        do {
            try await driver.connect()
            self.activeDriver = driver
            self.activeConnection = config
            self.activeDatabase = config.databaseName
            self.connectionStatus = .connected
            self.isConnecting = false

            // Fetch live databases
            if let dbs = try? await driver.fetchDatabases(), !dbs.isEmpty {
                self.availableDatabases = dbs
            }

            // Update last used
            if let idx = savedConnections.firstIndex(where: { $0.id == config.id }) {
                savedConnections[idx].lastUsedAt = Date()
            }
        } catch {
            self.connectionStatus = .error(error.localizedDescription)
            self.lastErrorMessage = error.localizedDescription
            self.isConnecting = false
        }
    }

    public func disconnect() async {
        if let driver = activeDriver {
            try? await driver.disconnect()
        }
        activeDriver = nil
        activeConnection = nil
        connectionStatus = .disconnected
    }

    public func saveConnection(config: ConnectionConfig, password: String?) {
        if let idx = savedConnections.firstIndex(where: { $0.id == config.id }) {
            savedConnections[idx] = config
        } else {
            savedConnections.append(config)
        }

        if let pass = password, !pass.isEmpty {
            _ = KeychainManager.shared.savePassword(pass, for: config.keychainKey)
        }
    }

    public func duplicateConnection(config: ConnectionConfig) {
        var copy = config
        copy.id = UUID()
        copy.name = "\(config.name) (Copy)"
        copy.keychainKey = UUID().uuidString
        if let origPass = KeychainManager.shared.getPassword(for: config.keychainKey) {
            _ = KeychainManager.shared.savePassword(origPass, for: copy.keychainKey)
        }
        savedConnections.append(copy)
    }

    public func deleteConnection(config: ConnectionConfig) {
        _ = KeychainManager.shared.deletePassword(for: config.keychainKey)
        savedConnections.removeAll { $0.id == config.id }
        if activeConnection?.id == config.id {
            Task { await disconnect() }
        }
    }

    public func testConnection(config: ConnectionConfig, password: String?) async -> Result<String, Error> {
        let driver: any DatabaseDriver
        switch config.type {
        case .postgresql: driver = PostgreSQLDriver(config: config)
        case .mysql: driver = MySQLDriver(config: config)
        case .mongodb: driver = MongoDBDriver(config: config)
        }

        do {
            let version = try await driver.testConnection()
            return .success(version)
        } catch {
            return .failure(error)
        }
    }

    public func switchDatabase(to dbName: String) async {
        guard let driver = activeDriver else { return }
        activeDatabase = dbName
        _ = try? await driver.refreshMetadata(database: dbName)
        ActivityLogger.shared.log(
            statement: "-- Switched active database to: \(dbName)",
            database: dbName,
            durationMs: 5.0,
            isSuccess: true
        )
    }

    public func switchSchema(to schemaName: String) {
        activeSchema = schemaName
    }
}
