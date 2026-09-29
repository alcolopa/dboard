import SwiftUI

public struct SettingsView: View {
    @ObservedObject var appSettings = AppSettings.shared
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                // Header
                HStack(spacing: 8) {
                    Image(systemName: "gearshape.fill")
                        .foregroundColor(ThemeTokens.accentBlue)
                        .font(.system(size: 20))
                    Text("Preferences & Settings")
                        .font(ThemeTokens.uiFont(size: 16, weight: .bold))
                }

                Divider().background(ThemeTokens.borderColor(for: scheme))

                // General Section
                sectionTitle("General & Appearance")
                VStack(alignment: .leading, spacing: 12) {
                    HStack {
                        Text("Application Theme:")
                            .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                            .frame(width: 160, alignment: .leading)
                        Picker("", selection: $appSettings.theme) {
                            ForEach(AppTheme.allCases) { t in
                                Text(t.rawValue).tag(t)
                            }
                        }
                        .pickerStyle(.segmented)
                        .frame(width: 220)
                    }

                    HStack {
                        Text("Interface Density:")
                            .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                            .frame(width: 160, alignment: .leading)
                        Picker("", selection: $appSettings.density) {
                            ForEach(InterfaceDensity.allCases) { d in
                                Text(d.rawValue).tag(d)
                            }
                        }
                        .pickerStyle(.segmented)
                        .frame(width: 220)
                    }

                    HStack {
                        Text("Default Page Size:")
                            .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                            .frame(width: 160, alignment: .leading)
                        Picker("", selection: $appSettings.defaultPageSize) {
                            Text("50 rows").tag(50)
                            Text("100 rows").tag(100)
                            Text("250 rows").tag(250)
                            Text("500 rows").tag(500)
                        }
                        .pickerStyle(.menu)
                        .frame(width: 140)
                    }

                    Toggle("Require confirmation for destructive operations (DROP, TRUNCATE, DELETE)", isOn: $appSettings.confirmDestructiveActions)
                        .font(ThemeTokens.uiFont(size: 12))
                }

                Divider().background(ThemeTokens.borderColor(for: scheme))

                // Editor Section
                sectionTitle("SQL Editor")
                VStack(alignment: .leading, spacing: 12) {
                    HStack {
                        Text("Font Family:")
                            .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                            .frame(width: 160, alignment: .leading)
                        TextField("SF Mono", text: $appSettings.editorFontFamily)
                            .textFieldStyle(.roundedBorder)
                            .frame(width: 160)
                    }

                    HStack {
                        Text("Font Size:")
                            .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                            .frame(width: 160, alignment: .leading)
                        Slider(value: $appSettings.editorFontSize, in: 10...18, step: 0.5)
                            .frame(width: 160)
                        Text(String(format: "%.1f pt", appSettings.editorFontSize))
                            .font(ThemeTokens.codeFont(size: 11))
                    }

                    HStack {
                        Text("Tab Size:")
                            .font(ThemeTokens.uiFont(size: 12, weight: .medium))
                            .frame(width: 160, alignment: .leading)
                        Picker("", selection: $appSettings.tabSize) {
                            Text("2 spaces").tag(2)
                            Text("4 spaces").tag(4)
                        }
                        .pickerStyle(.menu)
                        .frame(width: 120)
                    }

                    Toggle("Enable SQL Autocomplete & Introspection Suggestions", isOn: $appSettings.autocomplete)
                        .font(ThemeTokens.uiFont(size: 12))

                    Toggle("Format SQL automatically before execution", isOn: $appSettings.formatOnExecute)
                        .font(ThemeTokens.uiFont(size: 12))
                }

                Divider().background(ThemeTokens.borderColor(for: scheme))

                // Security Section
                sectionTitle("Security & Credentials")
                VStack(alignment: .leading, spacing: 12) {
                    Toggle("Use macOS Keychain Services to securely store database credentials", isOn: $appSettings.useKeychainStorage)
                        .font(ThemeTokens.uiFont(size: 12))

                    Button("Clear All Saved Credentials from Keychain") {
                        KeychainManager.shared.clearAllCredentials()
                        ToastManager.shared.show("Credentials Cleared", style: .info)
                    }
                    .font(ThemeTokens.uiFont(size: 11.5))
                    .foregroundColor(ThemeTokens.accentCrimson)
                }
            }
            .padding(24)
        }
        .background(ThemeTokens.bgPrimary(for: scheme))
    }

    private func sectionTitle(_ title: String) -> some View {
        Text(title)
            .font(ThemeTokens.uiFont(size: 13, weight: .bold))
            .foregroundColor(ThemeTokens.accentBlue)
    }
}
