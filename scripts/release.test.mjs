import assert from "node:assert/strict";
import { test } from "vitest";

import { highlightNotes, nextReleaseVersion, releaseDate, releaseHistory, releaseNotes } from "./release.mjs";

test("uses the New York calendar date across UTC midnight", () => {
  assert.equal(releaseDate(new Date("2026-09-25T02:30:00Z")), "2026-09-24");
  assert.equal(releaseDate(new Date("2026-09-25T05:30:00Z")), "2026-09-25");
});

test("starts each day at revision 1 and increments within the day", () => {
  assert.deepEqual(nextReleaseVersion("2026-09-24", ["v2026.09.23.4"]), {
    date: "2026-09-24",
    revision: 1,
    displayVersion: "2026.09.24.1",
    tag: "v2026.09.24.1",
    appVersion: "2026.9.2401",
  });
  const next = nextReleaseVersion("2026-09-24", ["v2026.09.24.1", "v2026.09.24.2", "other"]);
  assert.equal(next.tag, "v2026.09.24.3");
  assert.equal(next.appVersion, "2026.9.2403");
});

test("app versions keep increasing across days and months", () => {
  const compare = (left, right) => {
    const [a, b] = [left, right].map((version) => version.split(".").map(Number));
    return a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
  };
  const lateInMonth = nextReleaseVersion(
    "2026-09-30",
    Array.from({ length: 98 }, (_, index) => `v2026.09.30.${index + 1}`),
  );
  const nextMonth = nextReleaseVersion("2026-10-01", []);
  assert.equal(lateInMonth.appVersion, "2026.9.3099");
  assert.ok(compare(nextMonth.appVersion, lateInMonth.appVersion) > 0);
  assert.ok(compare(nextReleaseVersion("2026-09-24", []).appVersion, "0.1.0") > 0);
});

test("rejects malformed tags, impossible dates, and a 100th daily release", () => {
  assert.throws(() => nextReleaseVersion("2026-09-24", ["v2026.09.24.x"]), /malformed/);
  assert.throws(() => nextReleaseVersion("2026-02-30", []), /real calendar date/);
  assert.throws(
    () =>
      nextReleaseVersion(
        "2026-09-24",
        Array.from({ length: 99 }, (_, index) => `v2026.09.24.${index + 1}`),
      ),
    /at most 99/,
  );
});

test("summarizes merged pull requests and skips Dependabot", () => {
  const notes = releaseNotes([
    { subject: "Merge pull request #24 from joswayski/feature", body: "Add per-merge releases\n\nDetails" },
    { subject: "Merge pull request #9 from joswayski/dependabot/npm_and_yarn/postcss-8.5.26", body: "Bump postcss" },
    { subject: "Bump js-yaml from 4.3.0 to 4.3.1 (#8)", body: "" },
    { subject: "Fix schema refresh (#20)", body: "" },
    { subject: "Tidy docs", body: "" },
  ]);
  assert.equal(notes, "* Add per-merge releases (#24)\n* Fix schema refresh (#20)\n* Tidy docs");
  assert.equal(releaseNotes([]), "* Maintenance release");
});

const entry = (build, title = `Release ${build}`) => ({
  build,
  version: `2026.10.18.${build}`,
  commit: String(build).padStart(40, "0"),
  publishedAt: "2026-10-18T12:00:00Z",
  highlights: [{ title, body: `${title} body` }],
});

test("release history puts the new release first and keeps valid entries only", () => {
  const previous = [
    entry(2),
    entry(1),
    { ...entry(0), commit: "abc1234" },
    { ...entry(3), extra: "dropped" },
    "not a release",
  ];
  const history = releaseHistory(previous, entry(4));
  assert.deepEqual(
    history.map(({ build }) => build),
    [4, 3, 2, 1],
  );
  assert.equal("extra" in history[1], false);
  // A rerun replaces its own entry instead of listing it twice.
  assert.deepEqual(
    releaseHistory(history, entry(4, "Rewritten")).map(({ highlights }) => highlights[0].title),
    ["Rewritten", "Release 3", "Release 2", "Release 1"],
  );
  // Without new highlights the history carries on unchanged.
  assert.deepEqual(releaseHistory(previous, null), history.slice(1));
  assert.deepEqual(releaseHistory(undefined, null), []);
});

test("release history stays within its count and size caps", () => {
  const many = Array.from({ length: 40 }, (_, index) => entry(index + 1));
  assert.equal(releaseHistory(many, entry(41)).length, 30);
  const large = Array.from({ length: 20 }, (_, index) => entry(index + 1, "x".repeat(5_000)));
  const kept = releaseHistory(large, null);
  assert.ok(JSON.stringify(kept).length <= 64 * 1024);
  assert.equal(kept[0].build, 20);
});

test("highlights read as plain notes for the release page and older installs", () => {
  assert.equal(
    highlightNotes([
      { title: "Copy rows as CSV", body: "Paste them anywhere." },
      { title: "Saved filters", body: "They stay put." },
    ]),
    "* Copy rows as CSV — Paste them anywhere.\n* Saved filters — They stay put.",
  );
});
