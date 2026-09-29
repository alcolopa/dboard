import Foundation
import Combine

public enum TabKind: Equatable {
    case tableData(schema: String, table: String)
    case tableStructure(schema: String, table: String)
    case routine(schema: String, name: String)
    case sqlQuery(queryId: UUID)
    case mongoDocuments(collection: String)
    case mongoAggregation(collection: String)
    case settings

    public var isTableData: Bool {
        if case .tableData = self { return true }
        return false
    }

    public var isSQLQuery: Bool {
        if case .sqlQuery = self { return true }
        return false
    }

    public var isMongo: Bool {
        switch self {
        case .mongoDocuments, .mongoAggregation: return true
        default: return false
        }
    }
}

public struct WorkspaceTab: Identifiable, Equatable {
    public let id: UUID
    public var title: String
    public var iconName: String
    public var kind: TabKind
    public var isPinned: Bool
    public var queryText: String
    public var filterClause: String
    public var sortColumn: String?
    public var sortAscending: Bool
    public var page: Int
    public var pageSize: Int

    public init(
        id: UUID = UUID(),
        title: String,
        iconName: String = "tablecells",
        kind: TabKind,
        isPinned: Bool = false,
        queryText: String = "",
        filterClause: String = "",
        sortColumn: String? = nil,
        sortAscending: Bool = true,
        page: Int = 0,
        pageSize: Int = 100
    ) {
        self.id = id
        self.title = title
        self.iconName = iconName
        self.kind = kind
        self.isPinned = isPinned
        self.queryText = queryText
        self.filterClause = filterClause
        self.sortColumn = sortColumn
        self.sortAscending = sortAscending
        self.page = page
        self.pageSize = pageSize
    }
}

@MainActor
public final class TabManager: ObservableObject {
    public static let shared = TabManager()

    @Published public var openTabs: [WorkspaceTab] = []
    @Published public var activeTabId: UUID?
    @Published public var closedTabsHistory: [WorkspaceTab] = []

    private var queryCounter = 1

    private init() {
        openInitialTabs()
    }

    private func openInitialTabs() {
        let usersTab = WorkspaceTab(
            title: "users",
            iconName: "tablecells",
            kind: .tableData(schema: "public", table: "users"),
            sortColumn: "id",
            sortAscending: true
        )

        let queryTab = WorkspaceTab(
            title: "Query 1",
            iconName: "bolt.fill",
            kind: .sqlQuery(queryId: UUID()),
            queryText: "SELECT * FROM users WHERE status = 'active' ORDER BY balance DESC LIMIT 10;"
        )

        openTabs = [usersTab, queryTab]
        activeTabId = usersTab.id
    }

    public var activeTab: WorkspaceTab? {
        openTabs.first { $0.id == activeTabId }
    }

    public func openTableDataTab(schema: String, table: String) {
        if let existing = openTabs.first(where: {
            if case .tableData(let s, let t) = $0.kind {
                return s == schema && t == table
            }
            return false
        }) {
            activeTabId = existing.id
            return
        }

        let tab = WorkspaceTab(
            title: table,
            iconName: "tablecells",
            kind: .tableData(schema: schema, table: table)
        )
        openTabs.append(tab)
        activeTabId = tab.id
    }

    public func openTableStructureTab(schema: String, table: String) {
        let tab = WorkspaceTab(
            title: "\(table) (Structure)",
            iconName: "wrench.and.screwdriver",
            kind: .tableStructure(schema: schema, table: table)
        )
        openTabs.append(tab)
        activeTabId = tab.id
    }

    public func openRoutineTab(schema: String, name: String, isProcedure: Bool = false) {
        if let existing = openTabs.first(where: {
            if case .routine(let s, let n) = $0.kind {
                return s == schema && n == name
            }
            return false
        }) {
            activeTabId = existing.id
            return
        }

        let tab = WorkspaceTab(
            title: name,
            iconName: isProcedure ? "gearshape.2.fill" : "function",
            kind: .routine(schema: schema, name: name)
        )
        openTabs.append(tab)
        activeTabId = tab.id
    }

    public func openQueryTab(initialSQL: String? = nil) {
        queryCounter += 1
        let title = "Query \(queryCounter)"
        let defaultSQL = initialSQL ?? "SELECT * FROM users LIMIT 50;"
        let tab = WorkspaceTab(
            title: title,
            iconName: "bolt.fill",
            kind: .sqlQuery(queryId: UUID()),
            queryText: defaultSQL
        )
        openTabs.append(tab)
        activeTabId = tab.id
    }

    public func openMongoTab(collection: String) {
        if let existing = openTabs.first(where: {
            if case .mongoDocuments(let c) = $0.kind {
                return c == collection
            }
            return false
        }) {
            activeTabId = existing.id
            return
        }

        let tab = WorkspaceTab(
            title: collection,
            iconName: "leaf.fill",
            kind: .mongoDocuments(collection: collection)
        )
        openTabs.append(tab)
        activeTabId = tab.id
    }

    public func openMongoAggregationTab(collection: String) {
        let tab = WorkspaceTab(
            title: "\(collection) (Pipeline)",
            iconName: "arrow.triangle.merge",
            kind: .mongoAggregation(collection: collection)
        )
        openTabs.append(tab)
        activeTabId = tab.id
    }

    public func openSettingsTab() {
        if let existing = openTabs.first(where: { $0.kind == .settings }) {
            activeTabId = existing.id
            return
        }
        let tab = WorkspaceTab(
            title: "Settings",
            iconName: "gearshape",
            kind: .settings
        )
        openTabs.append(tab)
        activeTabId = tab.id
    }

    public func closeTab(id: UUID) {
        guard let index = openTabs.firstIndex(where: { $0.id == id }) else { return }
        let removed = openTabs.remove(at: index)
        closedTabsHistory.append(removed)

        if activeTabId == id {
            if openTabs.indices.contains(index) {
                activeTabId = openTabs[index].id
            } else if let last = openTabs.last {
                activeTabId = last.id
            } else {
                activeTabId = nil
            }
        }
    }

    public func duplicateTab(id: UUID) {
        guard let target = openTabs.first(where: { $0.id == id }) else { return }
        var copy = target
        copy = WorkspaceTab(
            title: "\(target.title) (Copy)",
            iconName: target.iconName,
            kind: target.kind,
            queryText: target.queryText,
            filterClause: target.filterClause,
            sortColumn: target.sortColumn,
            sortAscending: target.sortAscending,
            page: target.page,
            pageSize: target.pageSize
        )
        if let idx = openTabs.firstIndex(where: { $0.id == id }) {
            openTabs.insert(copy, at: idx + 1)
        } else {
            openTabs.append(copy)
        }
        activeTabId = copy.id
    }

    public func reopenLastClosedTab() {
        guard let lastClosed = closedTabsHistory.popLast() else { return }
        openTabs.append(lastClosed)
        activeTabId = lastClosed.id
    }

    public func closeOtherTabs(exceptId: UUID) {
        let toKeep = openTabs.filter { $0.id == exceptId || $0.isPinned }
        closedTabsHistory.append(contentsOf: openTabs.filter { $0.id != exceptId && !$0.isPinned })
        openTabs = toKeep
        activeTabId = exceptId
    }

    public func updateActiveTabQueryText(_ text: String) {
        guard let id = activeTabId, let idx = openTabs.firstIndex(where: { $0.id == id }) else { return }
        openTabs[idx].queryText = text
    }

    public func updateActiveTabFilter(_ filter: String) {
        guard let id = activeTabId, let idx = openTabs.firstIndex(where: { $0.id == id }) else { return }
        openTabs[idx].filterClause = filter
    }

    public func updateActiveTabSorting(column: String?, ascending: Bool) {
        guard let id = activeTabId, let idx = openTabs.firstIndex(where: { $0.id == id }) else { return }
        openTabs[idx].sortColumn = column
        openTabs[idx].sortAscending = ascending
    }

    public func updateActiveTabPage(_ page: Int) {
        guard let id = activeTabId, let idx = openTabs.firstIndex(where: { $0.id == id }) else { return }
        openTabs[idx].page = page
    }
}
