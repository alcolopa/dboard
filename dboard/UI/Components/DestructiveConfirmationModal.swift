import SwiftUI

public struct DestructiveConfirmationModal: View {
    @Binding var isPresented: Bool
    public let title: String
    public let message: String
    public let environment: ConnectionEnvironment
    public let requiredPhrase: String?
    public let onConfirm: () -> Void

    @State private var typedPhrase: String = ""
    @Environment(\.colorScheme) var scheme

    private var canExecute: Bool {
        if let phrase = requiredPhrase, !phrase.isEmpty {
            return typedPhrase.trimmingCharacters(in: .whitespacesAndNewlines) == phrase
        }
        return true
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Image(systemName: "exclamationmark.octagon.fill")
                    .foregroundColor(ThemeTokens.accentCrimson)
                    .font(.system(size: 24))

                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(ThemeTokens.uiFont(size: 14, weight: .bold))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                    if environment == .production || environment == .staging {
                        Text("DANGER: Connected to \(environment.rawValue.uppercased())")
                            .font(ThemeTokens.uiFont(size: 11, weight: .bold))
                            .foregroundColor(ThemeTokens.accentCrimson)
                    }
                }
            }

            Text(message)
                .font(ThemeTokens.uiFont(size: 12))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .fixedSize(horizontal: false, vertical: true)

            if let phrase = requiredPhrase, !phrase.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Type \"\(phrase)\" to confirm:")
                        .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                    TextField("", text: $typedPhrase)
                        .textFieldStyle(.roundedBorder)
                        .font(ThemeTokens.codeFont(size: 12))
                }
                .padding(.top, 4)
            }

            Divider().background(ThemeTokens.borderColor(for: scheme))

            HStack {
                Spacer()

                Button("Cancel") {
                    isPresented = false
                }
                .font(ThemeTokens.uiFont(size: 12))

                Button("Confirm & Execute") {
                    onConfirm()
                    isPresented = false
                }
                .font(ThemeTokens.uiFont(size: 12, weight: .bold))
                .buttonStyle(.borderedProminent)
                .tint(ThemeTokens.accentCrimson)
                .disabled(!canExecute)
            }
        }
        .padding(20)
        .frame(width: 440)
        .background(ThemeTokens.bgElevated(for: scheme))
        .cornerRadius(10)
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 1)
        )
        .shadow(color: Color.black.opacity(0.35), radius: 24, x: 0, y: 10)
    }
}
