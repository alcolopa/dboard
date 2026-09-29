import SwiftUI

public struct EnvironmentBadgeView: View {
    public let environment: ConnectionEnvironment
    @Environment(\.colorScheme) var scheme

    public init(environment: ConnectionEnvironment) {
        self.environment = environment
    }

    public var body: some View {
        HStack(spacing: 5) {
            Circle()
                .fill(Color(hex: environment.badgeColorHex))
                .frame(width: 6, height: 6)

            Text(environment.rawValue.uppercased())
                .font(ThemeTokens.uiFont(size: 9.5, weight: .bold))
                .foregroundColor(Color(hex: environment.badgeColorHex))

            if environment == .production {
                Image(systemName: "lock.shield.fill")
                    .font(.system(size: 9))
                    .foregroundColor(Color(hex: environment.badgeColorHex))
            }
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 3)
        .background(
            RoundedRectangle(cornerRadius: 4)
                .fill(Color(hex: environment.badgeColorHex).opacity(0.12))
                .overlay(
                    RoundedRectangle(cornerRadius: 4)
                        .stroke(Color(hex: environment.badgeColorHex).opacity(0.3), lineWidth: 0.8)
                )
        )
    }
}

public struct ConnectionStatusIndicatorView: View {
    public let status: ConnectionStatus
    @Environment(\.colorScheme) var scheme

    public init(status: ConnectionStatus) {
        self.status = status
    }

    public var body: some View {
        HStack(spacing: 6) {
            switch status {
            case .connected:
                Circle()
                    .fill(ThemeTokens.accentEmerald)
                    .frame(width: 7, height: 7)
                Text("Connected")
                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))

            case .connecting:
                ProgressView()
                    .scaleEffect(0.5)
                    .frame(width: 8, height: 8)
                Text("Connecting...")
                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                    .foregroundColor(ThemeTokens.accentAmber)

            case .disconnected:
                Circle()
                    .fill(ThemeTokens.textMuted(for: scheme))
                    .frame(width: 7, height: 7)
                Text("Disconnected")
                    .font(ThemeTokens.uiFont(size: 11, weight: .regular))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))

            case .error(let msg):
                Circle()
                    .fill(ThemeTokens.accentCrimson)
                    .frame(width: 7, height: 7)
                Text(msg)
                    .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                    .foregroundColor(ThemeTokens.accentCrimson)
                    .lineLimit(1)
            }
        }
    }
}

// Extension to support Hex Colors in SwiftUI
extension Color {
    public init(hex: String) {
        let cleanHex = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: cleanHex).scanHexInt64(&int)
        let a, r, g, b: UInt64
        switch cleanHex.count {
        case 3: // RGB (12-bit)
            (a, r, g, b) = (255, (int >> 8) * 17, (int >> 4 & 0xF) * 17, (int & 0xF) * 17)
        case 6: // RGB (24-bit)
            (a, r, g, b) = (255, int >> 16, int >> 8 & 0xFF, int & 0xFF)
        case 8: // ARGB (32-bit)
            (a, r, g, b) = (int >> 24, int >> 16 & 0xFF, int >> 8 & 0xFF, int & 0xFF)
        default:
            (a, r, g, b) = (255, 0, 0, 0)
        }
        self.init(
            .sRGB,
            red: Double(r) / 255,
            green: Double(g) / 255,
            blue: Double(b) / 255,
            opacity: Double(a) / 255
        )
    }
}
