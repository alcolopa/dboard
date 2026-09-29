import SwiftUI

public struct PaginationBarView: View {
    @Binding var currentPage: Int
    @Binding var pageSize: Int
    public let totalRows: Int
    public let executionDurationMs: Double
    public let onPageChange: (Int) -> Void
    public let onExportClick: () -> Void
    public let onInsertRowClick: () -> Void
    public var canDeleteRow: Bool = false
    public var onDeleteRowClick: () -> Void = {}
    @Environment(\.colorScheme) var scheme

    @State private var isJumpPagePopoverOpen: Bool = false
    @State private var jumpPageInput: String = ""

    private static let numberFormatter: NumberFormatter = {
        let f = NumberFormatter()
        f.numberStyle = .decimal
        return f
    }()

    private func formatNumber(_ num: Int) -> String {
        Self.numberFormatter.string(from: NSNumber(value: num)) ?? "\(num)"
    }

    private var totalPages: Int {
        max(1, Int(ceil(Double(totalRows) / Double(pageSize))))
    }

    private var startRowIndex: Int {
        totalRows == 0 ? 0 : (currentPage * pageSize) + 1
    }

    private var endRowIndex: Int {
        min(totalRows, (currentPage + 1) * pageSize)
    }

    public var body: some View {
        ViewThatFits(in: .horizontal) {
            bar(compact: false)
            bar(compact: true)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(ThemeTokens.bgElevated(for: scheme))
        .overlay(
            Rectangle()
                .frame(height: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .top
        )
    }

    private func bar(compact: Bool) -> some View {
        HStack(spacing: compact ? 6 : 12) {
            // Execution info
            HStack(spacing: 8) {
                HStack(spacing: 4) {
                    Image(systemName: "clock")
                        .font(.system(size: 15))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    Text(String(format: "%.1f ms", executionDurationMs))
                        .font(ThemeTokens.codeFont(size: 11))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                }

                Text("•")
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))

                Text(compact
                     ? "\(formatNumber(totalRows)) rows"
                     : "Showing \(formatNumber(startRowIndex))–\(formatNumber(endRowIndex)) of \(formatNumber(totalRows)) rows")
                    .font(ThemeTokens.uiFont(size: 11))
                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))

                if totalRows >= 1_000_000 {
                    Text("10M Scale Fast Catalog")
                        .font(ThemeTokens.codeBoldFont(size: 9))
                        .padding(.horizontal, 4)
                        .padding(.vertical, 1)
                        .background(ThemeTokens.accentEmerald.opacity(0.12))
                        .foregroundColor(ThemeTokens.accentEmerald)
                        .cornerRadius(3)
                }
            }

            Spacer()

            // Insert Row Button
            Button(action: onInsertRowClick) {
                HStack(spacing: 3) {
                    Image(systemName: "plus")
                        .font(.system(size: 15, weight: .bold))
                    if !compact {
                        Text("Insert Row")
                            .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                    }
                }
                .padding(.horizontal, 6)
                .padding(.vertical, 3)
                .background(ThemeTokens.bgSecondary(for: scheme))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .cornerRadius(4)
            }
            .buttonStyle(.plain)
            .help("Insert a new row")

            // Delete Row Button
            Button(action: onDeleteRowClick) {
                HStack(spacing: 3) {
                    Image(systemName: "trash")
                        .font(.system(size: 15))
                    if !compact {
                        Text("Delete Row")
                            .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                    }
                }
                .padding(.horizontal, 6)
                .padding(.vertical, 3)
                .background(ThemeTokens.bgSecondary(for: scheme))
                .foregroundColor(canDeleteRow ? ThemeTokens.accentCrimson : ThemeTokens.textMuted(for: scheme))
                .cornerRadius(4)
            }
            .buttonStyle(.plain)
            .disabled(!canDeleteRow)
            .help("Delete the selected row")

            // Export Button
            Button(action: onExportClick) {
                HStack(spacing: 3) {
                    Image(systemName: "square.and.arrow.up")
                        .font(.system(size: 15))
                    if !compact {
                        Text("Export")
                            .font(ThemeTokens.uiFont(size: 11, weight: .medium))
                    }
                }
                .padding(.horizontal, 6)
                .padding(.vertical, 3)
                .background(ThemeTokens.bgSecondary(for: scheme))
                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                .cornerRadius(4)
            }
            .buttonStyle(.plain)
            .help("Export")

            Divider()
                .frame(height: 14)
                .background(ThemeTokens.borderColor(for: scheme))

            // Page size picker
            HStack(spacing: 4) {
                if !compact {
                    Text("Page size:")
                        .font(ThemeTokens.uiFont(size: 11))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }

                Picker("", selection: $pageSize) {
                    Text("25").tag(25)
                    Text("50").tag(50)
                    Text("100").tag(100)
                    Text("500").tag(500)
                    Text("1000").tag(1000)
                }
                .pickerStyle(.menu)
                .frame(width: 70)
                .onChange(of: pageSize) { _, _ in
                    currentPage = 0
                    onPageChange(0)
                }
            }

            // Pagination Controls
            HStack(spacing: 2) {
                Button(action: {
                    currentPage = 0
                    onPageChange(0)
                }) {
                    Image(systemName: "backward.end.fill")
                        .font(.system(size: 14))
                        .frame(width: 28, height: 28)
                }
                .buttonStyle(.plain)
                .disabled(currentPage == 0)
                .help("First Page")

                Button(action: {
                    if currentPage > 0 {
                        currentPage -= 1
                        onPageChange(currentPage)
                    }
                }) {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 14, weight: .semibold))
                        .frame(width: 28, height: 28)
                }
                .buttonStyle(.plain)
                .disabled(currentPage == 0)
                .help("Previous Page")

                // Page Number with click to Jump
                Button(action: {
                    jumpPageInput = "\(currentPage + 1)"
                    isJumpPagePopoverOpen = true
                }) {
                    Text("Page \(formatNumber(currentPage + 1)) of \(formatNumber(totalPages))")
                        .font(ThemeTokens.codeFont(size: 11))
                        .foregroundColor(ThemeTokens.accentBlue)
                        .padding(.horizontal, 4)
                        .padding(.vertical, 2)
                        .background(ThemeTokens.accentBlue.opacity(0.08))
                        .cornerRadius(3)
                }
                .buttonStyle(.plain)
                .help("Click to jump to any page or row")
                .popover(isPresented: $isJumpPagePopoverOpen) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Jump to Page")
                            .font(ThemeTokens.uiFont(size: 12, weight: .bold))

                        HStack {
                            TextField("Page 1–\(totalPages)", text: $jumpPageInput)
                                .textFieldStyle(.roundedBorder)
                                .frame(width: 110)

                            Button("Go") {
                                if let targetPage = Int(jumpPageInput.replacingOccurrences(of: ",", with: "")),
                                   targetPage >= 1 && targetPage <= totalPages {
                                    currentPage = targetPage - 1
                                    onPageChange(currentPage)
                                    isJumpPagePopoverOpen = false
                                }
                            }
                            .buttonStyle(.borderedProminent)
                        }

                        Text("Total pages: \(formatNumber(totalPages)) (\(formatNumber(totalRows)) rows)")
                            .font(ThemeTokens.uiFont(size: 10))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .padding(12)
                }

                Button(action: {
                    if currentPage < totalPages - 1 {
                        currentPage += 1
                        onPageChange(currentPage)
                    }
                }) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 14, weight: .semibold))
                        .frame(width: 28, height: 28)
                }
                .buttonStyle(.plain)
                .disabled(currentPage >= totalPages - 1)
                .help("Next Page")

                Button(action: {
                    currentPage = totalPages - 1
                    onPageChange(totalPages - 1)
                }) {
                    Image(systemName: "forward.end.fill")
                        .font(.system(size: 14))
                        .frame(width: 28, height: 28)
                }
                .buttonStyle(.plain)
                .disabled(currentPage >= totalPages - 1)
                .help("Last Page")
            }
            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
        }
        .lineLimit(1)
    }
}
