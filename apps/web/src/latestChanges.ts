/**
 * The "Latest changes" list is fetched from GitHub when the site is built, the
 * same way captur.es and caper.chat build theirs. Every deploy refreshes it.
 */

export type LatestChange = {
  sha: string;
  title: string;
  url: string;
  committedAt: string;
};

export type GitHubCommit = {
  sha: string;
  html_url: string;
  author: { login: string } | null;
  commit: {
    message: string;
    committer: { date: string } | null;
    author: { name?: string; date: string } | null;
  };
};

export const CHANGE_COUNT = 10;
/** Fetch extra commits so Dependabot merges can be dropped without under-filling the list. */
const FETCH_COUNT = 30;

/** Dependency maintenance belongs in GitHub history, not the product change list. */
export function isDependencyUpdateCommit(entry: GitHubCommit): boolean {
  const title = entry.commit.message.split("\n", 1)[0]?.trim() ?? "";
  if (/^Bump\b/iu.test(title)) return true;

  const login = entry.author?.login?.toLowerCase() ?? "";
  const authorName = entry.commit.author?.name?.toLowerCase() ?? "";
  return login.startsWith("dependabot") || authorName.startsWith("dependabot");
}

export function toLatestChange(entry: GitHubCommit, repository: string): LatestChange {
  const title = entry.commit.message.split("\n", 1)[0]?.trim();
  const committedAt = entry.commit.committer?.date ?? entry.commit.author?.date;
  if (!entry.sha || !entry.html_url || !title || !committedAt) {
    throw new Error("GitHub returned an incomplete commit entry");
  }

  const pullRequest =
    title.match(/\(#(\d+)\)$/u)?.[1] ?? title.match(/^Merge pull request #(\d+)/u)?.[1];

  return {
    sha: entry.sha,
    title: pullRequest ? title.replace(/\s+\(#\d+\)$/u, "") : title,
    url: pullRequest ? `https://github.com/${repository}/pull/${pullRequest}` : entry.html_url,
    committedAt,
  };
}

export function selectLatestChanges(entries: GitHubCommit[], repository: string): LatestChange[] {
  return entries
    .filter((entry) => !isDependencyUpdateCommit(entry))
    .map((entry) => toLatestChange(entry, repository))
    .slice(0, CHANGE_COUNT);
}

/** Fails the build rather than deploying an empty list; the previous deploy stays live. */
export async function fetchLatestChanges(repository: string, token?: string): Promise<LatestChange[]> {
  const url = new URL(`https://api.github.com/repos/${repository}/commits`);
  url.searchParams.set("sha", "main");
  url.searchParams.set("per_page", String(FETCH_COUNT));

  const response = await fetch(url, {
    headers: {
      Accept: "application/vnd.github+json",
      "User-Agent": "anybase-web-build",
      "X-GitHub-Api-Version": "2022-11-28",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    // Fail promptly instead of hanging the build on a stalled connection.
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) {
    throw new Error(`GitHub history request failed with ${response.status}`);
  }

  const entries = (await response.json()) as GitHubCommit[];
  const changes = Array.isArray(entries) ? selectLatestChanges(entries, repository) : [];
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
