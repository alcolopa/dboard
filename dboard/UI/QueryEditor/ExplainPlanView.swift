import SwiftUI

public struct ExplainPlanView: View {
    public let rootPlan: ExplainPlanNode
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Image(systemName: "chart.bar.doc.horizontal")
                        .foregroundColor(ThemeTokens.accentBlue)
                    Text("Query Execution Plan (EXPLAIN ANALYZE)")
                        .font(ThemeTokens.uiFont(size: 13, weight: .bold))
                        .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                    Spacer()

                    Text("Total Cost: \(String(format: "%.2f", rootPlan.totalCost))")
                        .font(ThemeTokens.codeBoldFont(size: 11))
                        .foregroundColor(ThemeTokens.accentAmber)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(ThemeTokens.accentAmber.opacity(0.12))
                        .cornerRadius(4)
                }
                .padding(.bottom, 4)

                Divider().background(ThemeTokens.borderColor(for: scheme))

                PlanNodeCardView(node: rootPlan, depth: 0)
            }
            .padding(14)
        }
        .background(ThemeTokens.bgPrimary(for: scheme))
    }
}

public struct PlanNodeCardView: View {
    public let node: ExplainPlanNode
    public let depth: Int
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                // Indentation
                if depth > 0 {
                    HStack(spacing: 2) {
                        ForEach(0..<depth, id: \.self) { _ in
                            Rectangle()
                                .fill(ThemeTokens.borderColor(for: scheme))
                                .frame(width: 2, height: 16)
                                .padding(.horizontal, 4)
                        }
                    }
                }

                Image(systemName: iconForNodeType(node.nodeType))
                    .font(.system(size: 13))
                    .foregroundColor(colorForNodeType(node.nodeType))

                Text(node.nodeType)
                    .font(ThemeTokens.codeBoldFont(size: 12))
                    .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                if let rel = node.relationName {
                    Text("on \"\(rel)\"")
                        .font(ThemeTokens.codeFont(size: 11.5))
                        .foregroundColor(ThemeTokens.accentBlue)
                }

                if let idx = node.indexName {
                    Text("using [\(idx)]")
                        .font(ThemeTokens.codeFont(size: 10.5))
                        .foregroundColor(ThemeTokens.accentEmerald)
                }

                Spacer()

                // Metrics
                HStack(spacing: 8) {
                    if let actualTime = node.actualTotalTime {
                        Text(String(format: "%.3f ms", actualTime))
                            .font(ThemeTokens.codeFont(size: 10.5))
                            .foregroundColor(ThemeTokens.textPrimary(for: scheme))
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(ThemeTokens.bgSecondary(for: scheme))
                            .cornerRadius(3)
                    }

                    if let actualRows = node.actualRows {
                        Text("\(actualRows) rows")
                            .font(ThemeTokens.codeFont(size: 10.5))
                            .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    }

                    Text("cost: \(String(format: "%.1f..%.1f", node.startupCost, node.totalCost))")
                        .font(ThemeTokens.codeFont(size: 10))
                        .foregroundColor(ThemeTokens.textMuted(for: scheme))
                }
            }

            if let filter = node.filter {
                HStack(spacing: 4) {
                    Text("Filter:")
                        .font(ThemeTokens.codeBoldFont(size: 10))
                        .foregroundColor(ThemeTokens.accentAmber)
                    Text(filter)
                        .font(ThemeTokens.codeFont(size: 10.5))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                }
                .padding(.leading, CGFloat(depth * 16 + 24))
            }

            // Child nodes
            ForEach(node.children) { child in
                PlanNodeCardView(node: child, depth: depth + 1)
            }
        }
        .padding(8)
        .background(
            RoundedRectangle(cornerRadius: 6)
                .fill(ThemeTokens.bgElevated(for: scheme))
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.8)
                )
        )
    }

    private func iconForNodeType(_ type: String) -> String {
        if type.contains("Index") { return "key.fill" }
        if type.contains("Scan") { return "magnifyingglass" }
        if type.contains("Join") { return "arrow.triangle.merge" }
        if type.contains("Sort") { return "arrow.up.arrow.down" }
        return "gearshape.fill"
    }

    private func colorForNodeType(_ type: String) -> Color {
        if type.contains("Index") { return ThemeTokens.accentEmerald }
        if type.contains("Seq Scan") { return ThemeTokens.accentAmber }
        if type.contains("Join") { return ThemeTokens.accentBlue }
        return ThemeTokens.textSecondary(for: scheme)
    }
}
