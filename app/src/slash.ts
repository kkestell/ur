import type { AvailableCommand } from "@agentclientprotocol/sdk";

/**
 * The command name typed so far, when the text is `/` followed by no
 * whitespace, and otherwise `undefined`.
 */
export function slashQuery(text: string): string | undefined {
  return /^\/\S*$/.test(text) ? text.slice(1) : undefined;
}

/** The commands whose names start with the query, in the server's order. */
export function matchingCommands(commands: AvailableCommand[], query: string): AvailableCommand[] {
  return commands.filter((command) => command.name.startsWith(query));
}
