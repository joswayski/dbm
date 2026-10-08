import { useCallback, useEffect, useState } from "react";
import { detectDownload, DOWNLOADS, type DownloadId } from "../downloads";
import { formatRelativeTime, type LatestChange } from "../latestChanges";
import { AUTHOR_URL, FEATURES, PRODUCT_NAME, REPO_URL, SCREENSHOTS } from "../site";
import { AppleIcon, GitHubIcon, LinuxIcon, WindowsIcon } from "./icons";
import { ScreenshotViewer } from "./ScreenshotViewer";

type HomeProps = {
  latestChanges: readonly LatestChange[];
  builtAt: number;
};

export function Home({ latestChanges, builtAt }: HomeProps) {
  const [now, setNow] = useState(builtAt);
  // undefined until the browser has been checked; null for phones and unknown systems.
  const [platform, setPlatform] = useState<DownloadId | null | undefined>(undefined);
  const [viewing, setViewing] = useState<number | null>(null);
  const closeViewer = useCallback(() => setViewing(null), []);

  useEffect(() => {
    const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
    setPlatform(
      detectDownload({
        userAgent: nav.userAgent,
        platform: nav.userAgentData?.platform,
        maxTouchPoints: nav.maxTouchPoints,
      }),
    );

    setNow(Date.now());
    const interval = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(interval);
  }, []);

  const [hero, ...gallery] = SCREENSHOTS;

  return (
    <main className="mx-auto w-full max-w-2xl px-6 py-16 sm:py-24">
      <header>
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <img src="/icon-192.png" alt="" width={32} height={32} className="h-8 w-8" />
            <h1 className="text-2xl font-semibold tracking-tight text-ink-strong sm:text-3xl">{PRODUCT_NAME}</h1>
          </div>
          <a
            href={REPO_URL}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1.5 text-sm text-ink-muted no-underline transition-colors duration-150 ease-out hover:text-ink-strong"
          >
            <GitHubIcon className="h-4 w-4" />
            GitHub
          </a>
        </div>

        <p className="mt-5 max-w-xl text-[0.9375rem] leading-relaxed text-ink-secondary">
          A fast database client for PostgreSQL, MySQL, and Redis, by{" "}
          <a href={AUTHOR_URL} target="_blank" rel="noreferrer" className="text-link">
            Jose Valerio
          </a>
          .
        </p>

        <div className="mt-8 min-h-11">
          <DownloadAction platform={platform} />
        </div>
      </header>

      <section aria-label="Screenshots" className="mt-12">
        <button
          type="button"
          className="screenshot w-full"
          aria-label={`View screenshot: ${hero.caption}`}
          onClick={() => setViewing(0)}
        >
          <img src={hero.src} alt={hero.alt} width={1600} height={1000} className="block h-auto w-full" />
        </button>
        <ul className="mt-3 grid grid-cols-2 gap-3">
          {gallery.map((shot, index) => (
            <li key={shot.src}>
              <button
                type="button"
                className="screenshot w-full"
                aria-label={`View screenshot: ${shot.caption}`}
                onClick={() => setViewing(index + 1)}
              >
                <img
                  src={shot.src}
                  alt={shot.alt}
                  width={1600}
                  height={1000}
                  loading="lazy"
                  className="block h-auto w-full"
                />
              </button>
              <p className="mt-2 text-xs text-ink-faint">{shot.caption}</p>
            </li>
          ))}
        </ul>
        <ScreenshotViewer shots={SCREENSHOTS} index={viewing} onIndexChange={setViewing} onClose={closeViewer} />
      </section>

      <section aria-labelledby="features-heading" className="mt-14 border-t border-border pt-10">
        <h2 id="features-heading" className="section-label">
          Features
        </h2>
        <ul className="mt-6 space-y-3">
          {FEATURES.map((feature) => (
            <li key={feature} className="flex gap-3 text-sm leading-relaxed text-ink-secondary">
              <span aria-hidden="true" className="mt-[0.55rem] h-1 w-1 shrink-0 rounded-full bg-accent" />
              {feature}
            </li>
          ))}
        </ul>
      </section>

      <section aria-labelledby="latest-changes-heading" className="mt-14 border-t border-border pt-10">
        <h2 id="latest-changes-heading" className="section-label">
          Latest changes
        </h2>
        <ol className="mt-6 space-y-5">
          {latestChanges.map((change) => (
            <li key={change.sha}>
              <a
                href={change.url}
                target="_blank"
                rel="noreferrer"
                className="text-sm font-medium leading-snug text-ink no-underline underline-offset-4 transition-colors duration-150 ease-out hover:text-accent-text hover:underline"
              >
                {change.title}
              </a>
              <p className="mt-1.5 font-mono text-[0.6875rem] text-ink-faint">
                <time dateTime={change.committedAt}>{formatRelativeTime(change.committedAt, now)}</time>
              </p>
            </li>
          ))}
        </ol>
      </section>
    </main>
  );
}

/** One button for the visitor's desktop; phones get a note instead. */
function DownloadAction({ platform }: { platform: DownloadId | null | undefined }) {
  // Prerendered HTML can't know the visitor's system; hold the space until it does.
  if (platform === undefined) return null;

  const download = DOWNLOADS.find((item) => item.id === platform);
  if (!download) {
    return (
      <p className="text-sm leading-relaxed text-ink-muted">
        {PRODUCT_NAME} is a desktop app for macOS, Windows, and Linux. Open this page on your computer to download it.
      </p>
    );
  }

  return (
    <a href={download.href} className="download-button">
      <PlatformIcon id={download.id} />
      Download for {download.platform}
    </a>
  );
}

function PlatformIcon({ id }: { id: DownloadId }) {
  const className = "h-[1.15rem] w-[1.15rem] shrink-0";
  if (id === "macos") return <AppleIcon className={className} />;
  if (id === "windows") return <WindowsIcon className={className} />;
  return <LinuxIcon className={className} />;
}
