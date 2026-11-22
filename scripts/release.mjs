// Version and release-note helpers for the per-merge release workflow.
//
//   node scripts/release.mjs version     # next CalVer for the commit being released
//   node scripts/release.mjs notes       # Markdown notes since the previous release
//   node scripts/release.mjs highlights  # user-facing highlights and their history
//   node scripts/release.mjs mobile-notes  # TestFlight and Play notes per mobile app
//
// Each writes its results to $GITHUB_OUTPUT when it is set.

import { appendFileSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import { releaseHighlights } from "./release-highlights.mjs";

export const RELEASE_TIME_ZONE = "America/New_York";
const MAX_RELEASES_PER_DAY = 99;

/** The calendar date of `now` in the release time zone, as YYYY-MM-DD. */
export function releaseDate(now = new Date(), timeZone = RELEASE_TIME_ZONE) {
  const fields = Object.fromEntries(
    new Intl.DateTimeFormat("en-US", { timeZone, year: "numeric", month: "2-digit", day: "2-digit" })
      .formatToParts(now)
      .filter(({ type }) => type !== "literal")
      .map(({ type, value }) => [type, value]),
  );
  return `${fields.year}-${fields.month}-${fields.day}`;
}

/**
 * The next release for `date` given the existing tags. Tags look like
 * `v2026.09.24.1`; the app version packs the day and revision into the patch
 * number (`2026.9.2401`) so it stays valid, increasing SemVer for the updater.
 */
export function nextReleaseVersion(date, tags) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/u.exec(date);
  if (!match) throw new Error(`release date must use YYYY-MM-DD, received ${date}`);
  const [, yearText, monthText, dayText] = match;
  const [year, month, day] = [Number(yearText), Number(monthText), Number(dayText)];
  const normalized = new Date(`${date}T12:00:00Z`);
  if (
    normalized.getUTCFullYear() !== year ||
    normalized.getUTCMonth() + 1 !== month ||
    normalized.getUTCDate() !== day
  ) {
    throw new Error(`release date is not a real calendar date: ${date}`);
  }

  const prefix = `v${yearText}.${monthText}.${dayText}.`;
  const revisions = tags
    .filter((tag) => tag.startsWith(prefix))
    .map((tag) => {
      const suffix = tag.slice(prefix.length);
      if (!/^[1-9]\d*$/u.test(suffix)) throw new Error(`malformed DBM release tag: ${tag}`);
      return Number(suffix);
    });
  const revision = Math.max(0, ...revisions) + 1;
  if (revision > MAX_RELEASES_PER_DAY) {
    throw new Error(`DBM supports at most ${MAX_RELEASES_PER_DAY} releases per day (${date})`);
  }
  const displayVersion = `${yearText}.${monthText}.${dayText}.${revision}`;
  return {
    date,
    revision,
    displayVersion,
    tag: `v${displayVersion}`,
    appVersion: `${year}.${month}.${day * 100 + revision}`,
  };
}

/**
 * Markdown bullet list for the commits on main since the previous release.
 * GitHub merge commits are shown by pull request title; Dependabot bumps are
 * left out.
 */
export function releaseNotes(commits) {
  const lines = [];
  for (const { subject, body } of commits) {
    const merge = /^Merge pull request #(\d+) from (\S+)/u.exec(subject);
    const title = merge
      ? body
          .split(/\r?\n/u)
          .find((line) => line.trim())
          ?.trim() || subject
      : subject;
    if (/^Bump \S+ from \S+ to \S+/u.test(title) || (merge && merge[2].includes("dependabot/"))) continue;
    const reference = merge && !title.includes(`#${merge[1]}`) ? ` (#${merge[1]})` : "";
    lines.push(`* ${title}${reference}`);
  }
  return lines.length > 0 ? lines.join("\n") : "* Maintenance release";
}

// Apps a highlight's `platforms` can name, with how notes label them. A
// highlight without `platforms` applies to every desktop app.
const PLATFORM_LABELS = { macos: "macOS", windows: "Windows", linux: "Linux" };

function validPlatforms(platforms) {
  return (
    platforms === undefined ||
    (Array.isArray(platforms) &&
      platforms.length > 0 &&
      new Set(platforms).size === platforms.length &&
      platforms.every((platform) => Object.hasOwn(PLATFORM_LABELS, platform)))
  );
}

const MAX_HISTORY = 30;
const MAX_HISTORY_BYTES = 64 * 1024;

/** A manifest `releases` entry in the shape `crates/dbm-update` reads, or null. */
function releaseEntry(value) {
  if (!value || typeof value !== "object") return null;
  const { build, version, commit, publishedAt, highlights } = value;
  if (
    !Number.isSafeInteger(build) ||
    build <= 0 ||
    typeof version !== "string" ||
    !/^[0-9a-f]{40}$/u.test(commit) ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/u.test(publishedAt) ||
    !Array.isArray(highlights) ||
    !highlights.every(
      (item) =>
        item && typeof item.title === "string" && typeof item.body === "string" && validPlatforms(item.platforms),
    )
  )
    return null;
  return {
    build,
    version,
    commit,
    publishedAt,
    highlights: highlights.map(({ title, body, platforms }) => ({ title, body, ...(platforms && { platforms }) })),
  };
}

/**
 * The manifest's `releases`: `release` (when its highlights could be written)
 * ahead of the previous manifest's, newest first, capped so installed apps
 * can show every release they skipped and What's new can group them by day.
 */
export function releaseHistory(previous, release) {
  const entries = (Array.isArray(previous) ? previous : [])
    .map(releaseEntry)
    .filter((entry) => entry && (!release || entry.build < release.build));
  if (release) entries.push(releaseEntry(release));
  entries.sort((a, b) => b.build - a.build);
  const kept = entries.slice(0, MAX_HISTORY);
  while (kept.length > 1 && JSON.stringify(kept).length > MAX_HISTORY_BYTES) kept.pop();
  return kept;
}

/**
 * Highlights as the release page and the manifests' plain `notes` show them,
 * which every platform's older installs read: one for some apps names them.
 */
export function highlightNotes(highlights) {
  return highlights
    .map(({ title, body, platforms }) => {
      const label = platforms
        ? ` (${platforms.map((platform) => PLATFORM_LABELS[platform]).join(" and ")}${platforms.length === 1 ? " only" : ""})`
        : "";
      return `* ${title}${label} — ${body}`;
    })
    .join("\n");
}

// App directories, and the shared code every app that links it ships.
const APP_DIRS = [
  ["apps/native/ios/", "ios"],
  ["apps/native/android/", "android"],
  ["apps/native/macos/", "macos"],
  ["apps/native/win32/", "windows"],
  ["apps/native/windows/", "windows"],
  ["apps/native/gtk/", "linux"],
  ["apps/native/desktop/", "desktop"],
  ["crates/dbm-update/", "desktop"],
];
const SHARED_DIRS = ["crates/dbm-core/", "apps/native/bridge/"];

/**
 * TestFlight or Play notes for `platform` (`ios` or `android`): the commits
 * that changed its app, or only the shared core it links, so another app's
 * change never shows up there. At most `max` characters of whole lines.
 */
export function mobileNotes(platform, commits, max) {
  const relevant = commits.filter(({ subject, paths }) => {
    // Subjects are written for users: skip tooling and ones naming only desktop apps.
    if (
      /^(?:chore|docs|tests?|ci|build)(?:\([^)]*\))?:/iu.test(subject) ||
      (/\b(?:macOS|Windows|Linux|desktop)\b/iu.test(subject) && !/\b(?:iPhone|iOS|Android|mobile)\b/iu.test(subject))
    )
      return false;
    const apps = new Set(
      paths.flatMap((path) => APP_DIRS.filter(([dir]) => path.startsWith(dir)).map(([, app]) => app)),
    );
    return (
      apps.has(platform) || (apps.size === 0 && paths.some((path) => SHARED_DIRS.some((dir) => path.startsWith(dir))))
    );
  });
  const lines = releaseNotes(relevant).split("\n");
  while (lines.length > 1 && lines.join("\n").length > max) lines.pop();
  return lines.join("\n").slice(0, max);
}

function git(...args) {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

function previousReleaseTag(sha) {
  try {
    return git(
      "describe",
      "--tags",
      "--abbrev=0",
      "--first-parent",
      "--match",
      "v[0-9]*.[0-9]*.[0-9]*.[0-9]*",
      `${sha}^`,
    ).trim();
  } catch {
    return null;
  }
}

/** First-parent commits since `tag` (or the last 20), with the paths each changed. */
function commitsSince(tag, sha) {
  const range = tag ? `${tag}..${sha}` : sha;
  const limit = tag ? [] : ["--max-count=20"];
  return git(
    "log",
    "--first-parent",
    "--diff-merges=first-parent",
    "--name-only",
    ...limit,
    "--format=%x1e%s%x1f%b%x1f",
    range,
  )
    .split("\u001e")
    .filter((entry) => entry.trim())
    .map((entry) => {
      const [subject, body = "", files = ""] = entry.split("\u001f");
      return { subject: subject.trim(), body: body.trim(), paths: files.split("\n").filter(Boolean) };
    });
}

function writeOutputs(values) {
  const output = process.env.GITHUB_OUTPUT;
  if (!output) return;
  const lines = Object.entries(values).map(([key, value]) => {
    if (!String(value).includes("\n")) return `${key}=${value}`;
    const delimiter = `EOF_${Math.random().toString(36).slice(2)}`;
    return `${key}<<${delimiter}\n${value}\n${delimiter}`;
  });
  appendFileSync(output, `${lines.join("\n")}\n`);
}

async function main() {
  const [command] = process.argv.slice(2);
  const sha = process.env.DBM_RELEASE_SHA || "HEAD";
  if (command === "version") {
    const committedAt = new Date(git("show", "-s", "--format=%cI", sha).trim());
    const tags = git("tag", "--list").split(/\r?\n/u).filter(Boolean);
    const version = nextReleaseVersion(releaseDate(committedAt), tags);
    writeOutputs({ tag: version.tag, display_version: version.displayVersion, app_version: version.appVersion });
    process.stdout.write(`${JSON.stringify(version)}\n`);
    return;
  }
  if (command === "notes") {
    const previous = previousReleaseTag(sha);
    const notes = releaseNotes(commitsSince(previous, sha));
    writeOutputs({ notes, previous_tag: previous ?? "" });
    process.stdout.write(`${notes}\n`);
    return;
  }
  if (command === "highlights") {
    const commit = git("rev-parse", `${sha}^{commit}`).trim();
    let previous = [];
    try {
      previous = JSON.parse(readFileSync(process.env.PREVIOUS_MANIFEST, "utf8")).releases ?? [];
    } catch {
      // The first release with highlights starts the history.
    }
    const recentHighlights = releaseHistory(previous, null)
      .slice(0, 3)
      .flatMap((release) => release.highlights);
    const highlights = await releaseHighlights(process.env.PREVIOUS_TAG || null, commit, {
      recentHighlights,
    });
    const release = highlights && {
      build: Number(process.env.BUILD),
      version: process.env.DISPLAY_VERSION,
      commit,
      // The publish job stamps the real publication time.
      publishedAt: new Date().toISOString().replace(/\.\d+Z$/u, "Z"),
      highlights,
    };
    const commitNotes = process.env.NOTES || "* Maintenance release";
    const notes = highlights?.length ? highlightNotes(highlights) : commitNotes;
    writeOutputs({
      notes,
      body: highlights?.length ? `${notes}\n\n### Commits\n\n${commitNotes}` : commitNotes,
      releases: JSON.stringify(releaseHistory(previous, release)),
    });
    process.stdout.write(`${notes}\n`);
    return;
  }
  if (command === "mobile-notes") {
    // Since the previous mobile release's commit, when it is an earlier one.
    const previous = process.env.PREVIOUS_MOBILE_SHA || "";
    const commit = git("rev-parse", `${sha}^{commit}`).trim();
    let base = null;
    if (/^[0-9a-f]{40}$/u.test(previous) && previous !== commit) {
      try {
        git("merge-base", "--is-ancestor", previous, commit);
        base = previous;
      } catch {
        // Not an ancestor: fall back to the recent commits.
      }
    }
    const commits = commitsSince(base, commit);
    const ios = mobileNotes("ios", commits, 4000);
    const android = mobileNotes("android", commits, 500);
    writeOutputs({ ios_notes: ios, android_notes: android });
    process.stdout.write(`iPhone:\n${ios}\n\nAndroid:\n${android}\n`);
    return;
  }
  throw new Error("usage: node scripts/release.mjs <version|notes|highlights|mobile-notes>");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
