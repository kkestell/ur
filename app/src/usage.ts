import type { UsageUpdate } from "@agentclientprotocol/sdk";

/**
 * The usage indicator's popover lines: tokens used of the context size, such
 * as "84k / 200k tokens (42%)", and the cost, such as "Cost $1.27", when the
 * server reported one.
 */
export function usageText(usage: UsageUpdate): { tokens: string; cost: string | null } {
  const percent = usage.size > 0 ? Math.round((usage.used / usage.size) * 100) : 0;
  const tokens = `${count(usage.used)} / ${count(usage.size)} tokens (${percent}%)`;
  if (usage.cost == null) {
    return { tokens, cost: null };
  }
  return { tokens, cost: `Cost ${money(usage.cost.amount, usage.cost.currency)}` };
}

/** A token count: exact below 1000, then in thousands. */
function count(tokens: number): string {
  if (tokens < 1000) {
    return String(tokens);
  }
  const thousands = tokens / 1000;
  return `${thousands < 10 ? Number(thousands.toFixed(1)) : Math.round(thousands)}k`;
}

/** The amount in the currency, or the amount and the code when the code is unknown. */
function money(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat("en-US", { style: "currency", currency }).format(amount);
  } catch {
    return `${amount} ${currency}`;
  }
}
