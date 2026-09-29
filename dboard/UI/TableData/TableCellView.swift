import SwiftUI

public struct TableCellView: View, Equatable {
    public let column: ColumnDefinition
    public let value: DataValue
    public let isSelected: Bool
    public let isReadOnly: Bool
    public let onRequestJSONEdit: ((String) -> Void)?
    public let onCommit: (DataValue) async throws -> Void
    public let onSelect: () -> Void

    @State private var isEditing: Bool = false
    @State private var editText: String = ""
    @State private var syncStatus: CellEditStatus = .idle
    @State private var showErrorPopover: Bool = false
    @Environment(\.colorScheme) var scheme

    public init(
        column: ColumnDefinition,
        value: DataValue,
        isSelected: Bool = false,
        isReadOnly: Bool = false,
        onRequestJSONEdit: ((String) -> Void)? = nil,
        onCommit: @escaping (DataValue) async throws -> Void,
        onSelect: @escaping () -> Void
    ) {
        self.column = column
        self.value = value
        self.isSelected = isSelected
        self.isReadOnly = isReadOnly
        self.onRequestJSONEdit = onRequestJSONEdit
        self.onCommit = onCommit
        self.onSelect = onSelect
    }

    public static func == (lhs: TableCellView, rhs: TableCellView) -> Bool {
        lhs.column == rhs.column &&
        lhs.value == rhs.value &&
        lhs.isSelected == rhs.isSelected &&
        lhs.isReadOnly == rhs.isReadOnly
    }

    public var body: some View {
        ZStack(alignment: .leading) {
            // Normal display or edit view
            if isEditing && !isReadOnly {
                HStack(spacing: 4) {
                    TextField("", text: $editText, onCommit: {
                        finishEdit()
                    })
                    .textFieldStyle(.plain)
                    .font(ThemeTokens.codeFont(size: 11.5))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                    // Instant Set NULL button
                    Button(action: {
                        editText = "NULL"
                        finishEdit()
                    }) {
                        Text("NULL")
                            .font(ThemeTokens.codeBoldFont(size: 9))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                            .padding(.horizontal, 4)
                            .padding(.vertical, 1)
                            .background(ThemeTokens.bgSecondary(for: scheme))
                            .cornerRadius(3)
                    }
                    .buttonStyle(.plain)
                    .help("Set value to NULL")
                }
                .padding(.horizontal, 6)
                .background(ThemeTokens.bgElevated(for: scheme))
            } else {
                // Read-only / formatted display
                HStack(spacing: 6) {
                    cellContentDisplay

                    Spacer(minLength: 2)

                    // Inline Sync Indicator
                    syncIndicatorView
                }
                .padding(.horizontal, 6)
                .contentShape(Rectangle())
                .onTapGesture(count: 2) {
                    if !isReadOnly {
                        startEdit()
                    }
                }
                .onTapGesture(count: 1) {
                    onSelect()
                }
            }
        }
        .frame(height: 26)
        .overlay(
            // Focus / Selection Border
            Rectangle()
                .stroke(
                    borderColorForStatus,
                    lineWidth: isSelected || isEditing ? 1.5 : 0.5
                )
        )
    }

    @ViewBuilder
    private var cellContentDisplay: some View {
        switch value {
        case .null:
            Text("NULL")
                .font(ThemeTokens.uiFont(size: 10.5, weight: .semibold))
                .italic()
                .foregroundColor(ThemeTokens.textMuted(for: scheme))
                .padding(.horizontal, 4)
                .padding(.vertical, 1)
                .background(ThemeTokens.bgSecondary(for: scheme).opacity(0.8))
                .cornerRadius(3)

        case .boolean(let b):
            HStack(spacing: 4) {
                Circle()
                    .fill(b ? ThemeTokens.accentEmerald : ThemeTokens.accentCrimson)
                    .frame(width: 6, height: 6)
                Text(b ? "TRUE" : "FALSE")
                    .font(ThemeTokens.codeBoldFont(size: 10.5))
                    .foregroundColor(b ? ThemeTokens.accentEmerald : ThemeTokens.accentCrimson)
            }
            .contentShape(Rectangle())
            .onTapGesture {
                if !isReadOnly {
                    // Quick toggle boolean on click!
                    Task {
                        await executeInstantUpdate(newValue: .boolean(!b))
                    }
                }
            }

        case .json:
            HStack(spacing: 4) {
                Text("{ JSON }")
                    .font(ThemeTokens.codeBoldFont(size: 10))
                    .foregroundColor(ThemeTokens.accentBlue)
                    .padding(.horizontal, 4)
                    .padding(.vertical, 1)
                    .background(ThemeTokens.accentBlue.opacity(0.12))
                    .cornerRadius(3)

                Text(value.displayText)
                    .font(ThemeTokens.codeFont(size: 11))
                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    .lineLimit(1)
            }
            .contentShape(Rectangle())
            .onTapGesture {
                if !isReadOnly {
                    onRequestJSONEdit?(value.rawStringValue)
                }
            }

        default:
            Text(value.displayText)
                .font(value.isNumeric ? ThemeTokens.codeFont(size: 11.5) : ThemeTokens.uiFont(size: 11.5))
                .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                .lineLimit(1)
        }
    }

    @ViewBuilder
    private var syncIndicatorView: some View {
        switch syncStatus {
        case .saving:
            ProgressView()
                .scaleEffect(0.45)
                .frame(width: 12, height: 12)
                .help("Saving edit directly to database...")

        case .saved:
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 10))
                .foregroundColor(ThemeTokens.accentEmerald)
                .transition(.opacity)

        case .error(let msg):
            Button(action: { showErrorPopover.toggle() }) {
                Image(systemName: "exclamationmark.circle.fill")
                    .font(.system(size: 11))
                    .foregroundColor(ThemeTokens.accentCrimson)
            }
            .buttonStyle(.plain)
            .popover(isPresented: $showErrorPopover) {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Database Update Failed")
                        .font(ThemeTokens.uiFont(size: 12, weight: .bold))
                        .foregroundColor(ThemeTokens.accentCrimson)

                    Text(msg)
                        .font(ThemeTokens.uiFont(size: 11))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                    HStack {
                        Button("Revert to Original") {
                            editText = value.rawStringValue
                            syncStatus = .idle
                            showErrorPopover = false
                        }
                        .font(ThemeTokens.uiFont(size: 11))

                        Spacer()

                        Button("Retry Update") {
                            showErrorPopover = false
                            finishEdit()
                        }
                        .font(ThemeTokens.uiFont(size: 11, weight: .semibold))
                    }
                }
                .padding(12)
                .frame(width: 280)
            }

        case .idle:
            EmptyView()
        }
    }

    private var borderColorForStatus: Color {
        switch syncStatus {
        case .saving: return ThemeTokens.accentBlue
        case .saved: return ThemeTokens.accentEmerald
        case .error: return ThemeTokens.accentCrimson
        case .idle:
            return isSelected ? ThemeTokens.accentBlue : ThemeTokens.borderColor(for: scheme).opacity(0.6)
        }
    }

    private func startEdit() {
        editText = value.isNull ? "" : value.rawStringValue
        isEditing = true
    }

    private func finishEdit() {
        isEditing = false
        let parsed = DataValue.parseFromInput(editText, targetType: column.dataTypeName)
        if parsed == value {
            return // No change
        }

        Task {
            await executeInstantUpdate(newValue: parsed)
        }
    }

    private func executeInstantUpdate(newValue: DataValue) async {
        syncStatus = .saving
        do {
            try await onCommit(newValue)
            withAnimation {
                syncStatus = .saved
            }
            // Auto fade saved status after 1.2s
            try? await Task.sleep(nanoseconds: 1_200_000_000)
            withAnimation {
                if syncStatus == .saved {
                    syncStatus = .idle
                }
            }
        } catch {
            withAnimation {
                syncStatus = .error(message: error.localizedDescription)
            }
        }
    }
}
