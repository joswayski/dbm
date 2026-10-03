import SwiftUI

enum Graphite {
    static let bg = Color(hex: "161618"), chrome = Color(hex: "1c1c1e"), sidebar = Color(hex: "1f1f21")
    static let gridHeader = Color(hex: "1a1a1c"), control = Color(hex: "2a2a2d"), controlActive = Color(hex: "3a3a3d")
    static let hairline = Color(hex: "202023"), border = Color(hex: "2a2a2d"), borderStrong = Color(hex: "353538")
    static let textStrong = Color(hex: "f5f5f7"), text = Color(hex: "e8e8ea"), secondary = Color(hex: "c7c7cc")
    static let muted = Color(hex: "a0a0a6"), faint = Color(hex: "808087")
    static let accent = Color(hex: "4c9aff"), accentStrong = Color(hex: "3b78c7"), danger = Color(hex: "ff8a80")
    static let success = Color(hex: "5ad394")
    static let palette = ["#4c9aff", "#ff9f43", "#3dd6c6", "#b48cff", "#ff6b8a", "#7ed957", "#f0b14c", "#8e8e93"]
}

extension Color {
    init(hex: String) { let value = Int(hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted), radix: 16) ?? 0; self.init(red: Double((value >> 16) & 255) / 255, green: Double((value >> 8) & 255) / 255, blue: Double(value & 255) / 255) }
}

struct GraphiteField: ViewModifier {
    func body(content: Content) -> some View { content.padding(.horizontal, 10).frame(minHeight: 36).background(Graphite.control).clipShape(RoundedRectangle(cornerRadius: 6)).overlay(RoundedRectangle(cornerRadius: 6).stroke(Graphite.borderStrong)) }
}

extension View {
    func graphiteField() -> some View { modifier(GraphiteField()) }
    func graphitePanel(_ color: Color = Graphite.chrome) -> some View { background(color).overlay(Rectangle().stroke(Graphite.border, lineWidth: 0.5)) }
}
