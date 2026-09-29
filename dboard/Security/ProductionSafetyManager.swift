import Foundation

public struct DestructiveActionDetails: Identifiable {
    public let id: UUID = UUID()
    public var title: String
    public var description: String
    public var environment: ConnectionEnvironment
    public var requiredConfirmationText: String
    public var onConfirm: () -> Void

    public init(
        title: String,
        description: String,
        environment: ConnectionEnvironment,
        requiredConfirmationText: String,
        onConfirm: @escaping () -> Void
    ) {
        self.title = title
        self.description = description
        self.environment = environment
        self.requiredConfirmationText = requiredConfirmationText
        self.onConfirm = onConfirm
    }
}

public final class ProductionSafetyManager {
    public static let shared = ProductionSafetyManager()

    private init() {}

    public static func isDestructiveQuery(_ sql: String) -> Bool {
        let clean = sql.trimmingCharacters(in: .whitespacesAndNewlines).uppercased()
        let destructiveKeywords = [
            "DROP TABLE", "DROP DATABASE", "DROP SCHEMA", "DROP VIEW",
            "TRUNCATE", "DELETE FROM", "ALTER TABLE", "DROP COLUMN"
        ]
        return destructiveKeywords.contains { clean.contains($0) }
    }

    public static func requiresConfirmation(environment: ConnectionEnvironment, isDestructive: Bool) -> Bool {
        if isDestructive && (environment == .production || environment == .staging) {
            return true
        }
        return false
    }
}
