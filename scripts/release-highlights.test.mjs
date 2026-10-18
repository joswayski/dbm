import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, test } from "vitest";

import { pathApps, releaseHighlights } from "./release-highlights.mjs";

const directories = [];
afterEach(() => directories.splice(0).forEach((directory) => rmSync(directory, { recursive: true, force: true })));

function fixture() {
  const cwd = mkdtempSync(join(tmpdir(), "dbm-highlights-"));
  directories.push(cwd);
  const git = (...args) =>
    execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  git("init", "-b", "main");
  git("config", "user.name", "DBM Test");
  git("config", "user.email", "test@example.com");
  const commit = (files, message) => {
    for (const [path, text] of Object.entries(files)) {
      mkdirSync(dirname(join(cwd, path)), { recursive: true });
      writeFileSync(join(cwd, path), text);
    }
    git("add", ".");
    git("commit", "-m", message);
    return git("rev-parse", "HEAD");
  };
  return { cwd, git, commit, base: commit({ "README.md": "Anybase" }, "Base") };
}

function reply(highlights, extra = {}) {
  return Response.json({
    type: "message",
    stop_reason: "end_turn",
    content: [{ type: "text", text: JSON.stringify({ highlights }) }],
    ...extra,
  });
}
const env = { ANTHROPIC_API_KEY: "test-key" };
const highlight = (commit) => ({
  title: "Copy rows as CSV",
  body: "Select rows and copy them straight into a spreadsheet.",
  commits: [commit],
});

test("only desktop app sources count, each tagged with the apps it reaches", () => {
  assert.deepEqual(pathApps("apps/native/macos/Sources/Grid.swift"), ["macOS"]);
  assert.deepEqual(pathApps("apps/native/workbench/src/workbench.rs"), ["Windows", "Linux"]);
  assert.deepEqual(pathApps("crates/dbm-core/src/lib.rs"), ["macOS", "Windows", "Linux"]);
  for (const path of [
    "apps/web/src/routes/index.tsx",
    "apps/native/android/app/src/main/java/com/dbm/nativeapp/MainActivity.kt",
    "apps/native/ios/Sources/App.swift",
    "crates/dbm-core/tests/postgres.rs",
    "crates/dbm-core/src/tests.rs",
    "crates/dbm-core/src/secret_store.rs",
    "docs/releases.md",
    "scripts/release.mjs",
  ])
    assert.deepEqual(pathApps(path), [], path);
});

test("summarizes desktop changes, including merged pull requests, for Opus", async () => {
  const f = fixture();
  f.commit({ "apps/web/src/page.tsx": "WEB-ONLY" }, "Improve the website");
  f.commit({ "crates/dbm-core/Cargo.toml": "DEPENDENCY" }, "Bump serde from 1.0.0 to 1.0.1");
  f.git("checkout", "-b", "feature");
  f.commit(
    {
      "apps/native/workbench/src/table_view.rs": "COPY-AS-CSV",
      "crates/dbm-core/tests/fixtures.rs": "TEST-ONLY",
      "apps/native/android/app/src/main/java/Main.kt": "MOBILE-ONLY",
    },
    "Copy selected rows as CSV",
  );
  f.git("checkout", "main");
  f.git("merge", "--no-ff", "feature", "-m", "Merge pull request #7 from someone/csv\n\nCopy rows as CSV");
  const after = f.git("rev-parse", "HEAD");
  f.commit({ "apps/native/macos/Sources/Grid.swift": "FUTURE-CHANGE" }, "Later change");
  const recentHighlights = [{ title: "Saved filters", body: "Filters stay put." }];
  let request;
  const highlights = await releaseHighlights(f.base, after, {
    cwd: f.cwd,
    env,
    recentHighlights,
    fetch: async (url, options) => {
      assert.equal(url, "https://api.anthropic.com/v1/messages");
      assert.equal(options.headers["x-api-key"], "test-key");
      assert.equal(options.headers["anthropic-beta"], "server-side-fallback-2026-07-01");
      request = JSON.parse(options.body);
      return reply([highlight(after)], {
        content: [
          { type: "thinking", thinking: "not release prose" },
          { type: "fallback" },
          { type: "text", text: JSON.stringify({ highlights: [highlight(after)] }) },
        ],
      });
    },
  });
  assert.deepEqual(highlights, [
    { title: "Copy rows as CSV", body: "Select rows and copy them straight into a spreadsheet." },
  ]);
  assert.equal(request.model, "claude-opus-5-5");
  assert.equal(request.output_config.effort, "high");
  assert.equal(request.max_tokens, 16000);
  assert.equal(request.fallbacks, "default");
  const source = JSON.parse(request.messages[0].content);
  assert.deepEqual(source.recentHighlights, recentHighlights);
  assert.equal(source.commits.length, 1);
  assert.equal(source.commits[0].commit, after);
  assert.equal(source.commits[0].subject, "Copy rows as CSV");
  assert.deepEqual(source.commits[0].apps, ["Windows", "Linux"]);
  assert.match(source.commits[0].diff, /COPY-AS-CSV/u);
  assert.doesNotMatch(request.messages[0].content, /WEB-ONLY|DEPENDENCY|TEST-ONLY|MOBILE-ONLY|FUTURE-CHANGE/u);
});

test("oversized diffs keep every commit and mark truncation within the request budget", async () => {
  const f = fixture();
  const first = f.commit({ "crates/dbm-core/src/lib.rs": "a".repeat(450_000) }, "Faster grid");
  const after = f.commit({ "apps/native/macos/Sources/Grid.swift": "b".repeat(230_000) }, "Sharper grid on macOS");
  let source;
  const highlights = await releaseHighlights(f.base, after, {
    cwd: f.cwd,
    env: { ...env, ANTHROPIC_MODEL: "custom-model", ANTHROPIC_EFFORT: "medium" },
    fetch: async (_, options) => {
      assert.ok(options.body.length <= 400_000);
      const request = JSON.parse(options.body);
      assert.equal(request.model, "custom-model");
      assert.equal(request.output_config.effort, "medium");
      source = JSON.parse(request.messages[0].content);
      return reply([]);
    },
  });
  assert.deepEqual(
    source.commits.map(({ commit }) => commit),
    [first, after],
  );
  assert.ok(source.commits.every(({ diff, diffTruncated }) => diff.length > 0 && diffTruncated));
  assert.deepEqual(highlights, []);
});

test("invalid or unavailable editorial output returns null without leaking errors", async () => {
  const f = fixture();
  const after = f.commit({ "crates/dbm-core/src/lib.rs": "query" }, "Faster queries");
  const good = highlight(after);
  const cases = [
    async () => {
      throw new Error("sensitive provider body");
    },
    async () => new Response("sensitive provider body", { status: 429 }),
    async () => new Response("x".repeat(128 * 1024 + 1)),
    async () => new Response("malformed JSON"),
    async () => reply([good], { stop_reason: "max_tokens" }),
    async () => reply([good], { content: [{ type: "tool_use" }] }),
    async () => reply([{ ...good, commits: [f.base] }]),
    async () => reply([{ ...good, commits: [after, after] }]),
    async () => reply([{ ...good, body: "<script>unsafe</script>" }]),
    async () => reply([{ ...good, body: "Queries are now 50% faster" }]),
    async () => reply([{ ...good, title: "x".repeat(101) }]),
    async () => reply([{ ...good, body: "x".repeat(501) }]),
    async () => reply([{ ...good, extra: "unexpected" }]),
    async () => reply(Array(6).fill(good)),
  ];
  for (const fetch of cases) {
    const warnings = [];
    const highlights = await releaseHighlights(f.base, after, {
      cwd: f.cwd,
      env,
      fetch,
      warn: (...args) => warnings.push(args),
    });
    assert.equal(highlights, null);
    assert.deepEqual(warnings, [[]], "no raw provider error or source content reaches diagnostics");
  }
});

test("quiet ranges skip the API; missing key, baseline or budget return null", async () => {
  const f = fixture();
  const after = f.commit(
    { "apps/web/src/page.tsx": "web", "crates/dbm-core/tests/test.rs": "test" },
    "Website and test changes",
  );
  let calls = 0;
  const options = {
    cwd: f.cwd,
    env,
    fetch: async () => {
      calls++;
      throw new Error("unexpected API request");
    },
    warn: () => {},
  };
  assert.deepEqual(await releaseHighlights(f.base, after, options), []);
  assert.equal(await releaseHighlights(null, after, options), null);
  assert.equal(await releaseHighlights(f.base, after, { ...options, env: {} }), null);
  f.commit({ "crates/dbm-core/src/lib.rs": "code" }, "Faster queries");
  // Feed the message over stdin rather than the OS's argument-size limit.
  execFileSync("git", ["commit", "--amend", "-F", "-"], {
    cwd: f.cwd,
    input: "a".repeat(400_001),
    stdio: ["pipe", "ignore", "pipe"],
  });
  assert.equal(await releaseHighlights(f.base, f.git("rev-parse", "HEAD"), options), null);
  assert.equal(calls, 0);
});
