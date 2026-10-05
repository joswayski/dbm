import { REPO_URL } from "./site";

const DOWNLOAD_BASE = `${REPO_URL}/releases/latest/download`;

/** Asset names published by .github/workflows/release.yml. */
export const DOWNLOADS = [
  {
    id: "macos",
    platform: "macOS",
    href: `${DOWNLOAD_BASE}/DBM-macOS.dmg`,
  },
  {
    id: "windows",
    platform: "Windows",
    href: `${DOWNLOAD_BASE}/DBM-Windows-x64-setup.exe`,
  },
  {
    id: "linux",
    platform: "Linux",
    href: `${DOWNLOAD_BASE}/DBM-Linux-x86_64.AppImage`,
  },
] as const;

export type DownloadId = (typeof DOWNLOADS)[number]["id"];

type PlatformHints = {
  userAgent: string;
  /** navigator.userAgentData.platform where the browser exposes it. */
  platform?: string;
  /** navigator.maxTouchPoints; iPadOS reports a Mac user agent. */
  maxTouchPoints?: number;
};

/** The desktop download that matches the visitor, or null on phones and unknown systems. */
export function detectDownload({ userAgent, platform, maxTouchPoints = 0 }: PlatformHints): DownloadId | null {
  const hint = `${platform ?? ""} ${userAgent}`;
  if (/android|iphone|ipad|ipod|mobile/i.test(hint)) return null;
  if (/mac/i.test(hint)) return maxTouchPoints > 1 ? null : "macos";
  if (/win/i.test(hint)) return "windows";
  if (/linux|x11|cros/i.test(hint)) return "linux";
  return null;
}
