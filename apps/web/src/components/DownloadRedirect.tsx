import { useEffect } from "react";
import { detectDownload, detectMobile, DOWNLOADS, TESTFLIGHT_URL } from "../downloads";
import { PRODUCT_NAME } from "../site";

/**
 * anyba.se/download and /mobile send each visitor straight to their install:
 * the iPhone TestFlight beta or the desktop app for their system. Android's beta
 * takes two steps, so Android and unrecognized visitors land on the homepage,
 * which shows them. The site is static, so this runs in the browser.
 */
export function downloadTarget(hints: { userAgent: string; platform?: string; maxTouchPoints?: number }): string {
  const mobile = detectMobile(hints);
  if (mobile === "ios") return TESTFLIGHT_URL;
  if (mobile === "android") return "/";
  return DOWNLOADS.find((item) => item.id === detectDownload(hints))?.href ?? "/";
}

export function DownloadRedirect() {
  useEffect(() => {
    const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
    window.location.replace(
      downloadTarget({
        userAgent: nav.userAgent,
        platform: nav.userAgentData?.platform,
        maxTouchPoints: nav.maxTouchPoints,
      }),
    );
  }, []);

  return (
    <main className="mx-auto w-full max-w-2xl px-6 py-16 sm:py-24">
      <p className="text-sm text-ink-secondary">
        Opening the {PRODUCT_NAME} download for your device…{" "}
        <a href="/" className="text-link">
          See all downloads
        </a>
      </p>
    </main>
  );
}
