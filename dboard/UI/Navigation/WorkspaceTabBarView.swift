import SwiftUI

public struct WorkspaceTabBarView: View {
    @ObservedObject var tabManager = TabManager.shared
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        HStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 1) {
                    ForEach(tabManager.openTabs) { tab in
                        let isActive = tab.id == tabManager.activeTabId
                        HStack(spacing: 7) {
                            Image(systemName: tab.iconName)
                                .font(.system(size: 11))
                                .foregroundColor(isActive ? ThemeTokens.accentBlue : ThemeTokens.textSecondary(for: scheme))

                            Text(tab.title)
                                .font(ThemeTokens.uiFont(size: 11.5, weight: isActive ? .medium : .regular))
                                .foregroundColor(isActive ? ThemeTokens.textPrimary(for: scheme) : ThemeTokens.textSecondary(for: scheme))
                                .lineLimit(1)

                            Button(action: {
                                tabManager.closeTab(id: tab.id)
                            }) {
                                Image(systemName: "xmark")
                                    .font(.system(size: 9, weight: .semibold))
                                    .foregroundColor(ThemeTokens.textMuted(for: scheme))
                                    .frame(width: 14, height: 14)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .opacity(isActive ? 0.9 : 0.4)
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(
                            isActive ? ThemeTokens.bgPrimary(for: scheme) : ThemeTokens.bgSecondary(for: scheme)
                        )
                        .overlay(
                            Rectangle()
                                .frame(height: 2)
                                .foregroundColor(isActive ? ThemeTokens.accentBlue : Color.clear),
                            alignment: .bottom
                        )
                        .overlay(
                            Rectangle()
                                .frame(width: 1)
                                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
                            alignment: .trailing
                        )
                        .contentShape(Rectangle())
                        .onTapGesture {
                            tabManager.activeTabId = tab.id
                        }
                        .contextMenu {
                            Button("Close Tab") {
                                tabManager.closeTab(id: tab.id)
                            }
                            Button("Close Other Tabs") {
                                tabManager.closeOtherTabs(exceptId: tab.id)
                            }
                            Button("Duplicate Tab") {
                                tabManager.duplicateTab(id: tab.id)
                            }
                        }
                    }
                }
            }

            // New Query / Tab Button
            Menu {
                Button(action: { tabManager.openQueryTab() }) {
                    Label("New SQL Query", systemImage: "bolt.fill")
                }
                if let driver = ConnectionManager.shared.activeDriver, driver.supportsMongoDocuments {
                    Button(action: { tabManager.openMongoTab(collection: "user_profiles") }) {
                        Label("New MongoDB Tab", systemImage: "leaf.fill")
                    }
                }
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                    .frame(width: 28, height: 26)
                    .contentShape(Rectangle())
            }
            .menuStyle(.borderlessButton)
            .padding(.horizontal, 4)

            Spacer()
        }
        .background(ThemeTokens.bgSecondary(for: scheme))
        .overlay(
            Rectangle()
                .frame(height: 1)
                .foregroundColor(ThemeTokens.borderColor(for: scheme)),
            alignment: .bottom
        )
    }
}
