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

function reply(highlights, { parts = [{ text: JSON.stringify({ highlights }) }], finishReason = "STOP" } = {}) {
  return Response.json({ candidates: [{ content: { role: "model", parts }, finishReason }] });
}
const env = { GEMINI_API_KEY: "test-key" };
const highlight = (commit) => ({
  title: "Copy rows as CSV",
  body: "Select rows and copy them straight into a spreadsheet.",
  commits: [commit],
});

test("only desktop app sources count, each tagged with the apps it reaches", () => {
  assert.deepEqual(pathApps("apps/native/macos/Sources/Grid.swift"), ["macOS"]);
  assert.deepEqual(pathApps("apps/native/win32/app/src/view/table.rs"), ["Windows"]);
  assert.deepEqual(pathApps("apps/native/gtk/src/table.rs"), ["Linux"]);
  assert.deepEqual(pathApps("apps/native/desktop/src/updates.rs"), ["Windows", "Linux"]);
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

test("summarizes desktop changes, including merged pull requests, for Gemini", async () => {
  const f = fixture();
  f.commit({ "apps/web/src/page.tsx": "WEB-ONLY" }, "Improve the website");
  f.commit({ "crates/dbm-core/Cargo.toml": "DEPENDENCY" }, "Bump serde from 1.0.0 to 1.0.1");
  f.git("checkout", "-b", "feature");
  f.commit(
    {
      "apps/native/gtk/src/table.rs": "COPY-AS-CSV",
      "apps/native/win32/app/src/view/table.rs": "COPY-AS-CSV",
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
      assert.equal(url, "https://generativelanguage.googleapis.com/v1beta/models/gemini-4-hydrogen:generateContent");
      assert.equal(options.headers["x-goog-api-key"], "test-key");
      request = JSON.parse(options.body);
      return reply([highlight(after)], {
        parts: [
          { text: "not release prose", thought: true },
          { text: JSON.stringify({ highlights: [highlight(after)] }), thoughtSignature: "x" },
        ],
      });
    },
  });
  assert.deepEqual(highlights, [
    { title: "Copy rows as CSV", body: "Select rows and copy them straight into a spreadsheet." },
  ]);
  assert.equal(request.generationConfig.thinkingConfig.thinkingLevel, "MEDIUM");
  assert.equal(request.generationConfig.maxOutputTokens, 32768);
  assert.equal(request.generationConfig.responseMimeType, "application/json");
  assert.equal(request.systemInstruction.parts.length, 1);
  const source = JSON.parse(request.contents[0].parts[0].text);
  assert.deepEqual(source.recentHighlights, recentHighlights);
  assert.equal(source.commits.length, 1);
  assert.equal(source.commits[0].commit, after);
  assert.equal(source.commits[0].subject, "Copy rows as CSV");
  assert.deepEqual(source.commits[0].apps, ["Linux", "Windows"]);
  assert.match(source.commits[0].diff, /COPY-AS-CSV/u);
  assert.doesNotMatch(request.contents[0].parts[0].text, /WEB-ONLY|DEPENDENCY|TEST-ONLY|MOBILE-ONLY|FUTURE-CHANGE/u);
});

test("oversized diffs keep every commit and mark truncation within the request budget", async () => {
  const f = fixture();
  const first = f.commit({ "crates/dbm-core/src/lib.rs": "a".repeat(450_000) }, "Faster grid");
  const after = f.commit({ "apps/native/macos/Sources/Grid.swift": "b".repeat(230_000) }, "Sharper grid on macOS");
  let source;
  const highlights = await releaseHighlights(f.base, after, {
    cwd: f.cwd,
    env: { ...env, GEMINI_MODEL: "custom-model", GEMINI_THINKING_LEVEL: "high" },
    fetch: async (url, options) => {
      assert.ok(options.body.length <= 400_000);
      const request = JSON.parse(options.body);
      assert.match(url, /\/models\/custom-model:generateContent$/u);
      assert.equal(request.generationConfig.thinkingConfig.thinkingLevel, "HIGH");
      source = JSON.parse(request.contents[0].parts[0].text);
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
    async () => reply([good], { finishReason: "MAX_TOKENS" }),
    async () => reply([good], { finishReason: "SAFETY" }),
    async () => reply([good], { parts: [{ text: "secret summary", thought: true }] }),
    async () => Response.json({ promptFeedback: { blockReason: "PROHIBITED_CONTENT" } }),
    async () => reply([good], { parts: [{ functionCall: { name: "run" } }] }),
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
  const changed = f.commit({ "crates/dbm-core/src/lib.rs": "code" }, "Faster queries");
  for (const setting of [
    { GEMINI_MODEL: "../files/x" },
    { GEMINI_MODEL: "Gemini 4 Neon" },
    { GEMINI_THINKING_LEVEL: "minimal" },
    { GEMINI_THINKING_LEVEL: "max" },
  ])
    assert.equal(await releaseHighlights(f.base, changed, { ...options, env: { ...env, ...setting } }), null);
  // Feed the message over stdin rather than the OS's argument-size limit.
  execFileSync("git", ["commit", "--amend", "-F", "-"], {
    cwd: f.cwd,
    input: "a".repeat(400_001),
    stdio: ["pipe", "ignore", "pipe"],
  });
  assert.equal(await releaseHighlights(f.base, f.git("rev-parse", "HEAD"), options), null);
  assert.equal(calls, 0);
});

test("OpenRouter writes the highlights when Gemini fails or answers invalidly", async () => {
  const f = fixture();
  const after = f.commit({ "crates/dbm-core/src/lib.rs": "query" }, "Faster queries");
  const geminiFailures = [
    async () => new Response('{"error":{"status":"RESOURCE_EXHAUSTED"}}', { status: 429 }),
    async () => reply([{ ...highlight(after), commits: [f.base] }]),
  ];
  for (const gemini of geminiFailures) {
    const requests = [];
    const highlights = await releaseHighlights(f.base, after, {
      cwd: f.cwd,
      env: { ...env, OPENROUTER_API_KEY: "router-key" },
      fetch: async (url, options) => {
        requests.push(JSON.parse(options.body));
        if (!url.startsWith("https://openrouter.ai/")) return gemini();
        assert.equal(url, "https://openrouter.ai/api/v1/chat/completions");
        assert.equal(options.headers.Authorization, "Bearer router-key");
        const [geminiRequest, request] = requests;
        assert.equal(request.model, "meta/muse-spark-1.3");
        assert.equal(request.messages[1].content, geminiRequest.contents[0].parts[0].text);
        const content = JSON.stringify({ highlights: [highlight(after)] });
        return Response.json({ choices: [{ finish_reason: "stop", message: { content } }] });
      },
      warn: () => assert.fail("the fallback answered"),
    });
    assert.deepEqual(highlights, [
      { title: "Copy rows as CSV", body: "Select rows and copy them straight into a spreadsheet." },
    ]);
    assert.equal(requests.length, 2);
  }
});
