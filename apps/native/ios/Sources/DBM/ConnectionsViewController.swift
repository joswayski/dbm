import UIKit

@MainActor
final class ConnectionsViewController: UIViewController {
    let model: AppModel
    private var renderer: ModelRenderer?
    private let list = vstack([], spacing: 3)
    private var shownProfiles: [Profile]?

    init(model: AppModel) { self.model = model; super.init(nibName: nil, bundle: nil) }
    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        view.backgroundColor = Graphite.bg
        let demo = label("DEMO", size: 10, bold: true, color: Graphite.accent); demo.isHidden = !model.isDemo
        let header = hstack([symbol("cylinder", color: Graphite.accent), label("Anybase", size: 17, bold: true), spacer(), demo], spacing: 9)
            .padded(.init(top: 0, left: 16, bottom: 0, right: 16))
        header.backgroundColor = Graphite.sidebar; header.size(nil, 50); header.bottomBorder()
        let section = hstack([label("Connections", size: 11, bold: true, color: Graphite.muted), spacer()]).padded(.init(top: 0, left: 16, bottom: 0, right: 16))
        section.size(nil, 34)

        let scroll = UIScrollView(); scroll.alwaysBounceVertical = true
        scroll.addSubview(list); list.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            list.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor),
            list.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor),
            list.leadingAnchor.constraint(equalTo: scroll.frameLayoutGuide.leadingAnchor, constant: 10),
            list.trailingAnchor.constraint(equalTo: scroll.frameLayoutGuide.trailingAnchor, constant: -10),
        ])

        let add = button("New connection", image: "plus", size: 13) { [weak self] in self?.edit(ProfileDraft()) }
        add.contentHorizontalAlignment = .leading; add.accessibilityIdentifier = "add-connection"
        let footer = add.padded(.init(top: 0, left: 16, bottom: 0, right: 16)); footer.backgroundColor = Graphite.sidebar; footer.topBorder()
        add.size(nil, 48)
        let footerBackground = UIView(); footerBackground.backgroundColor = Graphite.sidebar

        let column = vstack([header, section, scroll, footer])
        view.addSubview(footerBackground); view.addSubview(column)
        column.translatesAutoresizingMaskIntoConstraints = false
        footerBackground.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            column.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            column.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor),
            column.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor),
            column.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor),
            footerBackground.topAnchor.constraint(equalTo: footer.topAnchor),
            footerBackground.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            footerBackground.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            footerBackground.trailingAnchor.constraint(equalTo: view.trailingAnchor),
        ])
        renderer = ModelRenderer(model) { [weak self] in self?.render() }
        render()
    }

    private func render() {
        guard shownProfiles != model.profiles else { return }
        shownProfiles = model.profiles
        list.arrangedSubviews.forEach { $0.removeFromSuperview() }
        if model.profiles.isEmpty {
            let empty = vstack([symbol("externaldrive"), label("No connections"),
                                label("Add a read-only TLS connection to begin.", size: 11, color: Graphite.faint)], spacing: 8, alignment: .center)
            list.addArrangedSubview(empty.padded(.init(top: 70, left: 0, bottom: 0, right: 0)))
        }
        for profile in model.profiles { list.addArrangedSubview(row(profile)) }
    }

    private func row(_ profile: Profile) -> UIView {
        let details = vstack([label(profile.name), label("\(profile.engine.title) · \(profile.host):\(profile.port)", size: 11, color: Graphite.faint)], spacing: 2, alignment: .leading)
        details.isUserInteractionEnabled = false
        let editButton = UIButton(primaryAction: UIAction { [weak self] _ in self?.edit(ProfileDraft(profile)) })
        editButton.addSubview(details); details.pin(to: editButton)
        editButton.accessibilityLabel = profile.name; editButton.accessibilityIdentifier = "edit-\(profile.id)"
        editButton.setContentHuggingPriority(.init(1), for: .horizontal)
        let connect = button("Connect", style: .secondary, height: 40) { [weak self] in
            guard let self else { return }
            if model.isDemo { model.connect(profile) } else { edit(ProfileDraft(profile)) }
        }
        connect.accessibilityIdentifier = "connect-\(profile.id)"
        let more = button(nil, image: "ellipsis") {}
        more.menu = UIMenu(children: [
            UIAction(title: "Edit connection") { [weak self] _ in self?.edit(ProfileDraft(profile)) },
            UIAction(title: "Delete connection", attributes: .destructive) { [weak self] _ in self?.confirmDelete(profile) },
        ])
        more.showsMenuAsPrimaryAction = true; more.accessibilityLabel = "Actions for \(profile.name)"; more.size(34, 44)
        let row = hstack([dot(UIColor(hex: profile.color), size: 9, stroke: true), editButton, connect, more], spacing: 10)
            .padded(.init(top: 0, left: 12, bottom: 0, right: 12))
        row.backgroundColor = Graphite.sidebar; row.layer.cornerRadius = 6; row.size(nil, 52)
        return row
    }

    private func edit(_ draft: ProfileDraft) { topPresenter.present(ProfileFormViewController(model: model, draft: draft), animated: true) }

    private func confirmDelete(_ profile: Profile) {
        let sheet = UIAlertController(title: "Delete \(profile.name)?", message: nil, preferredStyle: .actionSheet)
        sheet.addAction(UIAlertAction(title: "Delete connection", style: .destructive) { [weak self] _ in self?.model.delete(profile) })
        sheet.addAction(UIAlertAction(title: "Cancel", style: .cancel))
        topPresenter.present(sheet, animated: true)
    }
}

/// Add or edit a profile. The password lives only in this sheet's draft and is
/// cleared when it closes; the profile is always read-only with required TLS.
@MainActor
final class ProfileFormViewController: UIViewController, UITextFieldDelegate {
    let model: AppModel
    private var draft: ProfileDraft
    private var renderer: ModelRenderer?
    private let engineTitle = label(size: 10, bold: true, color: Graphite.muted)
    private let engine = UISegmentedControl(items: Engine.allCases.map(\.title))
    private let name = GraphiteTextField(placeholder: "Connection name", identifier: "name")
    private let colorField = GraphiteTextField(placeholder: "#RRGGBB", size: 11)
    private let host = GraphiteTextField(placeholder: "localhost", identifier: "host")
    private let port = GraphiteTextField(placeholder: "Port", identifier: "port")
    private let username = GraphiteTextField(placeholder: "Username", identifier: "username")
    private let database = GraphiteTextField(placeholder: "Database", identifier: "database")
    private let password = GraphiteTextField(placeholder: "Not saved on this device", identifier: "password")
    private let usernameLabel = label(size: 11, color: Graphite.muted), databaseLabel = label(size: 11, color: Graphite.muted)
    private let swatches = hstack([], spacing: 2)
    private var swatchDots: [UIView] = []
    private let status = label(size: 12)
    private var test: UIButton!, save: UIButton!, connect: UIButton!
    private var scroll = UIScrollView()

    init(model: AppModel, draft: ProfileDraft) {
        self.model = model; self.draft = draft
        super.init(nibName: nil, bundle: nil)
        if let sheet = sheetPresentationController { sheet.detents = [.large()] }
    }
    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        view.backgroundColor = Graphite.bg
        let cancel = button(nil, image: "xmark", size: 15) { [weak self] in self?.close() }
        cancel.accessibilityLabel = "Cancel"; cancel.size(44, 44)
        let title = label(draft.id == nil ? "New connection" : "Edit connection", size: 17, bold: true)
        let header = hstack([vstack([engineTitle, title], spacing: 2, alignment: .leading), spacer(), cancel])
            .padded(.init(top: 0, left: 16, bottom: 0, right: 16))
        header.backgroundColor = Graphite.chrome; header.size(nil, 60); header.bottomBorder()

        engine.accessibilityIdentifier = "engine"
        engine.setTitleTextAttributes([.font: Graphite.font(12), .foregroundColor: Graphite.text], for: .normal)
        engine.selectedSegmentTintColor = Graphite.controlActive; engine.backgroundColor = Graphite.control
        engine.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            draft.select(Engine.allCases[engine.selectedSegmentIndex]); fill()
        }, for: .valueChanged)
        for hex in Graphite.palette {
            let swatch = UIButton(primaryAction: UIAction { [weak self] _ in self?.draft.color = hex; self?.fill() })
            let circle = dot(UIColor(hex: hex), size: 23); circle.isUserInteractionEnabled = false
            circle.layer.borderColor = Graphite.textStrong.cgColor; swatchDots.append(circle)
            swatch.addSubview(circle); circle.centerXAnchor.constraint(equalTo: swatch.centerXAnchor).isActive = true
            circle.centerYAnchor.constraint(equalTo: swatch.centerYAnchor).isActive = true
            swatch.accessibilityLabel = "Use connection color \(hex)"; swatch.size(40, 44)
            swatches.addArrangedSubview(swatch)
        }
        let swatchScroll = UIScrollView(); swatchScroll.showsHorizontalScrollIndicator = false
        swatchScroll.addSubview(swatches); swatches.pin(to: swatchScroll)
        swatches.heightAnchor.constraint(equalTo: swatchScroll.heightAnchor).isActive = true; swatchScroll.size(nil, 44)
        colorField.accessibilityLabel = "Custom connection color"
        port.keyboardType = .numberPad
        password.isSecureTextEntry = true
        name.autocapitalizationType = .sentences
        for field in [name, colorField, host, port, username, database, password] {
            field.delegate = self
            field.addAction(UIAction { [weak self] _ in self?.read() }, for: .editingChanged)
        }
        let hostRow = hstack([field("Host", host), field("Port", port)], spacing: 10, alignment: .top)
        hostRow.arrangedSubviews[1].size(105, nil)
        let userRow = hstack([field(usernameLabel, username), field(databaseLabel, database)], spacing: 10, alignment: .top)
        userRow.distribution = .fillEqually
        let notice = hstack([symbol("lock.shield"), vstack([label("Read only · TLS required · System CA", size: 11),
            label("Password is cleared on disconnect, background, and dismissal.", size: 11, color: Graphite.faint)], spacing: 2)], spacing: 8)
        (notice.arrangedSubviews[1] as? UIStackView)?.arrangedSubviews.forEach { ($0 as? UILabel)?.numberOfLines = 0 }
        let noticeBox = notice.padded(.init(top: 12, left: 12, bottom: 12, right: 12))
        noticeBox.backgroundColor = Graphite.control; noticeBox.layer.cornerRadius = 6
        status.accessibilityIdentifier = "form-status"; status.numberOfLines = 0; status.isHidden = true

        let form = vstack([field("Database engine", engine), field("Name", name),
                           field("Connection color", vstack([swatchScroll, colorField], spacing: 5)),
                           hostRow, userRow, field("Password · session only", password), noticeBox, status], spacing: 13)
        scroll.alwaysBounceVertical = true; scroll.keyboardDismissMode = .interactive
        scroll.addSubview(form); form.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            form.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor, constant: 16),
            form.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor, constant: -16),
            form.leadingAnchor.constraint(equalTo: scroll.frameLayoutGuide.leadingAnchor, constant: 16),
            form.trailingAnchor.constraint(equalTo: scroll.frameLayoutGuide.trailingAnchor, constant: -16),
        ])

        test = button("Test connection", style: .secondary, height: 40) { [weak self] in
            guard let self else { return }
            setStatus("Testing…")
            model.test(draft) { [weak self] ok in self?.setStatus(ok ? "Connection succeeded" : "Connection failed") }
        }
        test.accessibilityIdentifier = "test-connection"
        save = button("Save", style: .secondary, height: 40) { [weak self] in
            guard let self else { return }
            model.save(draft) { [weak self] ok in if ok { self?.close() } }
        }
        save.accessibilityIdentifier = "save-connection"
        connect = button("Save & connect", style: .primary, height: 40) { [weak self] in
            guard let self else { return }
            model.saveAndConnect(draft) { [weak self] ok in if ok { self?.close() } }
        }
        connect.accessibilityIdentifier = "form-connect"
        let actions = hstack([test, spacer(), save, connect]).padded(.init(top: 12, left: 12, bottom: 12, right: 12))
        actions.backgroundColor = Graphite.chrome; actions.topBorder()

        let column = vstack([header, scroll, actions])
        view.addSubview(column); column.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            column.topAnchor.constraint(equalTo: view.topAnchor),
            column.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor),
            column.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor),
            column.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor),
        ])
        view.keyboardLayoutGuide.usesBottomSafeArea = true
        fill()
        renderer = ModelRenderer(model) { [weak self] in self?.render() }
    }

    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        if presentingViewController == nil { draft.password = ""; password.text = "" }
    }

    func textFieldShouldReturn(_ textField: UITextField) -> Bool { textField.resignFirstResponder() }

    private func field(_ title: String, _ content: UIView) -> UIView { field(label(title, size: 11, color: Graphite.muted), content) }
    private func field(_ title: UILabel, _ content: UIView) -> UIView { vstack([title, content], spacing: 5) }

    /// Copies the draft into the controls (after an engine or color change).
    private func fill() {
        engine.selectedSegmentIndex = Engine.allCases.firstIndex(of: draft.engine) ?? 0
        engineTitle.text = draft.engine.title.uppercased()
        usernameLabel.text = draft.engine == .redis ? "Username (optional)" : "Username"
        databaseLabel.text = draft.engine == .redis ? "Database number" : "Database"
        name.text = draft.name; colorField.text = draft.color; host.text = draft.host; port.text = String(draft.port)
        username.text = draft.username; database.text = draft.database; password.text = draft.password
        render()
    }

    /// Copies the controls into the draft as the user types.
    private func read() {
        draft.name = name.text ?? ""; draft.color = colorField.text ?? ""; draft.host = host.text ?? ""
        if let value = Int(port.text ?? "") { draft.port = value } else if (port.text ?? "").isEmpty { draft.port = 0 }
        draft.username = username.text ?? ""; draft.database = database.text ?? ""; draft.password = password.text ?? ""
        render()
    }

    private var valid: Bool {
        !draft.name.isEmpty && !draft.host.isEmpty && (1...65535).contains(draft.port) && !draft.database.isEmpty && (draft.engine == .redis || !draft.username.isEmpty)
    }

    private func render() {
        for (hex, circle) in zip(Graphite.palette, swatchDots) { circle.layer.borderWidth = draft.color.lowercased() == hex ? 2 : 0 }
        test.isEnabled = valid && !model.loading; save.isEnabled = valid && !model.loading; connect.isEnabled = valid && !model.loading
        isModalInPresentation = model.loading
        view.isUserInteractionEnabled = !model.loading
    }

    private func setStatus(_ text: String) {
        status.text = text; status.isHidden = text.isEmpty
        status.textColor = text == "Connection succeeded" ? Graphite.success : Graphite.danger
    }

    private func close() { draft.password = ""; password.text = ""; dismiss(animated: true) }
}
