import { downloadTarget } from "../src/components/DownloadRedirect";
import {
  ANDROID_TESTERS_GROUP_URL,
  detectDownload,
  detectMobile,
  DOWNLOADS,
  PLAY_TESTING_URL,
  TESTFLIGHT_URL,
} from "../src/downloads";

const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) Mobile/15E148";
const ANDROID = "Mozilla/5.0 (Linux; Android 15; Pixel 9) Mobile Safari/537.36";

describe("detectMobile", () => {
  it.each([
    [IPHONE, "ios"],
    [ANDROID, "android"],
    ["Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15", null],
    ["Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36", null],
  ])("maps %s to %s", (userAgent, expected) => {
    expect(detectMobile({ userAgent })).toBe(expected);
  });

  it("treats iPadOS, which reports a Mac user agent, as iOS", () => {
    expect(detectMobile({ userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", maxTouchPoints: 5 })).toBe(
      "ios",
    );
  });
});

describe("mobile betas", () => {
  it("point at the se.anyba.app betas", () => {
    expect(TESTFLIGHT_URL).toBe("https://testflight.apple.com/join/mFPKjTcv");
    expect(ANDROID_TESTERS_GROUP_URL).toBe("https://groups.google.com/g/anybase-testers");
    expect(PLAY_TESTING_URL).toBe("https://play.google.com/apps/testing/se.anyba.app");
  });
});

describe("downloadTarget", () => {
  it.each([
    [IPHONE, "https://testflight.apple.com/join/mFPKjTcv"],
    [ANDROID, "/"],
    [
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
      "https://github.com/joswayski/anybase/releases/latest/download/Anybase-Windows-x64-setup.exe",
    ],
    ["curl/8.9.1", "/"],
  ])("sends %s to %s", (userAgent, expected) => {
    expect(downloadTarget({ userAgent })).toBe(expected);
  });
});

describe("detectDownload", () => {
  it.each([
    ["Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15", "macos"],
    ["Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36", "windows"],
    ["Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0", "linux"],
    ["Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) Mobile/15E148", null],
    ["Mozilla/5.0 (Linux; Android 15; Pixel 9) Mobile Safari/537.36", null],
    ["curl/8.9.1", null],
  ])("maps %s to %s", (userAgent, expected) => {
    expect(detectDownload({ userAgent })).toBe(expected);
  });

  it("prefers the client hint platform", () => {
    expect(detectDownload({ userAgent: "Mozilla/5.0", platform: "Windows" })).toBe("windows");
  });

  it("treats iPadOS, which reports a Mac user agent, as mobile", () => {
    expect(
      detectDownload({ userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", maxTouchPoints: 5 }),
    ).toBeNull();
  });
});

describe("DOWNLOADS", () => {
  it("points at the latest release assets", () => {
    expect(DOWNLOADS.map((download) => download.href)).toEqual([
      "https://github.com/joswayski/anybase/releases/latest/download/Anybase-macOS.dmg",
      "https://github.com/joswayski/anybase/releases/latest/download/Anybase-Windows-x64-setup.exe",
      "https://github.com/joswayski/anybase/releases/latest/download/Anybase-Linux-x86_64.AppImage",
    ]);
  });
});
