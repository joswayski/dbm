/**
 * The "Latest changes" list is fetched from GitHub when the site is built, the
 * same way captur.es and caper.chat build theirs. Every deploy refreshes it.
 *
 * It reads the public Atom feed of `main` rather than the REST API: the API's
 * unauthenticated limit is 60 requests an hour per IP, and Cloudflare's build
 * machines share IPs, so API-based builds fail there without a token.
 */

export type LatestChange = {
  sha: string;
  title: string;
  url: string;
  committedAt: string;
};

export type FeedEntry = {
  link: string;
  title: string;
  updated: string;
  author: string;
};

export const CHANGE_COUNT = 10;

const XML_ENTITIES: Record<string, string> = {
  amp: "&",
  lt: "<",
  gt: ">",
  quot: '"',
  apos: "'",
};

function decodeXml(text: string): string {
  return text.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/giu, (entity, name: string) => {
    if (name.startsWith("#x") || name.startsWith("#X")) return String.fromCodePoint(Number.parseInt(name.slice(2), 16));
    if (name.startsWith("#")) return String.fromCodePoint(Number.parseInt(name.slice(1), 10));
    return XML_ENTITIES[name.toLowerCase()] ?? entity;
  });
}

/** Parses GitHub's `commits/<branch>.atom` feed, newest first. */
export function parseCommitFeed(xml: string): FeedEntry[] {
  return Array.from(xml.matchAll(/<entry>([\s\S]*?)<\/entry>/gu), ([, body = ""]) => {
    const link = body.match(/<link\b[^>]*\bhref="([^"]+)"/u)?.[1];
    const title = body.match(/<title>([\s\S]*?)<\/title>/u)?.[1];
    const updated = body.match(/<updated>([^<]+)<\/updated>/u)?.[1];
    const author = body.match(/<author>\s*<name>([^<]*)<\/name>/u)?.[1] ?? "";
    if (!link || !title?.trim() || !updated) {
      throw new Error("GitHub returned an incomplete commit feed entry");
    }
    return {
      link: decodeXml(link),
      title: decodeXml(title).trim(),
      updated: updated.trim(),
      author: decodeXml(author).trim(),
    };
  });
}

/** Dependency maintenance belongs in GitHub history, not the product change list. */
export function isDependencyUpdate(entry: FeedEntry): boolean {
  return /^Bump\b/iu.test(entry.title) || entry.author.toLowerCase().startsWith("dependabot");
}

export function toLatestChange(entry: FeedEntry, repository: string): LatestChange {
  const sha = entry.link.match(/\/commit\/([0-9a-f]{7,40})$/iu)?.[1];
  if (!sha) throw new Error(`Unexpected commit link in GitHub feed: ${entry.link}`);

  const pullRequest = entry.title.match(/\(#(\d+)\)$/u)?.[1] ?? entry.title.match(/^Merge pull request #(\d+)/u)?.[1];

  return {
    sha,
    title: pullRequest ? entry.title.replace(/\s+\(#\d+\)$/u, "") : entry.title,
    url: pullRequest ? `https://github.com/${repository}/pull/${pullRequest}` : entry.link,
    committedAt: entry.updated,
  };
}

export function selectLatestChanges(entries: FeedEntry[], repository: string): LatestChange[] {
  return entries
    .filter((entry) => !isDependencyUpdate(entry))
    .map((entry) => toLatestChange(entry, repository))
    .slice(0, CHANGE_COUNT);
}

/** Fails the build rather than deploying an empty list; the previous deploy stays live. */
export async function fetchLatestChanges(repository: string): Promise<LatestChange[]> {
  const response = await fetch(`https://github.com/${repository}/commits/main.atom`, {
    headers: { Accept: "application/atom+xml", "User-Agent": "anybase-web-build" },
    // Fail promptly instead of hanging the build on a stalled connection.
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) {
    throw new Error(`GitHub commit feed request failed with ${response.status}`);
  }

  const changes = selectLatestChanges(parseCommitFeed(await response.text()), repository);
  if (changes.length === 0) {
    throw new Error("GitHub returned no product changes for main");
  }
  return changes;
}

const relativeTimeFormatter = new Intl.RelativeTimeFormat("en", { numeric: "always" });

const DIVISIONS = [
  { amount: 60, unit: "second" },
  { amount: 60, unit: "minute" },
  { amount: 24, unit: "hour" },
  { amount: 7, unit: "day" },
  { amount: 4.345, unit: "week" },
  { amount: 12, unit: "month" },
  { amount: Number.POSITIVE_INFINITY, unit: "year" },
] as const;

export function formatRelativeTime(date: string, now: number): string {
  let duration = (new Date(date).getTime() - now) / 1_000;
  for (const division of DIVISIONS) {
    if (Math.abs(duration) < division.amount) {
      return relativeTimeFormatter.format(Math.round(duration), division.unit);
    }
    duration /= division.amount;
  }
  return relativeTimeFormatter.format(0, "second");
}
