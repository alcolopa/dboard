import Foundation
import SwiftUI
import Combine

public enum AppTheme: String, Codable, CaseIterable, Identifiable {
    case system = "System"
    case dark = "Dark"
    case light = "Light"
    public var id: String { rawValue }

    public var colorScheme: ColorScheme? {
        switch self {
        case .system: return nil
        case .dark: return .dark
        case .light: return .light
        }
    }
}

public enum InterfaceDensity: String, Codable, CaseIterable, Identifiable {
    case compact = "Compact"
    case regular = "Regular"
    public var id: String { rawValue }

    public var rowHeight: CGFloat {
        switch self {
        case .compact: return 24.0
        case .regular: return 30.0
        }
    }

    public var cellPadding: CGFloat {
        switch self {
        case .compact: return 4.0
        case .regular: return 8.0
        }
    }
}

@MainActor
public final class AppSettings: ObservableObject {
    public static let shared = AppSettings()

    // General
    @Published public var theme: AppTheme = .dark
    @Published public var fontSize: Double = 12.0
    @Published public var density: InterfaceDensity = .compact
    @Published public var defaultPageSize: Int = 100
    @Published public var confirmDestructiveActions: Bool = true

    // Editor
    @Published public var editorFontFamily: String = "SF Mono"
    @Published public var editorFontSize: Double = 12.5
    @Published public var tabSize: Int = 2
    @Published public var wordWrap: Bool = true
    @Published public var autocomplete: Bool = true
    @Published public var formatOnExecute: Bool = false

    // Database
    @Published public var connectionTimeoutSeconds: Int = 10
    @Published public var queryTimeoutSeconds: Int = 60
    @Published public var defaultResultLimit: Int = 500

    // Security
    @Published public var useKeychainStorage: Bool = true

    private init() {}
}
