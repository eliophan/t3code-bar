import SwiftUI

public struct ThreadRowView: View {
    public let thread: ThreadItem
    public let onSelect: () -> Void
    public let showsLiveStatus: Bool
    @State private var isHovered = false

    public init(thread: ThreadItem, showsLiveStatus: Bool = true, onSelect: @escaping () -> Void) {
        self.thread = thread
        self.showsLiveStatus = showsLiveStatus
        self.onSelect = onSelect
    }

    private func formatRelativeTime(_ date: Date) -> String {
        let seconds = max(0, Int(Date().timeIntervalSince(date)))
        if seconds < 60 {
            return "now"
        }
        let minutes = seconds / 60
        if minutes < 60 {
            return "\(minutes)m"
        }
        let hours = minutes / 60
        if hours < 24 {
            return "\(hours)h"
        }
        let days = hours / 24
        return "\(days)d"
    }

    /// How long until a future time, such as a usage limit reset: "45m", "2h", "3d".
    /// Nil once the time has passed, because then there is nothing left to wait for.
    static func timeUntil(_ date: Date, now: Date = Date()) -> String? {
        let seconds = Int(date.timeIntervalSince(now).rounded(.up))
        guard seconds > 0 else { return nil }
        let minutes = max(1, (seconds + 59) / 60)
        if minutes < 60 { return "\(minutes)m" }
        let hours = (minutes + 59) / 60
        if hours < 24 { return "\(hours)h" }
        return "\((hours + 23) / 24)d"
    }

    private var limitHelp: String {
        if let reset = thread.usageLimitResetAt, let until = Self.timeUntil(reset) {
            return "Usage limit reached. Resets in \(until)."
        }
        return "Usage limit reached"
    }

    private func formatDuration(_ date: Date) -> String {
        let elapsed = max(0, Int(Date().timeIntervalSince(date)))
        if elapsed < 60 {
            return "\(elapsed)s"
        }
        let minutes = elapsed / 60
        if minutes < 60 {
            return "\(minutes)m"
        }
        let hours = minutes / 60
        let remMinutes = minutes % 60
        return "\(hours)h \(remMinutes)m"
    }

    private var hasGitBranch: Bool {
        guard let branch = thread.branch else { return false }
        let trimmed = branch.trimmingCharacters(in: .whitespacesAndNewlines)
        return !trimmed.isEmpty
    }

    private var completionDate: Date {
        thread.latestRunCompletedAt ?? thread.updatedAt
    }

    private var completionTooltip: String {
        "Done · \(completionDate.formatted(date: .abbreviated, time: .shortened))"
    }

    public var body: some View {
        Button(action: onSelect) {
            VStack(alignment: .leading, spacing: 3) {
            // Line 1: [Badge] [📌] [projectTitle] ... [Status or RelativeTime]
            HStack(alignment: .center, spacing: 6) {
                ProjectBadgeView(thread: thread)

                if thread.isPinned {
                    Image(systemName: "pin.fill")
                        .font(.system(size: 9))
                        .rotationEffect(.degrees(-45))
                        .foregroundColor(Color(hex: "#71717a"))
                }

                Text(thread.projectTitle)
                    .font(.system(size: 12.5, weight: .regular))
                    .foregroundColor(isHovered ? Color(hex: "#d1d5db") : Color(hex: "#9ca3af"))
                    .lineLimit(1)

                Spacer(minLength: 4)

                // Right slot: Working / Goal / Woke / Input / Failed, or fallback to relative time
                switch showsLiveStatus ? thread.status : .settled {
                case .connecting:
                    Text("Connecting")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(T3Theme.statusWorking)
                case .working(let startedAt):
                    HStack(spacing: 3) {
                        Text("Working")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(hex: "#38bdf8"))
                        if let started = startedAt {
                            Text(formatDuration(started))
                                .font(.system(size: 10, weight: .regular, design: .monospaced))
                                .foregroundColor(Color(hex: "#38bdf8").opacity(0.85))
                        }
                    }
                case .goal(let startedAt):
                    HStack(spacing: 3) {
                        Text("Goal")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(hex: "#38bdf8"))
                        if let started = startedAt {
                            Text(formatDuration(started))
                                .font(.system(size: 10, weight: .regular, design: .monospaced))
                                .foregroundColor(Color(hex: "#38bdf8").opacity(0.85))
                        }
                    }
                case .woke:
                    HStack(spacing: 3) {
                        Image(systemName: "alarm.fill")
                            .font(.system(size: 9))
                            .foregroundColor(Color(hex: "#f59e0b"))
                        Text("Woke")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(hex: "#f59e0b"))
                    }
                case .input:
                    Text("Input")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(Color(hex: "#a5b4fc"))
                case .approval:
                    Text("Approval")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(Color(hex: "#fcd34d"))
                case .failed:
                    HStack(spacing: 3) {
                        Image(systemName: "exclamationmark.circle")
                            .font(.system(size: 9, weight: .semibold))
                            .foregroundColor(Color(hex: "#f87171"))
                        Text("Failed")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(hex: "#f87171"))
                    }
                case .limited:
                    HStack(spacing: 3) {
                        Image(systemName: "exclamationmark.circle")
                            .font(.system(size: 9, weight: .semibold))
                            .foregroundColor(Color(hex: "#fcd34d"))
                        Text("Limited")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(hex: "#fcd34d"))
                        if let reset = thread.usageLimitResetAt, let until = Self.timeUntil(reset) {
                            Text("· \(until)")
                                .font(.system(size: 11, weight: .regular))
                                .foregroundColor(Color(hex: "#fcd34d").opacity(0.75))
                        }
                    }
                    .help(limitHelp)
                case .request:
                    Text("Request")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(T3Theme.statusRequest)
                        .help("A guest has a message waiting for you")
                case .waiting:
                    Text("Waiting")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(Color(hex: "#71717a"))
                case .planReady:
                    Text("Plan")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(Color(hex: "#c084fc"))
                case .done:
                    HStack(spacing: 4) {
                        CompletionStatusIcon()
                            .stroke(style: StrokeStyle(lineWidth: 14.0 / 12, lineCap: .round, lineJoin: .round))
                            .frame(width: 14, height: 14)
                            .accessibilityHidden(true)
                        Text("Done")
                            .font(.system(size: 11, weight: .medium))
                    }
                    .foregroundColor(T3Theme.statusDone)
                    .help(completionTooltip)
                case .settled:
                    Text(formatRelativeTime(thread.latestUserMessageAt ?? thread.updatedAt))
                        .font(.system(size: 11, weight: .regular))
                        .foregroundColor(Color(hex: "#71717a"))
                        .help(thread.latestRunStatus == "completed" && thread.latestRunCompletedAt != nil ? completionTooltip : "Last activity")
                }
            }

            if hasGitBranch {
                // Thread with Git Branch: 3-line layout
                // Line 2: [Thread Title]
                Text(thread.title)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundColor(isHovered ? Color.white : Color(hex: "#f4f4f5"))
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .padding(.top, 1)

                // Line 3: [branch] ... [Provider Icon(s)]
                HStack(alignment: .center, spacing: 6) {
                    Text(thread.branch ?? "")
                        .font(.system(size: 11, weight: .regular))
                        .foregroundColor(isHovered ? Color(hex: "#9ca3af") : Color(hex: "#71717a"))
                        .lineLimit(1)

                    Spacer()

                    ProviderIconView(providers: thread.providers, size: 13.5)
                }
                .padding(.top, 1)
            } else {
                // Thread without Git Branch: Compact 2-line layout (shorter row height like T3 Code)
                // Line 2: [Thread Title] ... [Provider Icon(s)]
                HStack(alignment: .center, spacing: 6) {
                    Text(thread.title)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundColor(isHovered ? Color.white : Color(hex: "#f4f4f5"))
                        .lineLimit(1)
                        .truncationMode(.tail)

                    Spacer(minLength: 4)

                    ProviderIconView(providers: thread.providers, size: 13.5)
                }
                .padding(.top, 1)
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(isHovered ? Color(hex: "#1e1e1e") : Color.clear)
        )
            .contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
        .buttonStyle(.plain)
        .focusable(false)
        .focusEffectDisabled()
        .onHover { hovering in
            withAnimation(.easeInOut(duration: 0.12)) {
                isHovered = hovering
            }
            if hovering {
                NSCursor.pointingHand.push()
            } else {
                NSCursor.pop()
            }
        }
        .pointingHandCursor()
        .onDisappear {
            if isHovered {
                NSCursor.pop()
                isHovered = false
            }
        }
        .contextMenu {
            Button("Open in T3 Code") {
                AppLauncher.openThread(thread: thread)
            }
            Button("Open in Browser") {
                AppLauncher.openThreadInBrowser(threadId: thread.id)
            }
            Divider()
            Button("Copy Link") {
                AppLauncher.copyThreadLink(threadId: thread.id)
            }
        }
    }
}

/// Matches the 24-point circle-check geometry used by T3 Code's sidebar.
private struct CompletionStatusIcon: Shape {
    func path(in rect: CGRect) -> Path {
        let sx = rect.width / 24
        let sy = rect.height / 24
        var path = Path()
        path.addEllipse(in: CGRect(x: rect.minX + 2 * sx, y: rect.minY + 2 * sy,
                                   width: 20 * sx, height: 20 * sy))
        path.move(to: CGPoint(x: rect.minX + 9 * sx, y: rect.minY + 12 * sy))
        path.addLine(to: CGPoint(x: rect.minX + 11 * sx, y: rect.minY + 14 * sy))
        path.addLine(to: CGPoint(x: rect.minX + 15 * sx, y: rect.minY + 10 * sy))
        return path
    }
}
