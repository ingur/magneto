// Settings edit lifecycle. A frozen `committed` baseline + a `working` copy:
// dirty = working ≠ committed. committed is seeded from the live config on open
// and re-seeded from the daemon's *applied* config after a save, so an external
// config_changed (which updates daemon.config) never shifts the user's dirty
// state. save() projects to the nested Config, sends it, and, when the daemon
// reports a restart is required, restarts automatically (the user never decides).

import { daemon } from "@/daemon/client.svelte";
import type { SetConfigResp } from "@/daemon/protocol";
import { toast } from "@/lib/feedback/toasts/toasts.svelte";
import { type EditableConfig, toConfig, toEditable, validate } from "./config";

class SettingsStore {
  committed = $state<EditableConfig | null>(null);
  working = $state<EditableConfig | null>(null);
  saving = $state(false);

  dirty = $derived(
    this.working !== null &&
      this.committed !== null &&
      JSON.stringify(this.working) !== JSON.stringify(this.committed),
  );

  // Seed a fresh edit session from the live config. Idempotent: a no-op once
  // editing, so a config_changed mid-edit cannot reseed over the working copy.
  begin(): void {
    if (this.working || !daemon.config) return;
    this.committed = toEditable(daemon.config);
    this.working = { ...this.committed };
  }

  // Revert edits to the committed baseline (the Reset button).
  reset(): void {
    if (this.committed) this.working = { ...this.committed };
  }

  // Drop the edit session on close; the next open reseeds from the then-current config.
  end(): void {
    this.working = null;
    this.committed = null;
  }

  async save(): Promise<void> {
    if (!this.working || !this.dirty || this.saving) return;
    const error = validate(this.working);
    if (error) {
      toast.error(error);
      return;
    }
    this.saving = true;
    const sent = JSON.stringify(this.working);
    try {
      const resp = await daemon.request<SetConfigResp>("set_config", toConfig(this.working));
      // Re-seed the baseline from the APPLIED config the daemon returned, so a
      // successful save clears dirty without depending on event ordering.
      this.committed = toEditable(resp.config);
      // Snap working to that baseline only if the user did not edit during the
      // request: clears dirty (even when the daemon normalized a value) without
      // clobbering in-flight edits.
      if (this.working && JSON.stringify(this.working) === sent) {
        this.working = { ...this.committed };
      }
      if (resp.restart_required) {
        toast.info("Settings saved. Restarting the daemon…");
        // Fire-and-forget: the reconnect loop rides the restart and refreshes.
        daemon
          .request("restart")
          .catch(() =>
            toast.warn("Couldn't restart the daemon. Restart it to apply your settings"),
          );
      } else {
        toast.success("Settings saved");
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      this.saving = false;
    }
  }
}

export const settings = new SettingsStore();
