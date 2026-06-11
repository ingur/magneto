// Desktop-notification policy: the mute preference (local, persisted) and the
// when-to-fire gates. The preference is separate from the OS permission
// requested at launch (tauri.ts initNotifications); both must allow it for
// one to show. Defaults on. Mirrors the theme/zoom local-pref shape.

import { notify } from "@/daemon/tauri";

const STORAGE_KEY = "magneto:notifications";

function read(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) !== "off";
  } catch {
    // localStorage blocked (private mode); default on, in-memory.
    return true;
  }
}

let enabled = $state<boolean>(read());

export const notifications = {
  get enabled(): boolean {
    return enabled;
  },
  set enabled(next: boolean) {
    enabled = next;
    try {
      localStorage.setItem(STORAGE_KEY, next ? "on" : "off");
    } catch {
      // blocked; stays in-memory for the session.
    }
  },
};

/** OS notification when the app is backgrounded: another window focused, or
 *  hidden to tray. A focused app shows the toast instead. The visibility
 *  check is load-bearing for tray-hidden: a hidden window keeps
 *  document.hasFocus() true (focus never moves away from the page's
 *  perspective), so a focus check alone would suppress exactly the
 *  notifications the tray state exists for. */
export function notifyBackground(title: string, body: string): void {
  const backgrounded = document.visibilityState !== "visible" || !document.hasFocus();
  if (backgrounded && enabled) notify(title, body);
}
