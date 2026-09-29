import SwiftUI

/// Button style whose whole padded rectangle is clickable, not just the glyphs.
public struct HitAreaButtonStyle: ButtonStyle {
    public var minWidth: CGFloat? = nil
    public var minHeight: CGFloat = 22

    public init(minWidth: CGFloat? = nil, minHeight: CGFloat = 22) {
        self.minWidth = minWidth
        self.minHeight = minHeight
    }

    public func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 4)
            .frame(minWidth: minWidth, minHeight: minHeight)
            .contentShape(Rectangle())
            .opacity(configuration.isPressed ? 0.6 : 1)
    }
}

/// Dropdown that activates anywhere inside its bordered box (including the chevron area).
public struct DropdownPicker<Value: Hashable, Option: Hashable>: View {
    @Binding var selection: Value
    let options: [Option]
    let value: (Option) -> Value
    let title: (Option) -> String
    @Environment(\.colorScheme) private var scheme

    public init(selection: Binding<Value>, options: [Option],
                value: @escaping (Option) -> Value, title: @escaping (Option) -> String) {
        self._selection = selection
        self.options = options
        self.value = value
        self.title = title
    }

    public var body: some View {
        Menu {
            ForEach(options, id: \.self) { option in
                Button(title(option)) { selection = value(option) }
            }
        } label: {
            HStack {
                Text(options.first { value($0) == selection }.map(title) ?? "")
                    .font(ThemeTokens.uiFont(size: 12))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                Spacer(minLength: 4)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
            }
            .padding(.horizontal, 8)
            .frame(maxWidth: .infinity, minHeight: 24, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 5)
                    .fill(ThemeTokens.bgElevated(for: scheme))
                    .overlay(RoundedRectangle(cornerRadius: 5)
                        .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.8))
            )
            .contentShape(Rectangle())
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .buttonStyle(.plain)
    }
}

/// Bordered footer-style button with a fully clickable padded area.
public struct BoxButtonStyle: ButtonStyle {
    public var prominent = false
    @Environment(\.colorScheme) private var scheme

    public init(prominent: Bool = false) { self.prominent = prominent }

    public func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 12)
            .frame(minHeight: 26)
            .background(
                RoundedRectangle(cornerRadius: 5)
                    .fill(prominent ? Color.accentColor : ThemeTokens.bgElevated(for: scheme))
                    .overlay(RoundedRectangle(cornerRadius: 5)
                        .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: prominent ? 0 : 0.8))
            )
            .foregroundColor(prominent ? .white : nil)
            .contentShape(Rectangle())
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}
