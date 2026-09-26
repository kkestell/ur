import { Menu as NativeMenu, type MenuOptions } from "@tauri-apps/api/menu/menu";

type MenuEntry = { text?: string; enabled?: boolean; handler?: { onmessage: (value: unknown) => void } };
type MenuState = { items: MenuEntry[]; pick?: string };

declare global {
  interface Window { __UR_E2E_MENU__?: MenuState }
}

/** Constructs the real menu, then lets WebDriver select its registered action. */
export const Menu = {
  async new(options: MenuOptions): Promise<NativeMenu> {
    const menu = await NativeMenu.new(options);
    const state = window.__UR_E2E_MENU__ ??= { items: [] };
    state.items = (options.items ?? []) as MenuEntry[];
    menu.popup = async () => {
      const selected = state.items.find((item) => item.text === state.pick);
      state.pick = undefined;
      selected?.handler?.onmessage(undefined);
    };
    return menu;
  },
};
