import SwiftUI

enum Graphite {
    static let bg = Color(hex: "161618"), chrome = Color(hex: "1c1c1e"), control = Color(hex: "2a2a2d")
    static let border = Color(hex: "353538"), text = Color(hex: "e8e8ea"), muted = Color(hex: "a0a0a6")
    static let accent = Color(hex: "4c9aff"), danger = Color(hex: "ff8a80")
}

extension Color {
    init(hex: String) { let value = Int(hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted), radix: 16) ?? 0; self.init(red: Double((value >> 16) & 255) / 255, green: Double((value >> 8) & 255) / 255, blue: Double(value & 255) / 255) }
}

struct GraphiteField: ViewModifier {
    func body(content: Content) -> some View { content.padding(10).background(Graphite.control).clipShape(RoundedRectangle(cornerRadius: 7)).overlay(RoundedRectangle(cornerRadius: 7).stroke(Graphite.border)) }
}
