import SwiftUI

public struct ThemeTokens {
    public static func bgPrimary(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.105, green: 0.11, blue: 0.125) : Color(red: 1, green: 1, blue: 1)
    }

    public static func bgSecondary(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.14, green: 0.148, blue: 0.166) : Color(red: 0.955, green: 0.96, blue: 0.972)
    }

    public static func bgElevated(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.175, green: 0.185, blue: 0.208) : Color(red: 1, green: 1, blue: 1)
    }

    public static func bgSidebar(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.085, green: 0.09, blue: 0.104) : Color(red: 0.935, green: 0.942, blue: 0.958)
    }

    public static func borderColor(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.24, green: 0.252, blue: 0.282) : Color(red: 0.855, green: 0.865, blue: 0.885)
    }

    public static func tableHeaderBg(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.135, green: 0.143, blue: 0.162) : Color(red: 0.93, green: 0.938, blue: 0.955)
    }

    public static func tableRowEven(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.105, green: 0.11, blue: 0.125) : Color(red: 1, green: 1, blue: 1)
    }

    public static func tableRowOdd(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.125, green: 0.132, blue: 0.15) : Color(red: 0.972, green: 0.976, blue: 0.985)
    }

    public static func tableRowSelected(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.2, green: 0.235, blue: 0.4) : Color(red: 0.86, green: 0.885, blue: 0.995)
    }

    public static var accentBlue: Color {
        Color(red: 0.40, green: 0.47, blue: 0.98) // Indigo
    }

    public static var accentEmerald: Color {
        Color(red: 0.20, green: 0.78, blue: 0.55) // Emerald
    }

    public static var accentAmber: Color {
        Color(red: 0.98, green: 0.71, blue: 0.20) // Amber
    }

    public static var accentCrimson: Color {
        Color(red: 0.96, green: 0.36, blue: 0.40) // Coral red
    }

    public static var accentPurple: Color {
        Color(red: 0.72, green: 0.45, blue: 0.96) // Purple for procedures
    }

    public static func textPrimary(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.93, green: 0.937, blue: 0.955) : Color(red: 0.09, green: 0.1, blue: 0.13)
    }

    public static func textSecondary(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.68, green: 0.705, blue: 0.75) : Color(red: 0.32, green: 0.345, blue: 0.4)
    }

    public static func textMuted(for scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.5, green: 0.525, blue: 0.575) : Color(red: 0.47, green: 0.495, blue: 0.545)
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
