import SwiftUI

public struct ConnectionManagerModal: View {
    @Binding var isPresented: Bool
    @ObservedObject var connectionManager = ConnectionManager.shared

    @State private var selectedConnectionId: UUID? = nil
    @State private var draftConfig: ConnectionConfig = ConnectionConfig(name: "New Connection", type: .postgresql)
    @State private var passwordInput: String = ""
    @State private var testResultText: String? = nil
    @State private var isTestSuccess: Bool = true
    @State private var isTesting: Bool = false
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        HStack(spacing: 0) {
            // Left list of connections
            VStack(spacing: 0) {
                HStack {
                    Text("Connections")
                        .font(ThemeTokens.uiFont(size: 13, weight: .bold))
                    Spacer()
                    Button(action: createNewConnection) {
                        Image(systemName: "plus")
                            .font(.system(size: 11, weight: .bold))
                    }
                    .buttonStyle(.plain)
                }
                .padding(12)
                .background(ThemeTokens.bgSidebar(for: scheme))

                Divider().background(ThemeTokens.borderColor(for: scheme))

                List(selection: $selectedConnectionId) {
                    ForEach(connectionManager.savedConnections) { conn in
                        HStack(spacing: 8) {
                            Circle()
                                .fill(Color(hex: conn.colorTag ?? conn.environment.badgeColorHex))
                                .frame(width: 8, height: 8)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(conn.name)
                                    .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                                Text("\(conn.type.rawValue) • \(conn.environment.rawValue)")
                                    .font(ThemeTokens.uiFont(size: 10))
                                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                            }
                            Spacer()
                        }
                        .tag(conn.id)
                        .padding(.vertical, 3)
                    }

                    if isCreatingNew {
                        HStack(spacing: 8) {
                            Circle()
                                .fill(ThemeTokens.accentBlue)
                                .frame(width: 8, height: 8)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(draftConfig.name.isEmpty ? "New Connection" : draftConfig.name)
                                    .font(ThemeTokens.uiFont(size: 12, weight: .semibold))
                                    .foregroundColor(ThemeTokens.accentBlue)
                                Text("Unsaved draft")
                                    .font(ThemeTokens.uiFont(size: 10))
                                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                            }
                            Spacer()
                        }
                        .tag(draftConfig.id)
                        .padding(.vertical, 3)
                    }
                }
                .listStyle(.sidebar)
            }
            .frame(width: 220)

            Divider().background(ThemeTokens.borderColor(for: scheme))

            // Right form editor
            VStack(spacing: 0) {
                // Header with Close / Cancel
                HStack {
                    Text(isCreatingNew ? "New Connection Configuration" : "Edit Connection")
                        .font(ThemeTokens.uiFont(size: 13, weight: .semibold))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                    Spacer()
                    Button(action: { isPresented = false }) {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 14))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .buttonStyle(.plain)
                    .help("Close")
                }
                .padding(.horizontal, 16)
                .padding(.top, 12)
                .padding(.bottom, 6)

                Divider().background(ThemeTokens.borderColor(for: scheme))

                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        // Title & Type Selector
                        HStack(spacing: 12) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text("Connection Name")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                TextField("My Database", text: $draftConfig.name)
                                    .textFieldStyle(.roundedBorder)
                            }

                            VStack(alignment: .leading, spacing: 4) {
                                Text("Database Engine")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                Picker("", selection: Binding(
                                    get: { draftConfig.type },
                                    set: { newType in
                                        draftConfig.type = newType
                                        draftConfig.port = newType.defaultPort
                                        if draftConfig.username == DatabaseType.postgresql.defaultUsername ||
                                           draftConfig.username == DatabaseType.mysql.defaultUsername ||
                                           draftConfig.username == DatabaseType.mongodb.defaultUsername {
                                            draftConfig.username = newType.defaultUsername
                                        }
                                        if draftConfig.databaseName == DatabaseType.postgresql.defaultDatabase ||
                                           draftConfig.databaseName == DatabaseType.mysql.defaultDatabase ||
                                           draftConfig.databaseName == DatabaseType.mongodb.defaultDatabase {
                                            draftConfig.databaseName = newType.defaultDatabase
                                        }
                                    }
                                )) {
                                    ForEach(DatabaseType.allCases) { type in
                                        Text(type.rawValue).tag(type)
                                    }
                                }
                                .pickerStyle(.menu)
                            }
                        }

                        // Environment & Safety
                        HStack(spacing: 12) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text("Environment Classification")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                Picker("", selection: $draftConfig.environment) {
                                    ForEach(ConnectionEnvironment.allCases) { env in
                                        Text(env.rawValue).tag(env)
                                    }
                                }
                                .pickerStyle(.menu)
                            }

                            VStack(alignment: .leading, spacing: 4) {
                                Text("SSL / TLS Mode")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                Picker("", selection: $draftConfig.sslMode) {
                                    ForEach(SSLMode.allCases) { mode in
                                        Text(mode.rawValue).tag(mode)
                                    }
                                }
                                .pickerStyle(.menu)
                            }
                        }

                        Divider().background(ThemeTokens.borderColor(for: scheme))

                        if draftConfig.type == .mongodb {
                            VStack(alignment: .leading, spacing: 4) {
                                Text("MongoDB Connection URI")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                TextField("mongodb://user:pass@host:27017/dbname", text: Binding(
                                    get: { draftConfig.mongoURI ?? "" },
                                    set: { draftConfig.mongoURI = $0 }
                                ))
                                .textFieldStyle(.roundedBorder)
                                .font(ThemeTokens.codeFont(size: 11.5))
                            }
                        }

                        // Host & Port
                        HStack(spacing: 12) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text("Host / Server")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                TextField("localhost", text: $draftConfig.host)
                                    .textFieldStyle(.roundedBorder)
                            }

                            VStack(alignment: .leading, spacing: 4) {
                                Text("Port")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                TextField("Port", value: $draftConfig.port, formatter: NumberFormatter())
                                    .textFieldStyle(.roundedBorder)
                                    .frame(width: 80)
                            }
                        }

                        // Database & User
                        HStack(spacing: 12) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text("Database Name")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                TextField("postgres", text: $draftConfig.databaseName)
                                    .textFieldStyle(.roundedBorder)
                            }

                            VStack(alignment: .leading, spacing: 4) {
                                Text("Username")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                TextField("username", text: $draftConfig.username)
                                    .textFieldStyle(.roundedBorder)
                            }
                        }

                        // Password (with Keychain notice)
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text("Password")
                                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                                Spacer()
                                HStack(spacing: 3) {
                                    Image(systemName: "key.fill")
                                        .font(.system(size: 9))
                                    Text("Secured via macOS Keychain")
                                        .font(ThemeTokens.uiFont(size: 10))
                                }
                                .foregroundColor(ThemeTokens.accentEmerald)
                            }

                            SecureField("••••••••••••", text: $passwordInput)
                                .textFieldStyle(.roundedBorder)
                        }

                        // Test Result Output
                        if let res = testResultText {
                            HStack(spacing: 6) {
                                Image(systemName: isTestSuccess ? "checkmark.seal.fill" : "xmark.octagon.fill")
                                    .foregroundColor(isTestSuccess ? ThemeTokens.accentEmerald : ThemeTokens.accentCrimson)
                                Text(res)
                                    .font(ThemeTokens.codeFont(size: 11))
                                    .foregroundColor(isTestSuccess ? ThemeTokens.accentEmerald : ThemeTokens.accentCrimson)
                            }
                            .padding(8)
                            .background((isTestSuccess ? ThemeTokens.accentEmerald : ThemeTokens.accentCrimson).opacity(0.1))
                            .cornerRadius(6)
                        }
                    }
                    .padding(16)
                }

                Divider().background(ThemeTokens.borderColor(for: scheme))

                // Action Footer
                HStack(spacing: 10) {
                    Button("Test Connection") {
                        Task { await testConnection() }
                    }
                    .font(ThemeTokens.uiFont(size: 11.5))
                    .disabled(isTesting)

                    if isTesting {
                        ProgressView().scaleEffect(0.5)
                    }

                    Spacer()

                    if !isCreatingNew {
                        Button("Duplicate") {
                            connectionManager.duplicateConnection(config: draftConfig)
                        }
                        .font(ThemeTokens.uiFont(size: 11.5))

                        Button("Delete") {
                            connectionManager.deleteConnection(config: draftConfig)
                            if let first = connectionManager.savedConnections.first {
                                selectConnection(first)
                            } else {
                                createNewConnection()
                            }
                        }
                        .font(ThemeTokens.uiFont(size: 11.5))
                        .foregroundColor(ThemeTokens.accentCrimson)
                    }

                    Button("Save") {
                        saveOnly()
                    }
                    .font(ThemeTokens.uiFont(size: 11.5))

                    Button("Connect") {
                        saveAndConnect()
                    }
                    .font(ThemeTokens.uiFont(size: 12, weight: .bold))
                    .buttonStyle(.borderedProminent)
                }
                .padding(12)
                .background(ThemeTokens.bgElevated(for: scheme))
            }
        }
        .frame(width: 740, height: 500)
        .background(ThemeTokens.bgPrimary(for: scheme))
        .onAppear {
            if let active = connectionManager.activeConnection {
                selectConnection(active)
            } else if let first = connectionManager.savedConnections.first {
                selectConnection(first)
            } else {
                createNewConnection()
            }
        }
        .onChange(of: selectedConnectionId) { _, newId in
            if let id = newId, let conn = connectionManager.savedConnections.first(where: { $0.id == id }) {
                selectConnection(conn)
            }
        }
    }

    private var isCreatingNew: Bool {
        !connectionManager.savedConnections.contains(where: { $0.id == draftConfig.id })
    }

    private func createNewConnection() {
        let newConfig = ConnectionConfig(
            id: UUID(),
            name: "New Connection",
            type: .postgresql,
            host: "localhost",
            port: 5432,
            databaseName: "postgres",
            username: "postgres"
        )
        draftConfig = newConfig
        passwordInput = ""
        testResultText = nil
        selectedConnectionId = newConfig.id
    }

    private func selectConnection(_ conn: ConnectionConfig) {
        selectedConnectionId = conn.id
        draftConfig = conn
        passwordInput = KeychainManager.shared.getPassword(for: conn.keychainKey) ?? ""
        testResultText = nil
    }

    private func testConnection() async {
        isTesting = true
        testResultText = nil
        let res = await connectionManager.testConnection(config: draftConfig, password: passwordInput)
        isTesting = false
        switch res {
        case .success(let info):
            isTestSuccess = true
            testResultText = "Connection Succeeded: \(info)"
        case .failure(let err):
            isTestSuccess = false
            testResultText = "Connection Failed: \(err.localizedDescription)"
        }
    }

    private func saveOnly() {
        connectionManager.saveConnection(config: draftConfig, password: passwordInput)
        selectedConnectionId = draftConfig.id
        testResultText = "Connection settings saved"
        isTestSuccess = true
    }

    private func saveAndConnect() {
        connectionManager.saveConnection(config: draftConfig, password: passwordInput)
        Task {
            await connectionManager.connect(to: draftConfig)
            isPresented = false
        }
    }
}
