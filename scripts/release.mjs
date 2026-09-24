// Version and release-note helpers for the per-merge release workflow.
//
//   node scripts/release.mjs version   # next CalVer for the commit being released
//   node scripts/release.mjs notes     # Markdown notes since the previous release
//
// Both write their results to $GITHUB_OUTPUT when it is set.

import { appendFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";

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
    normalized.getUTCFullYear() !== year
    || normalized.getUTCMonth() + 1 !== month
    || normalized.getUTCDate() !== day
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
    const title = merge ? body.split(/\r?\n/u).find((line) => line.trim())?.trim() || subject : subject;
    if (/^Bump \S+ from \S+ to \S+/u.test(title) || (merge && merge[2].includes("dependabot/"))) continue;
    const reference = merge && !title.includes(`#${merge[1]}`) ? ` (#${merge[1]})` : "";
    lines.push(`* ${title}${reference}`);
  }
  return lines.length > 0 ? lines.join("\n") : "* Maintenance release";
}

function git(...args) {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

function previousReleaseTag(sha) {
  try {
    return git("describe", "--tags", "--abbrev=0", "--first-parent", "--match", "v[0-9]*.[0-9]*.[0-9]*.[0-9]*", `${sha}^`).trim();
  } catch {
    return null;
  }
}

function commitsSince(tag, sha) {
  const range = tag ? `${tag}..${sha}` : sha;
  const limit = tag ? [] : ["--max-count=20"];
  const separator = "\u001e";
  return git("log", "--first-parent", ...limit, `--format=%s%x1f%b${separator}`, range)
    .split(separator)
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [subject, body = ""] = entry.split("\u001f");
      return { subject: subject.trim(), body };
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

function main() {
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
  throw new Error("usage: node scripts/release.mjs <version|notes>");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
