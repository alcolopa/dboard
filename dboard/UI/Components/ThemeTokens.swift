import SwiftUI

public struct ThemeTokens {
    public static func bgPrimary(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.08, green: 0.09, blue: 0.11) : Color(red: 0.98, green: 0.98, blue: 0.99)
    }

    public static func bgSecondary(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.11, green: 0.12, blue: 0.15) : Color(red: 0.95, green: 0.95, blue: 0.96)
    }

    public static func bgElevated(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.14, green: 0.16, blue: 0.19) : Color.white
    }

    public static func bgSidebar(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.07, green: 0.08, blue: 0.10) : Color(red: 0.94, green: 0.94, blue: 0.96)
    }

    public static func borderColor(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.18, green: 0.20, blue: 0.24) : Color(red: 0.88, green: 0.88, blue: 0.90)
    }

    public static func tableHeaderBg(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.10, green: 0.11, blue: 0.13) : Color(red: 0.92, green: 0.93, blue: 0.95)
    }

    public static func tableRowEven(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.08, green: 0.09, blue: 0.11) : Color(red: 0.99, green: 0.99, blue: 1.0)
    }

    public static func tableRowOdd(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.095, green: 0.105, blue: 0.125) : Color(red: 0.96, green: 0.97, blue: 0.98)
    }

    public static func tableRowSelected(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.12, green: 0.22, blue: 0.38) : Color(red: 0.85, green: 0.92, blue: 0.99)
    }

    public static var accentBlue: Color {
        Color(red: 0.23, green: 0.51, blue: 0.96) // Linear/VS Code blue
    }

    public static var accentEmerald: Color {
        Color(red: 0.06, green: 0.73, blue: 0.51) // Modern emerald
    }

    public static var accentAmber: Color {
        Color(red: 0.96, green: 0.62, blue: 0.04) // Amber
    }

    public static var accentCrimson: Color {
        Color(red: 0.94, green: 0.27, blue: 0.27) // Crimson
    }

    public static var accentPurple: Color {
        Color(red: 0.65, green: 0.35, blue: 0.95) // Purple for procedures
    }

    public static func textPrimary(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.92, green: 0.93, blue: 0.95) : Color(red: 0.10, green: 0.10, blue: 0.12)
    }

    public static func textSecondary(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.60, green: 0.63, blue: 0.68) : Color(red: 0.40, green: 0.42, blue: 0.46)
    }

    public static func textMuted(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.40, green: 0.43, blue: 0.48) : Color(red: 0.60, green: 0.62, blue: 0.66)
    }

    public static func codeFont(size: CGFloat = 12.0) -> Font {
        Font.system(size: size, weight: .regular, design: .monospaced)
    }

    public static func codeBoldFont(size: CGFloat = 12.0) -> Font {
        Font.system(size: size, weight: .semibold, design: .monospaced)
    }

    public static func uiFont(size: CGFloat = 12.0, weight: Font.Weight = .regular) -> Font {
        Font.system(size: size, weight: weight, design: .default)
    }
}
