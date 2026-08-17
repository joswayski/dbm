import { describe, expect, it } from "vitest";

import { commandExecutionTarget } from "./commandSelection";

describe("commandExecutionTarget", () => {
  it("runs the current Redis line", () => {
    const text = "PING\nHGETALL user:1";
    expect(commandExecutionTarget(text, 0, 0)).toMatchObject({ kind: "statement", sql: "PING" });
    expect(commandExecutionTarget(text, text.indexOf("HGETALL"), text.indexOf("HGETALL"))).toMatchObject({
      kind: "statement",
      sql: "HGETALL user:1",
    });
  });

  it("prefers an explicit selection", () => {
    const text = "GET a\nSET b c";
    expect(commandExecutionTarget(text, 0, text.length)).toMatchObject({
      kind: "selection",
      sql: "GET a\nSET b c",
    });
  });
});
