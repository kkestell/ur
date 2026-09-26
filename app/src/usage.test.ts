import { expect, test } from "vitest";
import { usageText } from "./usage";

test.each([
  {
    name: "below 1000 tokens, without cost",
    usage: { used: 420, size: 800 },
    text: { tokens: "420 / 800 tokens (53%)", cost: null },
  },
  {
    name: "above 1000 tokens, with cost",
    usage: { used: 84_000, size: 200_000, cost: { amount: 1.27, currency: "USD" } },
    text: { tokens: "84k / 200k tokens (42%)", cost: "Cost $1.27" },
  },
  {
    name: "a few thousand tokens",
    usage: { used: 1200, size: 8000, cost: { amount: 0.25, currency: "USD" } },
    text: { tokens: "1.2k / 8k tokens (15%)", cost: "Cost $0.25" },
  },
  {
    name: "an unknown currency code",
    usage: { used: 0, size: 1000, cost: { amount: 2, currency: "not a code" } },
    text: { tokens: "0 / 1k tokens (0%)", cost: "Cost 2 not a code" },
  },
])("usage_text: $name", ({ usage, text }) => {
  expect(usageText(usage)).toEqual(text);
});
