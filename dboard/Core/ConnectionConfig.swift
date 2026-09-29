import Foundation

public enum DatabaseType: String, Codable, CaseIterable, Identifiable {
    case postgresql = "PostgreSQL"
    case mysql = "MySQL / MariaDB"
    case mongodb = "MongoDB"

    public var id: String { rawValue }

    public var defaultPort: Int {
        switch self {
        case .postgresql: return 5432
        case .mysql: return 3306
        case .mongodb: return 27017
        }
    }

    public var iconName: String {
        switch self {
        case .postgresql: return "cylinder.split.1x2"
        case .mysql: return "server.rack"
        case .mongodb: return "leaf"
        }
    }

    public var defaultUsername: String {
        switch self {
        case .postgresql: return "postgres"
        case .mysql: return "root"
        case .mongodb: return "admin"
        }
    }

    public var defaultDatabase: String {
        switch self {
        case .postgresql: return "postgres"
        case .mysql: return "mysql"
        case .mongodb: return "test"
        }
    }
}

public enum ConnectionEnvironment: String, Codable, CaseIterable, Identifiable {
    case production = "Production"
    case staging = "Staging"
    case development = "Development"
    case local = "Local"

    public var id: String { rawValue }

    public var badgeColorHex: String {
        switch self {
        case .production: return "#EF4444" // Bright Red
        case .staging: return "#F59E0B"    // Amber
        case .development: return "#3B82F6"// Blue
        case .local: return "#10B981"      // Emerald Green
        }
    }

    public var requiresDestructiveConfirmation: Bool {
        switch self {
        case .production, .staging: return true
        case .development, .local: return false
        }
    }
}

public enum SSLMode: String, Codable, CaseIterable, Identifiable {
    case disable = "Disable"
    case prefer = "Prefer"
    case require = "Require"
    case verifyCA = "Verify CA"
    case verifyFull = "Verify Full"

    public var id: String { rawValue }
}

public struct SSHTunnelConfig: Codable, Equatable {
    public var enabled: Bool = false
    public var host: String = ""
    public var port: Int = 22
    public var username: String = ""
    public var authenticationType: SSHAuthType = .password
    public var privateKeyPath: String = ""
    public var passphraseKeychainKey: String? = nil

    public enum SSHAuthType: String, Codable, CaseIterable, Identifiable {
        case password = "Password"
        case keyFile = "Private Key File"
        case sshAgent = "SSH Agent"
        public var id: String { rawValue }
    }

    public init(
        enabled: Bool = false,
        host: String = "",
        port: Int = 22,
        username: String = "",
        authenticationType: SSHAuthType = .password,
        privateKeyPath: String = "",
        passphraseKeychainKey: String? = nil
    ) {
        self.enabled = enabled
        self.host = host
        self.port = port
        self.username = username
        self.authenticationType = authenticationType
        self.privateKeyPath = privateKeyPath
        self.passphraseKeychainKey = passphraseKeychainKey
    }
}

public struct ConnectionConfig: Identifiable, Codable, Equatable {
    public var id: UUID
    public var name: String
    public var type: DatabaseType
    public var host: String
    public var port: Int
    public var databaseName: String
    public var username: String
    public var keychainKey: String // Identifier for Keychain storage
    public var mongoURI: String?
    public var sslMode: SSLMode
    public var sshTunnel: SSHTunnelConfig
    public var environment: ConnectionEnvironment
    public var colorTag: String?
    public var isReadOnly: Bool
    public var isFavorite: Bool
    public var groupName: String
    public var connectionTimeoutSeconds: Int
    public var queryTimeoutSeconds: Int
    public var createdAt: Date
    public var lastUsedAt: Date?

    public init(
        id: UUID = UUID(),
        name: String,
        type: DatabaseType,
        host: String = "localhost",
        port: Int? = nil,
        databaseName: String = "",
        username: String = "",
        keychainKey: String = UUID().uuidString,
        mongoURI: String? = nil,
        sslMode: SSLMode = .prefer,
        sshTunnel: SSHTunnelConfig = SSHTunnelConfig(),
        environment: ConnectionEnvironment = .local,
        colorTag: String? = nil,
        isReadOnly: Bool = false,
        isFavorite: Bool = false,
        groupName: String = "Default",
        connectionTimeoutSeconds: Int = 10,
        queryTimeoutSeconds: Int = 60,
        createdAt: Date = Date(),
        lastUsedAt: Date? = nil
    ) {
        self.id = id
        self.name = name
        self.type = type
        self.host = host
        self.port = port ?? type.defaultPort
        self.databaseName = databaseName.isEmpty ? type.defaultDatabase : databaseName
        self.username = username.isEmpty ? type.defaultUsername : username
        self.keychainKey = keychainKey
        self.mongoURI = mongoURI
        self.sslMode = sslMode
        self.sshTunnel = sshTunnel
        self.environment = environment
        self.colorTag = colorTag ?? environment.badgeColorHex
        self.isReadOnly = isReadOnly
        self.isFavorite = isFavorite
        self.groupName = groupName
        self.connectionTimeoutSeconds = connectionTimeoutSeconds
        self.queryTimeoutSeconds = queryTimeoutSeconds
        self.createdAt = createdAt
        self.lastUsedAt = lastUsedAt
    }

    public var displayURI: String {
        if type == .mongodb, let uri = mongoURI, !uri.isEmpty {
            return sanitizeURI(uri)
        }
        return "\(username)@\(host):\(port)/\(databaseName)"
    }

    private func sanitizeURI(_ uri: String) -> String {
        guard let url = URL(string: uri) else { return uri }
        var components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        if components?.password != nil {
            components?.password = "••••••"
        }
        return components?.string ?? uri
    }
}
