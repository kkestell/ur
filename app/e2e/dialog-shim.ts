export { message } from "../node_modules/@tauri-apps/plugin-dialog/dist-js/index.js";

type DialogState = { confirm?: boolean; lastQuestion?: string; folder?: string };
declare global { interface Window { __UR_E2E_DIALOG__?: DialogState } }

/** Lets WebDriver answer a native confirmation during end-to-end tests. */
export async function ask(question: string): Promise<boolean> {
  const state = window.__UR_E2E_DIALOG__ ??= {};
  state.lastQuestion = question;
  const answer = state.confirm ?? false;
  state.confirm = undefined;
  return answer;
}

/** Lets WebDriver answer the folder picker during end-to-end tests. */
export async function open(): Promise<string | null> {
  const state = window.__UR_E2E_DIALOG__ ??= {};
  const folder = state.folder ?? null;
  state.folder = undefined;
  return folder;
}
