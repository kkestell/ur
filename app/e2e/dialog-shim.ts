export { message, open } from "../node_modules/@tauri-apps/plugin-dialog/dist-js/index.js";

type DialogState = { confirm?: boolean; lastQuestion?: string };
declare global { interface Window { __UR_E2E_DIALOG__?: DialogState } }

/** Lets WebDriver answer a native confirmation during end-to-end tests. */
export async function ask(question: string): Promise<boolean> {
  const state = window.__UR_E2E_DIALOG__ ??= {};
  state.lastQuestion = question;
  const answer = state.confirm ?? false;
  state.confirm = undefined;
  return answer;
}
