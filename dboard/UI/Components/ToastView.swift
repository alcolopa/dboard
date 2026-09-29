import SwiftUI
import Combine

public enum ToastStyle {
    case info
    case success
    case warning
    case error

    public var iconName: String {
        switch self {
        case .info: return "info.circle.fill"
        case .success: return "checkmark.circle.fill"
        case .warning: return "exclamationmark.triangle.fill"
        case .error: return "xmark.octagon.fill"
        }
    }

    public var tintColor: Color {
        switch self {
        case .info: return ThemeTokens.accentBlue
        case .success: return ThemeTokens.accentEmerald
        case .warning: return ThemeTokens.accentAmber
        case .error: return ThemeTokens.accentCrimson
        }
    }
}

public struct ToastMessage: Identifiable, Equatable {
    public let id: UUID = UUID()
    public var title: String
    public var subtitle: String?
    public var style: ToastStyle

    public init(title: String, subtitle: String? = nil, style: ToastStyle = .info) {
        self.title = title
        self.subtitle = subtitle
        self.style = style
    }
}

@MainActor
public final class ToastManager: ObservableObject {
    public static let shared = ToastManager()

    @Published public var currentToast: ToastMessage?
    private var dismissWorkItem: DispatchWorkItem?

    private init() {}

    public func show(_ title: String, subtitle: String? = nil, style: ToastStyle = .info, duration: Double = 2.5) {
        dismissWorkItem?.cancel()
        currentToast = ToastMessage(title: title, subtitle: subtitle, style: style)

        let workItem = DispatchWorkItem { [weak self] in
            withAnimation(.easeInOut(duration: 0.2)) {
                self?.currentToast = nil
            }
        }
        dismissWorkItem = workItem
        DispatchQueue.main.asyncAfter(deadline: .now() + duration, execute: workItem)
    }

    public func dismiss() {
        dismissWorkItem?.cancel()
        currentToast = nil
    }
}

public struct ToastContainerView: View {
    @ObservedObject var toastManager = ToastManager.shared
    @Environment(\.colorScheme) var scheme

    public var body: some View {
        VStack {
            Spacer()
            if let toast = toastManager.currentToast {
                HStack(spacing: 10) {
                    Image(systemName: toast.style.iconName)
                        .foregroundColor(toast.style.tintColor)
                        .font(.system(size: 18, weight: .bold))

                    VStack(alignment: .leading, spacing: 2) {
                        Text(toast.title)
                            .font(ThemeTokens.uiFont(size: 12.5, weight: .medium))
                            .foregroundColor(ThemeTokens.textPrimary(for: scheme))

                        if let sub = toast.subtitle {
                            Text(sub)
                                .font(ThemeTokens.uiFont(size: 11, weight: .regular))
                                .foregroundColor(ThemeTokens.textSecondary(for: scheme))
                        }
                    }

                    Spacer(minLength: 12)

                    Button(action: { toastManager.dismiss() }) {
                        Image(systemName: "xmark")
                            .font(.system(size: 12, weight: .medium))
                            .foregroundColor(ThemeTokens.textMuted(for: scheme))
                    }
                    .buttonStyle(.hit)
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 9)
                .background(
                    RoundedRectangle(cornerRadius: 8)
                        .fill(ThemeTokens.bgElevated(for: scheme))
                        .shadow(color: Color.black.opacity(0.18), radius: 10, x: 0, y: 4)
                        .overlay(
                            RoundedRectangle(cornerRadius: 8)
                                .stroke(ThemeTokens.borderColor(for: scheme), lineWidth: 1)
                        )
                )
                .padding(.bottom, 20)
                .transition(.asymmetric(insertion: .move(edge: .bottom).combined(with: .opacity), removal: .opacity))
            }
        }
        .animation(.spring(response: 0.35, dampingFraction: 0.8), value: toastManager.currentToast)
    }
}
