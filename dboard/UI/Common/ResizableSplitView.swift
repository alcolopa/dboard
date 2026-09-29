import SwiftUI
#if canImport(AppKit)
import AppKit
#endif

// MARK: - Lightweight resizable split containers
//
// `HSplitView`/`VSplitView` bridge to AppKit's `NSSplitView`, which re-runs a
// full layout pass over every child on each intermediate frame of a window
// resize. During the macOS fullscreen animation this fires dozens of times a
// second, and on macOS 26+ the cost of that repeated bridged layout became
// visibly worse, producing UI stutter even with a small amount of on-screen
// content. These containers do the same "draggable divider" job with plain
// SwiftUI (HStack/VStack + a drag gesture), so a resize/fullscreen animation
// only has to reposition frames instead of re-laying out through AppKit.

private struct SplitDividerHandle: View {
    let axis: Axis
    let onDrag: (CGFloat) -> Void
    let onDragEnded: () -> Void

    var body: some View {
        Rectangle()
            .fill(Color.primary.opacity(0.08))
            .frame(width: axis == .horizontal ? 1 : nil,
                   height: axis == .vertical ? 1 : nil)
            .overlay(
                Rectangle()
                    .fill(Color.clear)
                    .frame(width: axis == .horizontal ? 8 : nil,
                           height: axis == .vertical ? 8 : nil)
                    .contentShape(Rectangle())
                    .gesture(
                        DragGesture(minimumDistance: 0)
                            .onChanged { value in
                                onDrag(axis == .horizontal ? value.translation.width : value.translation.height)
                            }
                            .onEnded { _ in onDragEnded() }
                    )
                    #if os(macOS)
                    .onHover { inside in
                        if inside {
                            (axis == .horizontal ? NSCursor.resizeLeftRight : NSCursor.resizeUpDown).push()
                        } else {
                            NSCursor.pop()
                        }
                    }
                    #endif
            )
    }
}

/// Horizontal split with a resizable leading (left) panel and a flexible trailing panel.
public struct HResizableSplit<Leading: View, Trailing: View>: View {
    @State private var leadingSize: CGFloat
    @State private var dragStartSize: CGFloat? = nil
    private let minLeading: CGFloat
    private let maxLeading: CGFloat
    private let leading: Leading
    private let trailing: Trailing

    public init(
        initialWidth: CGFloat = 260,
        minLeading: CGFloat = 180,
        maxLeading: CGFloat = 480,
        @ViewBuilder leading: () -> Leading,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self._leadingSize = State(initialValue: initialWidth)
        self.minLeading = minLeading
        self.maxLeading = maxLeading
        self.leading = leading()
        self.trailing = trailing()
    }

    public var body: some View {
        HStack(spacing: 0) {
            leading.frame(width: leadingSize)

            SplitDividerHandle(axis: .horizontal, onDrag: { delta in
                let start = dragStartSize ?? leadingSize
                if dragStartSize == nil { dragStartSize = leadingSize }
                leadingSize = min(max(start + delta, minLeading), maxLeading)
            }, onDragEnded: {
                dragStartSize = nil
            })

            trailing.frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

/// Horizontal split with a flexible leading panel and a resizable trailing (right) panel.
public struct HResizableTrailingSplit<Leading: View, Trailing: View>: View {
    @State private var trailingSize: CGFloat
    @State private var dragStartSize: CGFloat? = nil
    private let minTrailing: CGFloat
    private let maxTrailing: CGFloat
    private let leading: Leading
    private let trailing: Trailing

    public init(
        initialWidth: CGFloat = 280,
        minTrailing: CGFloat = 220,
        maxTrailing: CGFloat = 520,
        @ViewBuilder leading: () -> Leading,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self._trailingSize = State(initialValue: initialWidth)
        self.minTrailing = minTrailing
        self.maxTrailing = maxTrailing
        self.leading = leading()
        self.trailing = trailing()
    }

    public var body: some View {
        HStack(spacing: 0) {
            leading.frame(maxWidth: .infinity, maxHeight: .infinity)

            SplitDividerHandle(axis: .horizontal, onDrag: { delta in
                let start = dragStartSize ?? trailingSize
                if dragStartSize == nil { dragStartSize = trailingSize }
                trailingSize = min(max(start - delta, minTrailing), maxTrailing)
            }, onDragEnded: {
                dragStartSize = nil
            })

            trailing.frame(width: trailingSize)
        }
    }
}

/// Vertical split with a resizable top panel and a flexible bottom panel.
public struct VResizableSplit<Top: View, Bottom: View>: View {
    @State private var topSize: CGFloat
    @State private var dragStartSize: CGFloat? = nil
    private let minTop: CGFloat
    private let minBottom: CGFloat
    private let top: Top
    private let bottom: Bottom

    public init(
        initialHeight: CGFloat = 220,
        minTop: CGFloat = 120,
        minBottom: CGFloat = 140,
        @ViewBuilder top: () -> Top,
        @ViewBuilder bottom: () -> Bottom
    ) {
        self._topSize = State(initialValue: initialHeight)
        self.minTop = minTop
        self.minBottom = minBottom
        self.top = top()
        self.bottom = bottom()
    }

    public var body: some View {
        GeometryReader { geo in
            VStack(spacing: 0) {
                top.frame(height: min(topSize, max(geo.size.height - minBottom, minTop)))

                SplitDividerHandle(axis: .vertical, onDrag: { delta in
                    let start = dragStartSize ?? topSize
                    if dragStartSize == nil { dragStartSize = topSize }
                    let maxTop = max(geo.size.height - minBottom, minTop)
                    topSize = min(max(start + delta, minTop), maxTop)
                }, onDragEnded: {
                    dragStartSize = nil
                })

                bottom.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }
}
