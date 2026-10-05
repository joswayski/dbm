import {
  CHANGE_COUNT,
  formatRelativeTime,
  parseCommitFeed,
  selectLatestChanges,
  toLatestChange,
  type FeedEntry,
} from "../src/latestChanges";

const REPOSITORY = "joswayski/dbm";
const SHA = "8d64073752c2aede7004bf0126ad825414d8401d";

function feedEntry(title: string, author = "joswayski", sha = SHA): string {
  return `
  <entry>
    <id>tag:github.com,2008:Grit::Commit/${sha}</id>
    <link type="text/html" rel="alternate" href="https://github.com/${REPOSITORY}/commit/${sha}"/>
    <title>
        ${title}
    </title>
    <updated>2026-10-05T00:25:03Z</updated>
    <media:thumbnail height="30" width="30" url="https://avatars.githubusercontent.com/u/1?s=30&amp;v=4"/>
    <author>
      <name>${author}</name>
      <uri>https://github.com/${author}</uri>
    </author>
    <content type="html">&lt;pre&gt;${title}&lt;/pre&gt;</content>
  </entry>`;
}

function feed(...entries: string[]): string {
  return `<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/" xml:lang="en-US">
  <title>Recent Commits to dbm:main</title>
  <updated>2026-10-05T00:25:03Z</updated>
  ${entries.join("\n")}
</feed>`;
}

function entry(title: string, author = "joswayski"): FeedEntry {
  return {
    link: `https://github.com/${REPOSITORY}/commit/${SHA}`,
    title,
    updated: "2026-10-05T00:25:03Z",
    author,
  };
}

describe("parseCommitFeed", () => {
  it("reads each entry's link, trimmed title, date, and author", () => {
    expect(parseCommitFeed(feed(feedEntry("Add the anyba.se website (#59)")))).toEqual([
      {
        link: `https://github.com/${REPOSITORY}/commit/${SHA}`,
        title: "Add the anyba.se website (#59)",
        updated: "2026-10-05T00:25:03Z",
        author: "joswayski",
      },
    ]);
  });

  it("decodes XML entities in titles", () => {
    const [parsed] = parseCommitFeed(feed(feedEntry("Fix &amp; tidy &lt;Grid&gt; &#39;cells&#39;")));
    expect(parsed?.title).toBe("Fix & tidy <Grid> 'cells'");
  });

  it("rejects entries without a title", () => {
    expect(() => parseCommitFeed(feed(feedEntry("")))).toThrow(/incomplete/);
  });
});

describe("toLatestChange", () => {
  it("links squash merges to their pull request and drops the number from the title", () => {
    expect(toLatestChange(entry("Remember open connections (#54)"), REPOSITORY)).toEqual({
      sha: SHA,
      title: "Remember open connections",
      url: `https://github.com/${REPOSITORY}/pull/54`,
      committedAt: "2026-10-05T00:25:03Z",
    });
  });

  it("links direct commits to the commit page", () => {
    const change = toLatestChange(entry("Fix a typo"), REPOSITORY);
    expect(change.title).toBe("Fix a typo");
    expect(change.url).toBe(`https://github.com/${REPOSITORY}/commit/${SHA}`);
  });
});

describe("selectLatestChanges", () => {
  it("drops Dependabot updates and keeps the newest product changes", () => {
    const entries = [
      entry("Bump vite from 8.2.0 to 8.2.1 (#60)"),
      entry("Update a lockfile", "dependabot[bot]"),
      ...Array.from({ length: CHANGE_COUNT + 2 }, (_, index) => entry(`Change ${index} (#${index})`)),
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
