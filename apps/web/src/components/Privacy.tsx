import type { ReactNode } from "react";
import { CONTACT_EMAIL, PRODUCT_NAME } from "../site";

export const PRIVACY_UPDATED = "October 6, 2026";

/** The privacy policy for the apps and anyba.se, linked from the app stores. */
export function Privacy() {
  return (
    <main className="mx-auto w-full max-w-2xl px-6 py-16 sm:py-24">
      <a
        href="/"
        className="inline-flex items-center gap-2.5 text-sm text-ink-muted no-underline transition-colors duration-150 ease-out hover:text-ink-strong"
      >
        <img src="/icon-192.png" alt="" width={24} height={24} className="h-6 w-6" />
        {PRODUCT_NAME}
      </a>

      <h1 className="mt-10 text-2xl font-semibold tracking-tight text-ink-strong sm:text-3xl">Privacy policy</h1>
      <p className="mt-3 text-xs text-ink-faint">Last updated {PRIVACY_UPDATED}</p>

      <p className="mt-8 text-[0.9375rem] leading-relaxed text-ink-secondary">
        {PRODUCT_NAME} does not collect, sell, or share your data. It has no accounts, ads, analytics, or telemetry.
        This policy covers the {PRODUCT_NAME} apps for macOS, Windows, Linux, Android, and iPhone, and this website.
      </p>

      <Section title="What stays on your device">
        <p>
          Your connection profiles (names, hosts, ports, usernames, database names, colors, and settings), your query
          history, and app preferences are stored only on your device, in the app's own storage. Query history can
          contain sensitive SQL, so treat your device accordingly. Deleting a connection also deletes its history, and
          uninstalling the Android or iPhone app removes all of its data.
        </p>
        <p>
          On desktop, passwords are kept in your operating system's credential store (macOS Keychain, Windows Credential
          Manager, or the Linux Secret Service). On Android and iPhone, passwords stay in memory for the current session
          and are never written to storage.
        </p>
      </Section>

      <Section title="Connections to your databases">
        <p>
          {PRODUCT_NAME} connects directly to the PostgreSQL, MySQL, and Redis servers you configure, using the details
          you enter. Your queries and results travel only between your device and those servers. Nothing passes through
          a server run by {PRODUCT_NAME}. The Android and iPhone apps require TLS and verify server certificates.
        </p>
      </Section>

      <Section title="Updates">
        <p>
          The desktop apps check GitHub for a new release shortly after launch and then every 30 minutes, and download
          updates from GitHub. Those requests share standard information such as your IP address and the app version
          with GitHub, under{" "}
          <a
            href="https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement"
            target="_blank"
            rel="noreferrer"
            className="text-link"
          >
            GitHub's privacy statement
          </a>
          . The Android and iPhone apps are updated through Google Play and Apple, which handle installs and any
          diagnostics you agree to share under their own policies.
        </p>
      </Section>

      <Section title="This website">
        <p>
          anyba.se is a static site served by Cloudflare. It sets no cookies and runs no analytics or trackers.
          Cloudflare processes standard request data, such as IP addresses, to deliver and protect the site. Download
          links point to GitHub.
        </p>
      </Section>

      <Section title="Children">
        <p>{PRODUCT_NAME} is a tool for working with databases and is not directed at children.</p>
      </Section>

      <Section title="Changes and contact">
        <p>
          Changes to this policy are posted on this page with a new date. Questions:{" "}
          <a href={`mailto:${CONTACT_EMAIL}`} className="text-link">
            {CONTACT_EMAIL}
          </a>
          .
        </p>
      </Section>
    </main>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="mt-10 border-t border-border pt-8">
      <h2 className="section-label">{title}</h2>
      <div className="mt-4 space-y-4 text-sm leading-relaxed text-ink-secondary">{children}</div>
    </section>
  );
}
