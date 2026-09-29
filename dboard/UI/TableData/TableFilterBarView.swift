import SwiftUI

public struct TableFilterBarView: View {
    public let columns: [ColumnDefinition]
    @Binding var filterText: String
    public let onApplyFilter: () -> Void
    public let onClearFilter: () -> Void

    @State private var isVisualBuilderOpen: Bool = false
    @State private var selectedColumn: String = ""
    @State private var selectedOperator: String = "equals"
    @State private var filterValue: String = ""
    @Environment(\.colorScheme) var scheme

    private let operators = ["equals", "contains", "starts with", ">", "<", "IS NULL", "IS NOT NULL"]

    public var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "line.3.horizontal.decrease.circle")
                .font(.system(size: 13))
                .foregroundColor(filterText.isEmpty ? ThemeTokens.textMuted(for: scheme) : ThemeTokens.accentBlue)

            // SQL WHERE input field
            HStack(spacing: 4) {
                Text("WHERE")
                    .font(ThemeTokens.codeBoldFont(size: 10.5))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))

                TextField("e.g. status = 'active' AND balance > 100", text: $filterText, onCommit: {
                    onApplyFilter()
                })
                .textFieldStyle(.plain)
                .font(ThemeTokens.codeFont(size: 11.5))
                .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                if !filterText.isEmpty {
                    Button(action: {
                        filterText = ""
                        onClearFilter()
                    }) {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 12))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .buttonStyle(.hit)
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(ThemeTokens.bgPrimary(for: scheme))
            .cornerRadius(5)
            .overlay(
                RoundedRectangle(cornerRadius: 5)
                    .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.8)
            )

            // Visual Condition Builder Popover
            Button(action: { isVisualBuilderOpen.toggle() }) {
                HStack(spacing: 3) {
                    Image(systemName: "slider.horizontal.2.square")
                        .font(.system(size: 13))
                    Text("Builder")
                        .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                }
                .padding(.horizontal, 7)
                .padding(.vertical, 4)
                .background(ThemeTokens.bgSecondary(for: scheme))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .cornerRadius(5)
            }
            .buttonStyle(.hit)
            .popover(isPresented: $isVisualBuilderOpen) {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Visual Filter Builder")
                        .font(ThemeTokens.uiFont(size: 12, weight: .bold))

                    HStack(spacing: 6) {
                        Picker("Column", selection: $selectedColumn) {
                            ForEach(columns) { col in
                                Text(col.name).tag(col.name)
                            }
                        }
                        .pickerStyle(.menu)

                        Picker("Operator", selection: $selectedOperator) {
                            ForEach(operators, id: \.self) { op in
                                Text(op).tag(op)
                            }
                        }
                        .pickerStyle(.menu)

                        if !selectedOperator.contains("NULL") {
                            TextField("Value", text: $filterValue)
                                .textFieldStyle(.roundedBorder)
                                .frame(width: 120)
                        }
                    }

                    HStack {
                        Spacer()
                        Button("Apply Filter") {
                            applyVisualCondition()
                            isVisualBuilderOpen = false
                        }
                        .buttonStyle(.borderedProminent)
                        .font(ThemeTokens.uiFont(size: 11))
                    }
                }
                .padding(12)
                .onAppear {
                    if selectedColumn.isEmpty, let first = columns.first?.name {
                        selectedColumn = first
                    }
                }
            }

            Button(action: { onApplyFilter() }) {
                Text("Apply")
                    .font(ThemeTokens.uiFont(size: 11, weight: .semibold))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(ThemeTokens.accentBlue.opacity(0.12))
                    .foregroundColor(ThemeTokens.accentBlue)
                    .cornerRadius(5)
            }
            .buttonStyle(.hit)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(ThemeTokens.bgElevated(for: scheme))
        .overlay(
            Rectangle()
                .frame(height: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .bottom
        )
    }

    private func applyVisualCondition() {
        guard !selectedColumn.isEmpty else { return }
        switch selectedOperator {
        case "equals":
            filterText = "\"\(selectedColumn)\" = '\(filterValue)'"
        case "contains":
            filterText = "\"\(selectedColumn)\" ILIKE '%\(filterValue)%'"
        case "starts with":
            filterText = "\"\(selectedColumn)\" ILIKE '\(filterValue)%'"
        case ">", "<":
            filterText = "\"\(selectedColumn)\" \(selectedOperator) \(filterValue)"
        case "IS NULL":
            filterText = "\"\(selectedColumn)\" IS NULL"
        case "IS NOT NULL":
            filterText = "\"\(selectedColumn)\" IS NOT NULL"
        default:
            break
        }
        onApplyFilter()
    }
}
