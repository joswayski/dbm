import AppKit

final class ColorSwatch: RowControl {
    let hex: String

    init(hex: String) {
        self.hex = hex
        super.init(frame: .zero)
        // 22 pt dot plus room for the selection ring.
        widthAnchor.constraint(equalToConstant: 30).isActive = true
        heightAnchor.constraint(equalToConstant: 30).isActive = true
        setAccessibilityLabel("Use connection color \(hex)")
        toolTip = "Use connection color \(hex)"
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func hoverChanged() { needsDisplay = true }

    /// `.color-swatch`: a hairline inner ring, grown 8% on hover; selected
    /// swatches get a chrome gap and a text-coloured outer ring.
    override func draw(_ dirtyRect: NSRect) {
        let inset: CGFloat = hovering && !selected ? 3.1 : 4
        let dot = bounds.insetBy(dx: inset, dy: inset)
        (NSColor(css: hex) ?? Graphite.accent).setFill()
        NSBezierPath(ovalIn: dot).fill()
        NSColor(white: 1, alpha: 0.12).setStroke()
        let hairline = NSBezierPath(ovalIn: dot.insetBy(dx: 0.5, dy: 0.5))
        hairline.lineWidth = 1
        hairline.stroke()
        if selected {
            Graphite.text.setStroke()
            let ring = NSBezierPath(ovalIn: bounds.insetBy(dx: 1, dy: 1))
            ring.lineWidth = 2
            ring.stroke()
        }
    }
}

/// Segmented engine picker: control track, raised selected segment.
final class EngineSegment: PanelView {
    private var buttons: [GButton] = []
    var onSelect: ((Engine) -> Void)?
    var selectedEngine = Engine.postgres { didSet { update() } }

    init() {
        super.init(fill: Graphite.control)
        radius = 7
        let row = hstack([], spacing: 2)
        for engine in [Engine.postgres, .mysql, .redis] {
            let button = GButton(engine.label, style: .toolbar) { [weak self] in self?.onSelect?(engine) }
            button.widthAnchor.constraint(equalToConstant: 120).isActive = true
            button.minHeight = 26
            buttons.append(button)
            row.addArrangedSubview(button)
        }
        pin(row, insets: NSEdgeInsets(top: 2, left: 2, bottom: 2, right: 2))
        setAccessibilityElement(true)
        setAccessibilityRole(.radioGroup)
        setAccessibilityLabel("Database engine")
        update()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    private func update() {
        for (button, engine) in zip(buttons, [Engine.postgres, .mysql, .redis]) {
            button.isOn = engine == selectedEngine
            button.style = engine == selectedEngine ? .secondary : .toolbar
        }
    }
}

/// New/Edit connection, mirroring `ProfileModal` in `App.tsx`.
final class ProfileSheet: NSWindowController {
    private let original: Profile?
    private var engine: Engine
    private var color: String
    private let onTest: ([String: Any], @escaping (Error?) -> Void) -> Void
    private let onSave: ([String: Any], @escaping (Error?) -> Void) -> Void
    private let onDelete: (() -> Void)?

    private let eyebrowLabel = eyebrow("")
    private let engineSegment = EngineSegment()
    private let urlField = GSecureField("", placeholder: "")
    private let nameField = GTextField("")
    private let hostField = GTextField("")
    private let portField = GTextField("")
    private let usernameField = GTextField("")
    private let databaseField = GTextField("")
    private let passwordField = GSecureField("", placeholder: "")
    private let tlsPopup = GPopUp(items: ["Preferred", "Required", "Disabled"])
    private let caField = GTextField("", placeholder: "/path/to/root-ca.pem")
    private let readOnlyBox = NSButton(checkboxWithTitle: "Read-only profile (blocks GUI edits and mutations)", target: nil, action: nil)
    private let usernameLabel = label("", font: Graphite.ui(12), color: Graphite.muted)
    private let databaseLabel = label("", font: Graphite.ui(12), color: Graphite.muted)
    private let feedback = label("", font: Graphite.ui(12), color: Graphite.accentText)
    private let feedbackBox = PanelView(fill: Graphite.accentSoft)
    private var swatches: [ColorSwatch] = []
    private let customWell = NSColorWell(frame: NSRect(x: 0, y: 0, width: 26, height: 22))
    private lazy var testButton = GButton("Test connection", style: .secondary) { [weak self] in self?.test() }
    private lazy var saveButton = GButton("Save & connect", style: .primary) { [weak self] in self?.save() }
    private var busy = false
    private var deleteButton: GButton?

    init(profile: Profile?, onTest: @escaping ([String: Any], @escaping (Error?) -> Void) -> Void,
         onSave: @escaping ([String: Any], @escaping (Error?) -> Void) -> Void, onDelete: (() -> Void)?) {
        original = profile
        engine = profile?.engine ?? .postgres
        color = profile?.colorHex ?? Graphite.defaultConnectionColor
        self.onTest = onTest
        self.onSave = onSave
        self.onDelete = onDelete
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 560, height: 640), styleMask: [.titled], backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: .darkAqua)
        window.backgroundColor = Graphite.popover
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        super.init(window: window)
        build()
        fill()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    private func field(_ title: String, _ view: NSView, fill: Bool = true) -> NSStackView {
        let stack = vstack([label(title, font: Graphite.ui(12), color: Graphite.muted), view], spacing: 5)
        if fill { view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true }
        return stack
    }

    private func labeledField(_ caption: NSTextField, _ view: NSView) -> NSStackView {
        let stack = vstack([caption, view], spacing: 5)
        view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        return stack
    }

    private func pair(_ left: NSView, _ right: NSView) -> NSStackView {
        let row = hstack([left, right], spacing: 12)
        row.alignment = .top
        row.distribution = .fillEqually
        return row
    }

    private func build() {
        guard let content = window?.contentView else { return }
        let title = label(original == nil ? "New connection" : "Edit connection", font: Graphite.ui(17, .semibold), color: Graphite.textStrong)
        let close = GButton("", icon: .close, style: .icon, tooltip: "Close") { [weak self] in self?.dismiss() }
        let header = hstack([vstack([eyebrowLabel, title], spacing: 3), spacer(), close])
        header.alignment = .top

        engineSegment.onSelect = { [weak self] engine in self?.setEngine(engine) }
        let importButton = GButton("Import URL", style: .secondary) { [weak self] in self?.importURL() }
        importButton.isEnabled = false
        // Return imports; pasting a URL imports it straight away.
        urlField.behavior.onReturn = { [weak self] in self?.importURL() }
        var previousURL = ""
        urlField.onChange = { [weak self, weak importButton] text in
            importButton?.isEnabled = !text.trimmingCharacters(in: .whitespaces).isEmpty
            let pasted = NSApp.currentEvent?.modifierFlags.contains(.command) == true
                && NSApp.currentEvent?.charactersIgnoringModifiers == "v"
            if pasted || (text.count - previousURL.count > 1 && text.contains("://")) { self?.importURL() }
            previousURL = text
        }
        // Return anywhere else saves, like submitting the desktop form.
        for field in [nameField, hostField, portField, usernameField, databaseField, caField] {
            field.behavior.onReturn = { [weak self] in self?.save() }
            field.onCancel = { [weak self] in self?.dismiss() }
        }
        passwordField.behavior.onReturn = { [weak self] in self?.save() }
        passwordField.behavior.onCancel = { [weak self] in self?.dismiss() }
        urlField.behavior.onCancel = { [weak self] in self?.dismiss() }
        let urlRow = hstack([urlField, importButton], spacing: 8)

        let colorRow = hstack([], spacing: 0)
        for hex in Graphite.connectionColors {
            let swatch = ColorSwatch(hex: hex)
            swatch.onClick = { [weak self] in self?.setColor(hex) }
            swatches.append(swatch)
            colorRow.addArrangedSubview(swatch)
        }
        customWell.translatesAutoresizingMaskIntoConstraints = false
        customWell.widthAnchor.constraint(equalToConstant: 26).isActive = true
        customWell.heightAnchor.constraint(equalToConstant: 26).isActive = true
        customWell.target = self
        customWell.action = #selector(customColor)
        customWell.toolTip = "Choose a custom color"
        colorRow.addArrangedSubview(customWell)
        colorRow.setCustomSpacing(8, after: swatches.last!)
        colorRow.setCustomSpacing(6, after: customWell)
        colorRow.addArrangedSubview(label("Custom", font: Graphite.ui(12.5), color: Graphite.muted))

        readOnlyBox.attributedTitle = NSAttributedString(string: readOnlyBox.title, attributes: [.font: Graphite.ui(12.5), .foregroundColor: Graphite.text])
        feedback.maximumNumberOfLines = 6
        feedback.lineBreakMode = .byWordWrapping
        feedback.preferredMaxLayoutWidth = 494
        // `.modal-feedback`: a tinted, outlined status box.
        feedbackBox.radius = 7
        feedbackBox.pin(feedback, insets: NSEdgeInsets(top: 9, left: 12, bottom: 9, right: 12))
        feedbackBox.isHidden = true
        let note = label("Passwords are stored in your operating system credential manager and are never written to DBM's profile database.",
                         font: Graphite.ui(11.5), color: Graphite.faint)
        note.maximumNumberOfLines = 2
        note.lineBreakMode = .byWordWrapping
        note.preferredMaxLayoutWidth = 520

        var actions: [NSView] = []
        if onDelete != nil {
            let delete = GButton("Delete", style: .danger) { [weak self] in self?.delete() }
            deleteButton = delete
            actions.append(delete)
        }
        actions += [spacer(), testButton, saveButton]
        let form = vstack([
            header,
            field("Database engine", engineSegment, fill: false),
            field("Connection URL", urlRow),
            field("Name", nameField),
            field("Connection color", colorRow, fill: false),
            pair(field("Host", hostField), field("Port", portField)),
            pair(labeledField(usernameLabel, usernameField), labeledField(databaseLabel, databaseField)),
            pair(field("Password", passwordField), field("TLS", tlsPopup)),
            field("CA certificate path (optional)", caField),
            readOnlyBox, feedbackBox, note, hstack(actions, spacing: 8),
        ], spacing: 12)
        form.setCustomSpacing(16, after: header)
        for view in form.arrangedSubviews where !(view === readOnlyBox) {
            view.widthAnchor.constraint(equalTo: form.widthAnchor).isActive = true
        }
        let background = PanelView(fill: Graphite.popover)
        content.addSubview(background)
        content.pin(background)
        background.pin(form, insets: NSEdgeInsets(top: 18, left: 20, bottom: 18, right: 20))
        window?.initialFirstResponder = nameField
    }

    private func fill() {
        eyebrowLabel.stringValue = engine.label.uppercased()
        engineSegment.selectedEngine = engine
        urlField.placeholderAttributedString = NSAttributedString(string: engine.urlPlaceholder, attributes: [.font: Graphite.ui(12.5), .foregroundColor: Graphite.faint])
        nameField.stringValue = original?.name ?? engine.presetName
        hostField.stringValue = original?.host ?? "localhost"
        portField.stringValue = String(original?.port ?? engine.defaults.port)
        usernameField.stringValue = original?.username ?? engine.presetUser
        databaseField.stringValue = original?.database ?? engine.defaults.database
        passwordField.placeholderAttributedString = NSAttributedString(
            string: original == nil ? "Stored in OS keychain" : "Leave blank to keep saved password",
            attributes: [.font: Graphite.ui(12.5), .foregroundColor: Graphite.faint])
        tlsPopup.selectItem(withTitle: (original?.tlsMode ?? "preferred").capitalized)
        caField.stringValue = original?.caCertPath ?? ""
        readOnlyBox.state = original?.readOnly == true ? .on : .off
        updateEngineLabels()
        setColor(color)
    }

    private func updateEngineLabels() {
        let redis = engine == .redis
        usernameLabel.stringValue = redis ? "Username (ACL, optional)" : "Username"
        databaseLabel.stringValue = redis ? "Database index" : "Database"
        usernameField.setPlaceholder(redis ? "default" : "")
        databaseField.setPlaceholder(redis ? "0" : "")
        eyebrowLabel.stringValue = engine.label.uppercased()
        urlField.placeholderAttributedString = NSAttributedString(string: engine.urlPlaceholder, attributes: [.font: Graphite.ui(12.5), .foregroundColor: Graphite.faint])
    }

    /// Switches engine and replaces fields still holding the previous defaults.
    private func setEngine(_ next: Engine) {
        let previous = engine
        if nameField.stringValue == previous.presetName { nameField.stringValue = next.presetName }
        if portField.stringValue == String(previous.defaults.port) { portField.stringValue = String(next.defaults.port) }
        if usernameField.stringValue == previous.presetUser { usernameField.stringValue = next.presetUser }
        if databaseField.stringValue == previous.defaults.database { databaseField.stringValue = next.defaults.database }
        engine = next
        engineSegment.selectedEngine = next
        updateEngineLabels()
    }

    private func setColor(_ hex: String) {
        color = hex.lowercased()
        swatches.forEach { $0.selected = $0.hex.caseInsensitiveCompare(hex) == .orderedSame }
        customWell.color = NSColor(css: hex) ?? Graphite.accent
    }

    @objc private func customColor() {
        setColor(customWell.color.cssHex)
    }

    private func importURL() {
        let text = urlField.stringValue.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return }
        switch Helpers.parseConnectionURL(text) {
        case .success(let imported):
            let nameIsDefault = nameField.stringValue.trimmingCharacters(in: .whitespaces).isEmpty || nameField.stringValue == engine.presetName
            if original == nil && nameIsDefault { nameField.stringValue = string(imported["suggestedName"]) }
            engine = Engine(rawValue: string(imported["engine"])) ?? engine
            engineSegment.selectedEngine = engine
            hostField.stringValue = string(imported["host"])
            portField.stringValue = String(int(imported["port"]))
            usernameField.stringValue = string(imported["username"])
            databaseField.stringValue = string(imported["defaultDatabase"])
            tlsPopup.selectItem(withTitle: string(imported["tlsMode"]).capitalized)
            if let password = imported["password"] as? String { passwordField.stringValue = password }
            updateEngineLabels()
            show("Connection URL imported. Review the details, then save and connect.", color: Graphite.accentText)
        case .failure(let error):
            show(error.localizedDescription, color: Graphite.danger)
        }
    }

    private var input: [String: Any] {
        var input: [String: Any] = [
            "name": nameField.stringValue, "color": color, "engine": engine.rawValue,
            "host": hostField.stringValue, "port": Int(portField.stringValue.trimmingCharacters(in: .whitespaces)) ?? 0,
            "username": usernameField.stringValue, "defaultDatabase": databaseField.stringValue,
            "tlsMode": tlsPopup.titleOfSelectedItem?.lowercased() ?? "preferred",
            "caCertPath": caField.stringValue.trimmingCharacters(in: .whitespaces).isEmpty ? NSNull() : caField.stringValue,
            "ssh": original?.raw["ssh"] ?? NSNull(), "readOnly": readOnlyBox.state == .on,
        ]
        if let original { input["id"] = original.id }
        if original == nil || !passwordField.stringValue.isEmpty { input["password"] = passwordField.stringValue }
        return input
    }

    private func show(_ message: String, color: NSColor) {
        feedback.stringValue = message
        feedbackBox.isHidden = message.isEmpty
        if color == Graphite.danger {
            feedback.textColor = NSColor(hex: 0xffb3ac)
            feedbackBox.fill = Graphite.dangerSoft
            feedbackBox.outline = NSColor(hex: 0xff6b61, alpha: 0.3)
        } else if color == Graphite.success {
            feedback.textColor = NSColor(hex: 0x9be7bf)
            feedbackBox.fill = NSColor(hex: 0x5ad394, alpha: 0.1)
            feedbackBox.outline = NSColor(hex: 0x5ad394, alpha: 0.3)
        } else {
            feedback.textColor = Graphite.accentText
            feedbackBox.fill = Graphite.accentSoft
            feedbackBox.outline = NSColor(hex: 0x4c9aff, alpha: 0.3)
        }
    }

    private func setBusy(_ busy: Bool, testing: Bool = false) {
        self.busy = busy
        testButton.isEnabled = !busy
        saveButton.isEnabled = !busy
        deleteButton?.isEnabled = !busy
        testButton.title = busy && testing ? "Testing…" : "Test connection"
    }

    private func test() {
        guard !busy else { return }
        setBusy(true, testing: true)
        show("", color: Graphite.accentText)
        onTest(input) { [weak self] error in
            self?.setBusy(false)
            if let error { self?.show(error.localizedDescription, color: Graphite.danger) } else { self?.show("Connection successful.", color: Graphite.success) }
        }
    }

    private func save() {
        guard !busy else { return }
        setBusy(true)
        saveButton.title = "Testing…"
        show("Testing connection before saving…", color: Graphite.accentText)
        let input = self.input
        onTest(input) { [weak self] error in
            guard let self else { return }
            if let error {
                self.setBusy(false)
                self.saveButton.title = "Save & connect"
                self.show(error.localizedDescription, color: Graphite.danger)
                return
            }
            self.saveButton.title = "Connecting…"
            self.show("Connection successful. Saving and connecting…", color: Graphite.success)
            self.onSave(input) { [weak self] error in
                guard let self else { return }
                if let error {
                    self.setBusy(false)
                    self.saveButton.title = "Save & connect"
                    self.show(error.localizedDescription, color: Graphite.danger)
                } else {
                    self.dismiss()
                }
            }
        }
    }

    private func delete() {
        guard !busy else { return }
        onDelete?()
    }

    func dismiss() {
        guard let window, let parent = window.sheetParent else { window?.close(); return }
        parent.endSheet(window)
    }

    /// Escape closes the sheet.
    override func cancelOperation(_ sender: Any?) { dismiss() }
}
