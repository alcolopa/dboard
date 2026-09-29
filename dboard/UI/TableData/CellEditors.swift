import SwiftUI

public struct JSONEditorSheet: View {
    @Binding var isPresented: Bool
    @State var jsonText: String
    public let onSave: (String) -> Void
    @State private var validationError: String? = nil
    @Environment(\.colorScheme) var scheme

    public init(isPresented: Binding<Bool>, initialJSON: String, onSave: @escaping (String) -> Void) {
        self._isPresented = isPresented
        self._jsonText = State(initialValue: initialJSON)
        self.onSave = onSave
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: "curlybraces")
                    .foregroundColor(ThemeTokens.accentBlue)
                Text("JSON Editor")
                    .font(ThemeTokens.uiFont(size: 14, weight: .bold))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                Spacer()

                Button("Format JSON") {
                    formatJSON()
                }
                .font(ThemeTokens.uiFont(size: 11))

                Button(action: { isPresented = false }) {
                    Image(systemName: "xmark")
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }
                .buttonStyle(.hit)
            }

            if let err = validationError {
                HStack(spacing: 6) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundColor(ThemeTokens.accentCrimson)
                    Text(err)
                        .font(ThemeTokens.uiFont(size: 11))
                        .foregroundColor(ThemeTokens.accentCrimson)
                }
                .padding(8)
                .background(ThemeTokens.accentCrimson.opacity(0.1))
                .cornerRadius(6)
            }

            TextEditor(text: $jsonText)
                .font(ThemeTokens.codeFont(size: 12))
                .padding(8)
                .background(ThemeTokens.bgPrimary(for: scheme))
                .cornerRadius(6)
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(validationError == nil ? ThemeTokens.borderColor(for: scheme) : ThemeTokens.accentCrimson, lineWidth: 1)
                )

            HStack {
                Spacer()

                Button("Cancel") {
                    isPresented = false
                }
                .font(ThemeTokens.uiFont(size: 12))

                Button("Save JSON (Instant Update)") {
                    if validateJSON() {
                        onSave(jsonText)
                        isPresented = false
                    }
                }
                .font(ThemeTokens.uiFont(size: 12, weight: .bold))
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(16)
        .frame(width: 520, height: 420)
        .background(ThemeTokens.bgElevated(for: scheme))
        .cornerRadius(10)
    }

    private func validateJSON() -> Bool {
        guard let data = jsonText.data(using: .utf8) else {
            validationError = "Invalid character encoding."
            return false
        }
        do {
            _ = try JSONSerialization.jsonObject(with: data, options: [])
            validationError = nil
            return true
        } catch {
            validationError = "Malformed JSON: \(error.localizedDescription)"
            return false
        }
    }

    private func formatJSON() {
        guard let data = jsonText.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data, options: []),
              let prettyData = try? JSONSerialization.data(withJSONObject: obj, options: [.prettyPrinted]),
              let formatted = String(data: prettyData, encoding: .utf8) else {
            validationError = "Cannot format: JSON is invalid."
            return
        }
        jsonText = formatted
        validationError = nil
    }
}
