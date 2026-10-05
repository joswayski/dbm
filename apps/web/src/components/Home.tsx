import { useEffect, useState } from "react";
import { detectDownload, DOWNLOADS, type DownloadId } from "../downloads";
import { formatRelativeTime, type LatestChange } from "../latestChanges";
import { AUTHOR_URL, FEATURES, PRODUCT_NAME, RELEASES_URL, REPO_URL, SCREENSHOTS } from "../site";
import { AppleIcon, GitHubIcon, LinuxIcon, WindowsIcon } from "./icons";

type HomeProps = {
  latestChanges: readonly LatestChange[];
  builtAt: number;
};

export function Home({ latestChanges, builtAt }: HomeProps) {
  const [now, setNow] = useState(builtAt);
  const [detected, setDetected] = useState<DownloadId | null>(null);

  useEffect(() => {
    const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
    setDetected(
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
            <h1 className="text-2xl font-semibold tracking-tight text-ink-strong sm:text-3xl">
              {PRODUCT_NAME}
            </h1>
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
          A fast, local-first database manager for PostgreSQL, MySQL, and Redis, by{" "}
          <a href={AUTHOR_URL} target="_blank" rel="noreferrer" className="text-link">
            Jose Valerio
          </a>
          .
        </p>
        <p className="mt-2 font-mono text-xs text-ink-faint">Formerly DBM.</p>
      </header>

      <section aria-labelledby="download-heading" className="mt-12 border-t border-border pt-10">
        <h2 id="download-heading" className="text-base font-semibold tracking-tight text-ink-strong sm:text-lg">
          Download
        </h2>
        <p className="mt-3 max-w-xl text-sm leading-relaxed text-ink-muted">
          Native desktop apps for macOS, Windows, and Linux. Each merge to <code className="font-mono text-[0.8125rem] text-ink-secondary">main</code>{" "}
          ships a new release, and installed apps update themselves.
        </p>

        <div className="mt-6 flex flex-col gap-2.5 sm:flex-row sm:flex-wrap">
          {DOWNLOADS.map((download) => (
            <a
              key={download.id}
              href={download.href}
              className="download-button"
              data-primary={download.id === detected}
            >
              <PlatformIcon id={download.id} />
              <span className="flex flex-col leading-tight">
                <span className="text-sm font-semibold">{download.platform}</span>
                <span
                  className={`font-mono text-[0.6875rem] ${download.id === detected ? "text-white/75" : "text-ink-faint"}`}
                >
                  {download.detail}
                </span>
              </span>
            </a>
          ))}
        </div>

        <p className="mt-4 text-xs leading-relaxed text-ink-faint">
          The macOS app is signed and notarized. The Windows installer isn't code-signed yet, so
          SmartScreen may warn on first install.{" "}
          <a href={RELEASES_URL} target="_blank" rel="noreferrer" className="text-ink-muted underline-offset-4 hover:text-accent-text hover:underline">
            All releases
          </a>
        </p>
      </section>

      <section aria-label="Screenshots" className="mt-12">
        <a href={hero.src} target="_blank" rel="noreferrer" className="screenshot">
          <img src={hero.src} alt={hero.alt} width={1600} height={1000} className="block h-auto w-full" />
        </a>
        <ul className="mt-3 grid grid-cols-2 gap-3">
          {gallery.map((shot) => (
            <li key={shot.src}>
              <a href={shot.src} target="_blank" rel="noreferrer" className="screenshot">
                <img
                  src={shot.src}
                  alt={shot.alt}
                  width={1600}
                  height={1000}
                  loading="lazy"
                  className="block h-auto w-full"
                />
              </a>
              <p className="mt-2 text-xs text-ink-faint">{shot.caption}</p>
            </li>
          ))}
        </ul>
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

      <footer className="mt-16 border-t border-border pt-6 font-mono text-[0.6875rem] text-ink-faint">
        Apache-2.0 ·{" "}
        <a href={REPO_URL} target="_blank" rel="noreferrer" className="hover:text-ink-secondary">
          source
        </a>{" "}
        ·{" "}
        <a href={AUTHOR_URL} target="_blank" rel="noreferrer" className="hover:text-ink-secondary">
          josevalerio.com
        </a>
      </footer>
    </main>
  );
}

function PlatformIcon({ id }: { id: DownloadId }) {
  const className = "h-[1.15rem] w-[1.15rem] shrink-0";
  if (id === "macos") return <AppleIcon className={className} />;
  if (id === "windows") return <WindowsIcon className={className} />;
  return <LinuxIcon className={className} />;
}
