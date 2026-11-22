import { REPO_URL } from "./site";

const DOWNLOAD_BASE = `${REPO_URL}/releases/latest/download`;

/** Asset names published by .github/workflows/release.yml. */
export const DOWNLOADS = [
  {
    id: "macos",
    platform: "macOS",
    href: `${DOWNLOAD_BASE}/Anybase-macOS.dmg`,
  },
  {
    id: "windows",
    platform: "Windows",
    href: `${DOWNLOAD_BASE}/Anybase-Windows-x64-setup.exe`,
  },
  {
    id: "linux",
    platform: "Linux",
    href: `${DOWNLOAD_BASE}/Anybase-Linux-x86_64.AppImage`,
  },
] as const;

export type DownloadId = (typeof DOWNLOADS)[number]["id"];

/**
 * Public phone betas. iPhone uses the TestFlight public link of the "Public
 * Testing" group, which admits testers once Apple approves a build. Android
 * takes two steps: a Play closed test only admits members of its Google Group,
 * who then opt in on Google Play.
 */
export const TESTFLIGHT_URL = "https://testflight.apple.com/join/mFPKjTcv";
export const ANDROID_TESTERS_GROUP_URL = "https://groups.google.com/g/anybase-testers";
export const PLAY_TESTING_URL = "https://play.google.com/apps/testing/se.anyba.app";

export type MobileId = "ios" | "android";

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

/** The phone beta that matches the visitor, or null on desktops and unknown systems. */
export function detectMobile({ userAgent, platform, maxTouchPoints = 0 }: PlatformHints): MobileId | null {
  const hint = `${platform ?? ""} ${userAgent}`;
  if (/iphone|ipad|ipod/i.test(hint) || (/mac/i.test(hint) && maxTouchPoints > 1)) return "ios";
  if (/android/i.test(hint)) return "android";
  return null;
}
