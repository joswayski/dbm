import { detectDownload, DOWNLOADS } from "../src/downloads";

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
      "https://github.com/joswayski/dbm/releases/latest/download/DBM-macOS.dmg",
      "https://github.com/joswayski/dbm/releases/latest/download/DBM-Windows-x64-setup.exe",
      "https://github.com/joswayski/dbm/releases/latest/download/DBM-Linux-x86_64.AppImage",
    ]);
  });
});
