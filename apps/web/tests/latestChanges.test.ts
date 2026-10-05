import {
  CHANGE_COUNT,
  formatRelativeTime,
  selectLatestChanges,
  toLatestChange,
  type GitHubCommit,
} from "../src/latestChanges";

const REPOSITORY = "joswayski/dbm";

function commit(message: string, overrides: Partial<GitHubCommit> = {}): GitHubCommit {
  return {
    sha: `sha-${message}`,
    html_url: `https://github.com/${REPOSITORY}/commit/sha`,
    author: { login: "joswayski" },
    commit: {
      message,
      committer: { date: "2026-10-04T12:00:00Z" },
      author: { name: "Jose Valerio", date: "2026-10-04T11:00:00Z" },
    },
    ...overrides,
  };
}

describe("toLatestChange", () => {
  it("links squash merges to their pull request and drops the number from the title", () => {
    expect(toLatestChange(commit("Remember open connections (#54)\n\nBody"), REPOSITORY)).toEqual({
      sha: "sha-Remember open connections (#54)\n\nBody",
      title: "Remember open connections",
      url: `https://github.com/${REPOSITORY}/pull/54`,
      committedAt: "2026-10-04T12:00:00Z",
    });
  });

  it("links direct commits to the commit page", () => {
    const change = toLatestChange(commit("Fix a typo"), REPOSITORY);
    expect(change.title).toBe("Fix a typo");
    expect(change.url).toBe(`https://github.com/${REPOSITORY}/commit/sha`);
  });

  it("rejects incomplete entries", () => {
    expect(() => toLatestChange(commit(""), REPOSITORY)).toThrow(/incomplete/);
  });
});

describe("selectLatestChanges", () => {
  it("drops Dependabot updates and keeps the newest product changes", () => {
    const entries = [
      commit("Bump vite from 8.2.0 to 8.2.1 (#60)"),
      commit("Update a lockfile", { author: { login: "dependabot[bot]" } }),
      ...Array.from({ length: CHANGE_COUNT + 2 }, (_, index) => commit(`Change ${index} (#${index})`)),
    ];

    const changes = selectLatestChanges(entries, REPOSITORY);
    expect(changes).toHaveLength(CHANGE_COUNT);
    expect(changes[0]?.title).toBe("Change 0");
  });
});

describe("formatRelativeTime", () => {
  const now = Date.parse("2026-10-04T12:00:00Z");

  it.each([
    ["2026-10-04T11:59:30Z", "30 seconds ago"],
    ["2026-10-04T10:00:00Z", "2 hours ago"],
    ["2026-10-01T12:00:00Z", "3 days ago"],
    ["2026-08-04T12:00:00Z", "2 months ago"],
  ])("formats %s as %s", (date, expected) => {
    expect(formatRelativeTime(date, now)).toBe(expected);
  });
});
