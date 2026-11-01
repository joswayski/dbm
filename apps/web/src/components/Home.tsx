import { useCallback, useEffect, useRef, useState } from "react";
import { detectDownload, DOWNLOADS, type DownloadId } from "../downloads";
import { formatRelativeTime, type LatestChange } from "../latestChanges";
import {
  AUTHOR_URL,
  CAPER_INVITE_URL,
  CONTACT_EMAIL,
  FEATURES,
  PRODUCT_NAME,
  REPO_URL,
  SCREENSHOTS,
  X_URL,
} from "../site";
import { AppleIcon, ExternalLinkIcon, GitHubIcon, LinuxIcon, MailIcon, WarningIcon, WindowsIcon, XIcon } from "./icons";
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

        <FeedbackList />
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
      <div role="note" className="border-l-[3px] border-modified py-1 pl-4">
        <p className="flex items-center gap-2 text-sm font-semibold text-modified">
          <WarningIcon className="h-4 w-4 shrink-0" />
          Desktop app
        </p>
        <p className="mt-2 text-sm leading-relaxed text-ink-secondary">
          {PRODUCT_NAME} runs on macOS, Windows, and Linux. Open this page on your computer to download it.
        </p>
      </div>
    );
  }

  return (
    <a href={download.href} className="download-button">
      <PlatformIcon id={download.id} />
      Download for {download.platform}
    </a>
  );
}

function FeedbackList() {
  return (
    <div className="mt-10">
      <h2 className="text-xs text-ink-faint">Questions or feedback?</h2>
      <ul className="mt-3 space-y-3 text-sm leading-relaxed text-ink-muted">
        <li>
          Tell me on{" "}
          <a href={X_URL} target="_blank" rel="noreferrer" className="inline-chip" aria-label="@josevalerio on X">
            <XIcon className="h-3 w-3" />
            @josevalerio
            <ExternalLinkIcon className="h-3 w-3 opacity-60" />
          </a>
        </li>
        <li>
          Share it in the{" "}
          <a href={CAPER_INVITE_URL} target="_blank" rel="noreferrer" className="inline-chip">
            #feedback channel on caper.chat
            <ExternalLinkIcon className="h-3 w-3 opacity-60" />
          </a>
        </li>
        <li>
          Email me at <CopyEmailButton email={CONTACT_EMAIL} />
        </li>
      </ul>
    </div>
  );
}

function CopyEmailButton({ email }: { email: string }) {
  const [copied, setCopied] = useState(false);
  const resetTimer = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
    };
  }, []);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(email);
    } catch {
      // Fallback for older browsers or denied clipboard permission.
      const textarea = document.createElement("textarea");
      textarea.value = email;
      textarea.setAttribute("readonly", "");
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand("copy");
      document.body.removeChild(textarea);
    }

    setCopied(true);
    if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
    resetTimer.current = window.setTimeout(() => setCopied(false), 1_800);
  }

  return (
    <button
      type="button"
      className="group/email inline-chip flex-col items-start gap-1 px-2 py-1"
      onClick={handleCopy}
      aria-label={copied ? `Copied ${email}` : `Copy email ${email}`}
    >
      <span className="inline-flex items-center gap-1.5">
        <MailIcon className="h-3 w-3 shrink-0" />
        <span className="break-all">{email}</span>
      </span>
      <span role="status" className="text-[0.625rem] leading-none text-ink-faint group-hover/email:text-accent-text/70">
        {copied ? "copied!" : "click to copy"}
      </span>
    </button>
  );
}

function PlatformIcon({ id }: { id: DownloadId }) {
  const className = "h-[1.15rem] w-[1.15rem] shrink-0";
  if (id === "macos") return <AppleIcon className={className} />;
  if (id === "windows") return <WindowsIcon className={className} />;
  return <LinuxIcon className={className} />;
}
