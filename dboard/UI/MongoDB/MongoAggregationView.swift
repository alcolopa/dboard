import SwiftUI

public struct AggregationStage: Identifiable {
    public let id: UUID = UUID()
    public var stageOperator: String
    public var stageBodyJSON: String
    public var isEnabled: Bool = true
}

public struct MongoAggregationView: View {
    public let collectionName: String

    @ObservedObject var connectionManager = ConnectionManager.shared
    @State private var stages: [AggregationStage] = [
        AggregationStage(stageOperator: "$match", stageBodyJSON: "{\n  \"verified\": true\n}"),
        AggregationStage(stageOperator: "$group", stageBodyJSON: "{\n  \"_id\": \"$preferences.theme\",\n  \"count\": { \"$sum\": 1 },\n  \"avgScore\": { \"$avg\": \"$score\" }\n}"),
        AggregationStage(stageOperator: "$sort", stageBodyJSON: "{\n  \"count\": -1\n}")
    ]
    @State private var isExecuting: Bool = false
    @State private var result: QueryResult? = nil
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        VResizableSplit(initialHeight: 260, minTop: 200, minBottom: 140) {
            // Pipeline Stages Builder
            VStack(alignment: .leading, spacing: 0) {
                // Header
                HStack {
                    Image(systemName: "arrow.triangle.merge")
                        .foregroundColor(ThemeTokens.accentEmerald)
                    Text("Aggregation Pipeline: \(collectionName)")
                        .font(ThemeTokens.uiFont(size: 13, weight: .bold))

                    Spacer()

                    Button("Add Stage") {
                        stages.append(AggregationStage(stageOperator: "$match", stageBodyJSON: "{}"))
                    }
                    .font(ThemeTokens.uiFont(size: 11))

                    Button(action: {
                        Task { await runPipeline() }
                    }) {
                        HStack(spacing: 4) {
                            Image(systemName: "play.fill")
                                .font(.system(size: 9))
                            Text("Run Pipeline")
                                .font(ThemeTokens.uiFont(size: 11, weight: .bold))
                        }
                        .padding(.horizontal, 9)
                        .padding(.vertical, 4)
                        .background(ThemeTokens.accentEmerald)
                        .foregroundColor(.white)
                        .cornerRadius(5)
                    }
                    .buttonStyle(.plain)
                }
                .padding(10)
                .background(ThemeTokens.bgElevated(for: scheme))

                Divider().background(ThemeTokens.borderColor(for: scheme))

                // Stage Cards
                ScrollView {
                    VStack(spacing: 8) {
                        ForEach(Array(stages.enumerated()), id: \.element.id) { index, stage in
                            stageCard(index: index, stage: stage)
                        }
                    }
                    .padding(10)
                }
            }
        } bottom: {
            // Results Panel
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    Text("Pipeline Results")
                        .font(ThemeTokens.uiFont(size: 11.5, weight: .semibold))
                        .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    Spacer()
                    if let res = result {
                        Text("\(res.rows.count) documents • \(String(format: "%.1f", res.executionDurationMs)) ms")
                            .font(ThemeTokens.codeFont(size: 10.5))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 5)
                .background(ThemeTokens.bgSecondary(for: scheme))

                Divider().background(ThemeTokens.borderColor(for: scheme))

                if let res = result {
                    QueryResultsGridView(result: res)
                } else {
                    VStack(spacing: 6) {
                        Text("Click 'Run Pipeline' to preview aggregated documents")
                            .font(ThemeTokens.uiFont(size: 12))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
        }
    }

    private func stageCard(index: Int, stage: AggregationStage) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("Stage \(index + 1)")
                    .font(ThemeTokens.codeBoldFont(size: 11))
                    .foregroundColor(ThemeTokens.textMuted(for: scheme))

                Picker("", selection: Binding(
                    get: { stage.stageOperator },
                    set: { stages[index].stageOperator = $0 }
                )) {
                    Text("$match").tag("$match")
                    Text("$group").tag("$group")
                    Text("$project").tag("$project")
                    Text("$sort").tag("$sort")
                    Text("$limit").tag("$limit")
                    Text("$unwind").tag("$unwind")
                }
                .pickerStyle(.menu)
                .frame(width: 110)

                Spacer()

                Button(action: {
                    stages.remove(at: index)
                }) {
                    Image(systemName: "trash")
                        .font(.system(size: 10))
                        .foregroundColor(ThemeTokens.accentCrimson)
                }
                .buttonStyle(.plain)
            }

            TextEditor(text: Binding(
                get: { stage.stageBodyJSON },
                set: { stages[index].stageBodyJSON = $0 }
            ))
            .font(ThemeTokens.codeFont(size: 11.5))
            .frame(height: 70)
            .padding(4)
            .background(ThemeTokens.bgPrimary(for: scheme))
            .cornerRadius(4)
        }
        .padding(8)
        .background(ThemeTokens.bgElevated(for: scheme))
        .cornerRadius(6)
        .overlay(
            RoundedRectangle(cornerRadius: 6)
                .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 0.8)
        )
    }

    private func runPipeline() async {
        guard let driver = connectionManager.activeDriver else { return }
        isExecuting = true
        let pipelineJSON = "[\n" + stages.map { "  { \"\($0.stageOperator)\": \($0.stageBodyJSON) }" }.joined(separator: ",\n") + "\n]"
        do {
            let res = try await driver.runMongoAggregation(collection: collectionName, pipelineJSON: pipelineJSON)
            self.result = res
            self.isExecuting = false
            ToastManager.shared.show("Pipeline Completed", subtitle: "\(res.rows.count) documents returned", style: .success)
        } catch {
            isExecuting = false
            ToastManager.shared.show("Pipeline Failed", subtitle: error.localizedDescription, style: .error)
        }
    }
}
