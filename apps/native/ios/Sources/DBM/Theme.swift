import Combine
import UIKit

enum Graphite {
    static let bg = UIColor(hex: "161618"), chrome = UIColor(hex: "1c1c1e"), sidebar = UIColor(hex: "1f1f21")
    static let gridHeader = UIColor(hex: "1a1a1c"), control = UIColor(hex: "2a2a2d"), controlActive = UIColor(hex: "3a3a3d")
    static let hairline = UIColor(hex: "202023"), border = UIColor(hex: "2a2a2d"), borderStrong = UIColor(hex: "353538")
    static let textStrong = UIColor(hex: "f5f5f7"), text = UIColor(hex: "e8e8ea"), secondary = UIColor(hex: "c7c7cc")
    static let muted = UIColor(hex: "a0a0a6"), faint = UIColor(hex: "808087")
    static let accent = UIColor(hex: "4c9aff"), accentStrong = UIColor(hex: "3b78c7"), danger = UIColor(hex: "ff8a80")
    static let success = UIColor(hex: "5ad394")
    static let palette = ["#4c9aff", "#ff9f43", "#3dd6c6", "#b48cff", "#ff6b8a", "#7ed957", "#f0b14c", "#8e8e93"]

    static func font(_ size: CGFloat, bold: Bool = false) -> UIFont {
        UIFont(name: bold ? "SpaceMono-Bold" : "SpaceMono-Regular", size: size) ?? .monospacedSystemFont(ofSize: size, weight: bold ? .bold : .regular)
    }
}

extension UIColor {
    convenience init(hex: String) {
        let value = Int(hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted), radix: 16) ?? 0
        self.init(red: CGFloat((value >> 16) & 255) / 255, green: CGFloat((value >> 8) & 255) / 255, blue: CGFloat(value & 255) / 255, alpha: 1)
    }
}

/// Coalesces `AppModel` changes into one `render` on the next main-loop turn,
/// after the `@Published` values have been set.
@MainActor
final class ModelRenderer {
    private var subscription: AnyCancellable?
    private var pending = false
    init(_ model: AppModel, render: @escaping @MainActor () -> Void) {
        subscription = model.objectWillChange.sink { [weak self] _ in
            guard let self, !self.pending else { return }
            self.pending = true
            DispatchQueue.main.async { self.pending = false; render() }
        }
    }
}

// MARK: - Small view builders shared by the screens

func label(_ text: String = "", size: CGFloat = 13, bold: Bool = false, color: UIColor = Graphite.text) -> UILabel {
    let label = UILabel(); label.text = text; label.font = Graphite.font(size, bold: bold); label.textColor = color
    return label
}

func symbol(_ name: String, size: CGFloat = 13, color: UIColor = Graphite.text) -> UIImageView {
    let view = UIImageView(image: UIImage(systemName: name, withConfiguration: UIImage.SymbolConfiguration(pointSize: size)))
    view.tintColor = color; view.contentMode = .center
    view.setContentHuggingPriority(.required, for: .horizontal)
    return view
}

func hstack(_ views: [UIView], spacing: CGFloat = 8, alignment: UIStackView.Alignment = .center) -> UIStackView {
    let stack = UIStackView(arrangedSubviews: views); stack.axis = .horizontal; stack.spacing = spacing; stack.alignment = alignment
    return stack
}

func vstack(_ views: [UIView], spacing: CGFloat = 0, alignment: UIStackView.Alignment = .fill) -> UIStackView {
    let stack = UIStackView(arrangedSubviews: views); stack.axis = .vertical; stack.spacing = spacing; stack.alignment = alignment
    return stack
}

func spacer() -> UIView {
    let view = UIView(); view.setContentHuggingPriority(.init(1), for: .horizontal); view.setContentCompressionResistancePriority(.init(1), for: .horizontal)
    return view
}

func dot(_ color: UIColor, size: CGFloat = 8, stroke: Bool = false) -> UIView {
    let view = UIView(); view.layer.cornerRadius = size / 2
    if stroke { view.layer.borderColor = color.cgColor; view.layer.borderWidth = 1.5 } else { view.backgroundColor = color }
    view.size(size, size)
    return view
}

extension UIView {
    @discardableResult func size(_ width: CGFloat?, _ height: CGFloat?) -> Self {
        translatesAutoresizingMaskIntoConstraints = false
        if let width { widthAnchor.constraint(equalToConstant: width).isActive = true }
        if let height { heightAnchor.constraint(equalToConstant: height).isActive = true }
        return self
    }
    func pin(to other: UIView, insets: UIEdgeInsets = .zero) {
        translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            leadingAnchor.constraint(equalTo: other.leadingAnchor, constant: insets.left),
            trailingAnchor.constraint(equalTo: other.trailingAnchor, constant: -insets.right),
            topAnchor.constraint(equalTo: other.topAnchor, constant: insets.top),
            bottomAnchor.constraint(equalTo: other.bottomAnchor, constant: -insets.bottom),
        ])
    }
    func padded(_ insets: UIEdgeInsets) -> UIView { let wrapper = UIView(); wrapper.addSubview(self); pin(to: wrapper, insets: insets); return wrapper }
    /// A bar or panel with the Graphite hairline border along its bottom edge.
    func bottomBorder(_ color: UIColor = Graphite.border) {
        let line = UIView(); line.backgroundColor = color; addSubview(line); line.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([line.leadingAnchor.constraint(equalTo: leadingAnchor), line.trailingAnchor.constraint(equalTo: trailingAnchor),
                                     line.bottomAnchor.constraint(equalTo: bottomAnchor), line.heightAnchor.constraint(equalToConstant: 0.5)])
    }
    func topBorder(_ color: UIColor = Graphite.border) {
        let line = UIView(); line.backgroundColor = color; addSubview(line); line.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([line.leadingAnchor.constraint(equalTo: leadingAnchor), line.trailingAnchor.constraint(equalTo: trailingAnchor),
                                     line.topAnchor.constraint(equalTo: topAnchor), line.heightAnchor.constraint(equalToConstant: 0.5)])
    }
}

enum ButtonStyle { case plain, secondary, primary }

/// The Graphite button styles: plain rows/icons, bordered secondary, and filled primary.
func button(_ title: String?, image: String? = nil, style: ButtonStyle = .plain, size: CGFloat = 12, height: CGFloat? = nil,
            action: @escaping () -> Void) -> UIButton {
    var config = UIButton.Configuration.plain()
    config.imagePadding = 6
    config.contentInsets = style == .plain ? .zero : NSDirectionalEdgeInsets(top: 0, leading: 12, bottom: 0, trailing: 12)
    if let image { config.image = UIImage(systemName: image, withConfiguration: UIImage.SymbolConfiguration(pointSize: size)) }
    if let title {
        config.attributedTitle = AttributedString(title, attributes: AttributeContainer([.font: Graphite.font(size, bold: style == .primary)]))
    }
    let button = UIButton(configuration: config, primaryAction: UIAction { _ in action() })
    button.configurationUpdateHandler = { button in
        var config = button.configuration ?? .plain()
        var background = UIBackgroundConfiguration.clear()
        switch style {
        case .plain: break
        case .secondary:
            background.backgroundColor = button.isHighlighted ? Graphite.controlActive : Graphite.control
            background.strokeColor = Graphite.borderStrong; background.strokeWidth = 1; background.cornerRadius = 6
        case .primary:
            background.backgroundColor = button.isHighlighted ? Graphite.accent : Graphite.accentStrong; background.cornerRadius = 6
        }
        config.background = background
        config.baseForegroundColor = style == .primary ? .white : Graphite.text
        button.configuration = config
        button.alpha = button.isEnabled ? 1 : 0.4
    }
    if let height { button.heightAnchor.constraint(greaterThanOrEqualToConstant: height).isActive = true }
    button.setContentHuggingPriority(.required, for: .horizontal)
    button.setContentCompressionResistancePriority(.required, for: .horizontal)
    return button
}

/// A text field with the Graphite control styling.
final class GraphiteTextField: UITextField {
    init(placeholder: String, identifier: String? = nil, size: CGFloat = 13) {
        super.init(frame: .zero)
        font = Graphite.font(size); textColor = Graphite.text; tintColor = Graphite.accent
        attributedPlaceholder = NSAttributedString(string: placeholder, attributes: [.foregroundColor: Graphite.faint, .font: Graphite.font(size)])
        backgroundColor = Graphite.control; layer.cornerRadius = 6; layer.borderWidth = 1; layer.borderColor = Graphite.borderStrong.cgColor
        autocorrectionType = .no; spellCheckingType = .no; autocapitalizationType = .none
        accessibilityIdentifier = identifier
        heightAnchor.constraint(greaterThanOrEqualToConstant: 36).isActive = true
    }
    required init?(coder: NSCoder) { fatalError() }
    override func textRect(forBounds bounds: CGRect) -> CGRect { bounds.insetBy(dx: 10, dy: 0) }
    override func editingRect(forBounds bounds: CGRect) -> CGRect { bounds.insetBy(dx: 10, dy: 0) }
    override func placeholderRect(forBounds bounds: CGRect) -> CGRect { bounds.insetBy(dx: 10, dy: 0) }
}
