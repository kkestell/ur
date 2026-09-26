import { expect, test } from "vitest";
import { matchingCommands, slashQuery } from "./slash";

test.each([
  { text: "/", query: "" },
  { text: "/ta", query: "ta" },
  { text: "/tally x", query: undefined },
  { text: "hi /x", query: undefined },
  { text: "", query: undefined },
])("slash_query: $text", ({ text, query }) => {
  expect(slashQuery(text)).toBe(query);
});

const commands = [
  { name: "tally", description: "count the tallies" },
  { name: "tag", description: "tag the tallies" },
  { name: "plan", description: "write a plan" },
];

test.each([
  { query: "", names: ["tally", "tag", "plan"] },
  { query: "ta", names: ["tally", "tag"] },
  { query: "tal", names: ["tally"] },
  { query: "x", names: [] },
])("matching_commands: $query", ({ query, names }) => {
  expect(matchingCommands(commands, query).map((command) => command.name)).toEqual(names);
});
