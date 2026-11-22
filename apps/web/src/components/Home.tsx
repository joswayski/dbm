import { useCallback, useEffect, useRef, useState } from "react";
import {
  ANDROID_TESTERS_GROUP_URL,
  detectDownload,
  detectMobile,
  DOWNLOADS,
  PLAY_TESTING_URL,
  TESTFLIGHT_URL,
  type DownloadId,
  type MobileId,
} from "../downloads";
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
import {
  AppleIcon,
  CheckIcon,
  CopyIcon,
  ExternalLinkIcon,
  GitHubIcon,
  HashIcon,
  LinuxIcon,
  MailIcon,
  WarningIcon,
  WindowsIcon,
  XIcon,
} from "./icons";
import { ScreenshotViewer } from "./ScreenshotViewer";

type HomeProps = {
  latestChanges: readonly LatestChange[];
  builtAt: number;
};

export function Home({ latestChanges, builtAt }: HomeProps) {
  const [now, setNow] = useState(builtAt);
  // undefined until the browser has been checked; null for phones and unknown systems.
  const [platform, setPlatform] = useState<DownloadId | null | undefined>(undefined);
  const [mobile, setMobile] = useState<MobileId | null>(null);
  const [viewing, setViewing] = useState<number | null>(null);
  const closeViewer = useCallback(() => setViewing(null), []);

  useEffect(() => {
    const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
    const hints = {
      userAgent: nav.userAgent,
      platform: nav.userAgentData?.platform,
      maxTouchPoints: nav.maxTouchPoints,
    };
    setMobile(detectMobile(hints));
    setPlatform(detectDownload(hints));

    setNow(Date.now());
    const interval = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(interval);
  }, []);

  const [hero, ...gallery] = SCREENSHOTS;

  return (
    <main className="mx-auto w-full max-w-2xl px-6 py-16 sm:py-24">
      <header>
        <div className="flex items-center gap-3">
          <img src="/icon-192.png" alt="" width={32} height={32} className="h-8 w-8" />
          <h1 className="text-2xl font-semibold tracking-tight text-ink-strong sm:text-3xl">{PRODUCT_NAME}</h1>
        </div>

        <p className="mt-5 max-w-xl text-[0.9375rem] leading-relaxed text-ink-secondary">
          A fast database client for PostgreSQL, MySQL, and Redis, by{" "}
          <a href={AUTHOR_URL} target="_blank" rel="noreferrer" className="text-link">
            Jose Valerio
          </a>
          .
        </p>

        <div className="mt-8 min-h-11">
          <DownloadAction platform={platform} mobile={mobile} />
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

/** One button for the visitor's desktop, the phone beta on iPhone and Android, or a note otherwise. */
function DownloadAction({ platform, mobile }: { platform: DownloadId | null | undefined; mobile: MobileId | null }) {
  // Prerendered HTML can't know the visitor's system; hold the space until it does.
  if (platform === undefined) return null;

  if (mobile === "ios") {
    return (
      <>
        <div className="flex flex-col gap-2.5 sm:flex-row sm:flex-wrap sm:items-center">
          <a href={TESTFLIGHT_URL} className="download-button">
            <AppleIcon className="h-[1.15rem] w-[1.15rem] shrink-0" />
            Get the iPhone beta
          </a>
          <SourceButton />
        </div>
        <MobileBetaNote store="TestFlight" />
      </>
    );
  }

  if (mobile === "android") {
    return (
      <>
        <AndroidBetaSteps />
        <div className="mt-3.5">
          <SourceButton />
        </div>
        <MobileBetaNote store="Google Play" />
      </>
    );
  }

  const download = DOWNLOADS.find((item) => item.id === platform);
  if (!download) {
    return (
      <>
        <div role="note" className="border-l-[3px] border-modified py-1 pl-4">
          <p className="flex items-center gap-2 text-sm font-semibold text-modified">
            <WarningIcon className="h-4 w-4 shrink-0" />
            Desktop app
          </p>
          <p className="mt-2 text-sm leading-relaxed text-ink-secondary">
            {PRODUCT_NAME} runs on macOS, Windows, and Linux. Open this page on your computer to download it.
          </p>
        </div>
        <div className="mt-6">
          <SourceButton />
        </div>
      </>
    );
  }

  return (
    <div className="flex flex-col gap-2.5 sm:flex-row sm:flex-wrap sm:items-center">
      <a href={download.href} className="download-button">
        <PlatformIcon id={download.id} />
        Download for {download.platform}
      </a>
      <SourceButton />
    </div>
  );
}

// A Play closed test only admits members of its Google Group, who then opt in on
// Google Play. Google can't chain those two pages, so the steps are numbered here;
// the group opens in a new tab so this page is still waiting for step 2.
function AndroidBetaSteps() {
  return (
    <ol className="beta-steps">
      <li>
        <a className="download-button" href={ANDROID_TESTERS_GROUP_URL} target="_blank" rel="noreferrer">
          <span className="beta-step-number" aria-hidden="true">
            1
          </span>
          Join the testers group
        </a>
        <p>
          Tap <strong>Join group</strong>, then come back here.
        </p>
      </li>
      <li>
        <a className="download-button" href={PLAY_TESTING_URL}>
          <span className="beta-step-number" aria-hidden="true">
            2
          </span>
          Install from Google Play
        </a>
        <p>
          Tap <strong>Become a tester</strong>, then install {PRODUCT_NAME}.
        </p>
      </li>
    </ol>
  );
}

function MobileBetaNote({ store }: { store: string }) {
  return (
    <p className="mt-4 text-xs leading-relaxed text-ink-muted">
      The phone app is a beta through {store}: read-only connections over verified TLS. For the full app on macOS,
      Windows, or Linux, open this page on your computer.
    </p>
  );
}

function SourceButton() {
  return (
    <a href={REPO_URL} target="_blank" rel="noreferrer" className="source-button">
      <GitHubIcon className="h-[1.15rem] w-[1.15rem] shrink-0" />
      View source
    </a>
  );
}

function FeedbackList() {
  // One row per channel: what you do on the left, where it goes on the right.
  return (
    <section className="mt-10 max-w-sm" aria-labelledby="feedback-heading">
      <h2 id="feedback-heading" className="text-xs text-ink-faint">
        Questions or feedback?
      </h2>
      <ul className="contact-list mt-3">
        <li>
          <a className="contact-row" href={CAPER_INVITE_URL} target="_blank" rel="noreferrer">
            <HashIcon className="h-[15px] w-[15px] shrink-0" />
            <span className="contact-action">Post in #feedback</span>
            <span className="contact-detail">caper.chat</span>
            <ExternalLinkIcon className="contact-trailing" />
          </a>
        </li>
        <li>
          <a className="contact-row" href={X_URL} target="_blank" rel="noreferrer">
            <XIcon className="h-[15px] w-[15px] shrink-0" />
            <span className="contact-action">Message me on X</span>
            <span className="contact-detail">@josevalerio</span>
            <ExternalLinkIcon className="contact-trailing" />
          </a>
        </li>
        <li>
          <CopyEmailRow email={CONTACT_EMAIL} />
        </li>
      </ul>
    </section>
  );
}

function CopyEmailRow({ email }: { email: string }) {
  const [status, setStatus] = useState<"idle" | "copied" | "failed">("idle");
  const resetTimer = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
    };
  }, []);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(email);
      setStatus("copied");
    } catch {
      setStatus("failed");
    }
    if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
    resetTimer.current = window.setTimeout(() => setStatus("idle"), 2_500);
  }

  return (
    <>
      <button
        type="button"
        className="contact-row"
        data-copied={status === "copied" ? "" : undefined}
        onClick={() => void handleCopy()}
        title="Copy email address"
      >
        <MailIcon className="h-[15px] w-[15px] shrink-0" />
        <span className="contact-action">Email me</span>
        <span className="contact-detail">{status === "copied" ? "Copied" : email}</span>
        {status === "copied" ? <CheckIcon className="contact-trailing" /> : <CopyIcon className="contact-trailing" />}
      </button>
      <span className={status === "failed" ? "contact-feedback" : "sr-only"} role="status">
        {status === "copied"
          ? "Email address copied to clipboard."
          : status === "failed"
            ? `Couldn’t copy. Email ${email}`
            : ""}
      </span>
    </>
  );
}

function PlatformIcon({ id }: { id: DownloadId }) {
  const className = "h-[1.15rem] w-[1.15rem] shrink-0";
  if (id === "macos") return <AppleIcon className={className} />;
  if (id === "windows") return <WindowsIcon className={className} />;
  return <LinuxIcon className={className} />;
}
