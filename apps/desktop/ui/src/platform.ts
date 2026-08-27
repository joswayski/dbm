/* One place decides how shortcuts are described, so macOS shows ⌘ while Windows
   and Linux show Ctrl without every component re-deriving it. */

export function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad|iPod/.test(navigator.platform) || /Mac OS X/.test(navigator.userAgent);
}

export function modifierLabel(): string {
  return isApplePlatform() ? "⌘" : "Ctrl";
}

export function shortcut(key: string): string {
  return isApplePlatform() ? `⌘${key}` : `Ctrl+${key}`;
}

export function runShortcutGlyph(): string {
  return isApplePlatform() ? "⌘↵" : "Ctrl+↵";
}

export function isPrimaryModifier(event: { metaKey: boolean; ctrlKey: boolean }): boolean {
  return isApplePlatform() ? event.metaKey : event.ctrlKey;
}
