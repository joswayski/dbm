import AppKit

/// Release highlights as the update review and What's new show them: bold
/// titles over muted descriptions, each version linking to the commit it was
/// built from. `crates/dbm-update` stacks, de-duplicates, and groups them by
/// UTC day, so this only lays them out.
enum ReleaseNotes {
    static let width: CGFloat = 400
    private static let maxHeight: CGFloat = 340

    /// The update review: every release being installed, newest first.
    static func review(_ update: [String: Any]) -> NSAttributedString {
        let text = NSMutableAttributedString()
        text.append(versionLine(string(update["version"]), commit: update["commit"] as? String, date: update["released"] as? String))
        text.append(plain("\n\nWhat’s new\n", font: Graphite.ui(10.5, .semibold), color: Graphite.faint))
        if let highlights = update["highlights"] as? [[String: Any]] {
            if highlights.isEmpty {
                text.append(plain("Behind-the-scenes fixes this time.\n", font: Graphite.ui(12), color: Graphite.secondary))
            } else {
                appendHighlights(highlights, to: text)
            }
        } else {
            let notes = string(update["notes"]).trimmingCharacters(in: .whitespacesAndNewlines)
            text.append(plain((notes.isEmpty ? "Release highlights aren’t available for this update." : notes) + "\n",
                              font: Graphite.ui(12), color: Graphite.secondary))
        }
        return trimmed(text)
    }

    /// What's new: releases grouped under their UTC publication day.
    static func days(_ days: [[String: Any]]) -> NSAttributedString {
        guard !days.isEmpty else {
            return plain("No release highlights for this version yet.", font: Graphite.ui(12), color: Graphite.secondary)
        }
        let text = NSMutableAttributedString()
        for (index, day) in days.enumerated() {
            if index > 0 { text.append(plain("\n", font: Graphite.ui(8), color: Graphite.muted)) }
            text.append(plain(string(day["date"]) + "\n", font: Graphite.ui(12), color: Graphite.muted))
            for release in day["releases"] as? [[String: Any]] ?? [] {
                text.append(versionLine(string(release["version"]), commit: release["commit"] as? String, date: nil))
                text.append(plain("\n", font: Graphite.ui(12), color: Graphite.muted))
                appendHighlights(release["highlights"] as? [[String: Any]] ?? [], to: text)
            }
        }
        return trimmed(text)
    }

    /// A scrolling, selectable text view for an alert's accessory.
    static func accessory(_ text: NSAttributedString) -> NSView {
        let view = NSTextView(frame: NSRect(x: 0, y: 0, width: width, height: 10))
        view.isEditable = false
        view.isSelectable = true
        view.drawsBackground = false
        view.textContainerInset = .zero
        view.textContainer?.lineFragmentPadding = 0
        view.textContainer?.widthTracksTextView = true
        view.isVerticallyResizable = true
        view.autoresizingMask = [.width]
        view.linkTextAttributes = [.foregroundColor: Graphite.accentText, .cursor: NSCursor.pointingHand]
        view.textStorage?.setAttributedString(text)
        var height: CGFloat = 0
        if let container = view.textContainer, let layout = view.layoutManager {
            layout.ensureLayout(for: container)
            height = ceil(layout.usedRect(for: container).height)
        }
        view.frame.size.height = height
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: width, height: min(height, maxHeight)))
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = height > maxHeight
        scroll.autohidesScrollers = true
        scroll.documentView = view
        return scroll
    }

    private static func versionLine(_ version: String, commit: String?, date: String?) -> NSAttributedString {
        let line = NSMutableAttributedString(string: "Version \(version)", attributes: [
            .font: Graphite.ui(12), .foregroundColor: Graphite.muted,
        ])
        if let commit, commit.count == 40, commit.allSatisfy(\.isHexDigit),
           let url = URL(string: "https://github.com/joswayski/dbm/commit/\(commit)") {
            line.addAttribute(.link, value: url, range: NSRange(location: 0, length: line.length))
        }
        if let date, !date.isEmpty {
            line.append(plain(" · \(date)", font: Graphite.ui(12), color: Graphite.muted))
        }
        return line
    }

    private static func appendHighlights(_ highlights: [[String: Any]], to text: NSMutableAttributedString) {
        let spaced = NSMutableParagraphStyle()
        spaced.paragraphSpacing = 8
        for highlight in highlights {
            text.append(plain(string(highlight["title"]) + "\n", font: Graphite.ui(13, .semibold), color: Graphite.textStrong))
            let body = plain(string(highlight["body"]) + "\n", font: Graphite.ui(12), color: Graphite.secondary)
            let styled = NSMutableAttributedString(attributedString: body)
            styled.addAttribute(.paragraphStyle, value: spaced, range: NSRange(location: 0, length: styled.length))
            text.append(styled)
        }
    }

    private static func plain(_ string: String, font: NSFont, color: NSColor) -> NSAttributedString {
        NSAttributedString(string: string, attributes: [.font: font, .foregroundColor: color])
    }

    private static func trimmed(_ text: NSMutableAttributedString) -> NSAttributedString {
        while text.string.hasSuffix("\n") {
            text.deleteCharacters(in: NSRange(location: text.length - 1, length: 1))
        }
        return text
    }
}
