import UIKit

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession,
                     options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: "Default", sessionRole: session.role)
        configuration.delegateClass = SceneDelegate.self
        return configuration
    }
}

/// Owns the one window. Anything not active is covered; backgrounding
/// disposes the Rust session and returns to Connections (docs/mobile.md).
final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?
    private var cover: UIWindow?
    private let model = AppModel()

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: scene)
        window.overrideUserInterfaceStyle = .dark
        window.rootViewController = RootViewController(model: model)
        window.makeKeyAndVisible()
        self.window = window
        let cover = UIWindow(windowScene: scene)
        cover.overrideUserInterfaceStyle = .dark
        cover.windowLevel = .alert + 1
        cover.rootViewController = PrivacyCoverController()
        cover.isHidden = false
        self.cover = cover
    }

    func sceneDidBecomeActive(_ scene: UIScene) { cover?.isHidden = true; model.start() }
    func sceneWillResignActive(_ scene: UIScene) { cover?.isHidden = false }
    func sceneDidEnterBackground(_ scene: UIScene) { cover?.isHidden = false; model.background() }
}

final class PrivacyCoverController: UIViewController {
    override func viewDidLoad() {
        view.backgroundColor = Graphite.bg
        let stack = vstack([symbol("lock.fill", size: 30), label("Anybase Locked", size: 20, bold: true)], spacing: 12, alignment: .center)
        view.addSubview(stack); stack.translatesAutoresizingMaskIntoConstraints = false
        stack.centerXAnchor.constraint(equalTo: view.centerXAnchor).isActive = true
        stack.centerYAnchor.constraint(equalTo: view.centerYAnchor).isActive = true
        view.isAccessibilityElement = true; view.accessibilityLabel = "Anybase content hidden"
    }
}

/// Shows Connections or the workspace, the loading spinner, and errors. A new
/// privacy epoch (backgrounding, disconnect) discards every screen and sheet.
@MainActor
final class RootViewController: UIViewController {
    let model: AppModel
    private var renderer: ModelRenderer?
    private var content: UIViewController?
    private var shown: (route: AppModel.Route, epoch: Int)?
    private let spinner = UIActivityIndicatorView(style: .large)
    private let loadingBox = UIView()
    private weak var alert: UIAlertController?

    init(model: AppModel) { self.model = model; super.init(nibName: nil, bundle: nil) }
    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        view.backgroundColor = Graphite.bg
        loadingBox.backgroundColor = Graphite.control.withAlphaComponent(0.85); loadingBox.layer.cornerRadius = 12
        loadingBox.addSubview(spinner); spinner.pin(to: loadingBox, insets: .init(top: 20, left: 20, bottom: 20, right: 20))
        spinner.color = Graphite.text; spinner.startAnimating()
        loadingBox.isAccessibilityElement = true; loadingBox.accessibilityLabel = "Loading"
        view.addSubview(loadingBox); loadingBox.translatesAutoresizingMaskIntoConstraints = false
        loadingBox.centerXAnchor.constraint(equalTo: view.centerXAnchor).isActive = true
        loadingBox.centerYAnchor.constraint(equalTo: view.centerYAnchor).isActive = true
        renderer = ModelRenderer(model) { [weak self] in self?.render() }
        render()
    }

    private func render() {
        if shown?.route != model.route || shown?.epoch != model.privacyEpoch {
            if shown?.epoch != model.privacyEpoch, presentedViewController != nil { dismiss(animated: false) }
            shown = (model.route, model.privacyEpoch)
            content?.willMove(toParent: nil); content?.view.removeFromSuperview(); content?.removeFromParent()
            let next: UIViewController = model.route == .connections ? ConnectionsViewController(model: model) : WorkspaceViewController(model: model)
            addChild(next); view.insertSubview(next.view, at: 0); next.view.pin(to: view); next.didMove(toParent: self)
            content = next
        }
        content?.view.isUserInteractionEnabled = !model.loading
        loadingBox.isHidden = !model.loading
        if let message = model.error, alert == nil {
            let alert = UIAlertController(title: "Anybase", message: message, preferredStyle: .alert)
            alert.addAction(UIAlertAction(title: "OK", style: .default) { [weak self] _ in self?.model.error = nil })
            topPresenter.present(alert, animated: true)
            self.alert = alert
        } else if model.error == nil, let alert, alert.presentingViewController != nil {
            alert.dismiss(animated: true)
        }
    }
}

extension UIViewController {
    /// The view controller sheets should be presented from.
    var topPresenter: UIViewController {
        var top: UIViewController = view.window?.rootViewController ?? self
        while let next = top.presentedViewController, !next.isBeingDismissed { top = next }
        return top
    }
}
