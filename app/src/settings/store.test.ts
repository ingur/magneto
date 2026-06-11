import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Config, SetConfigResp } from "@/daemon/protocol";

// The store depends on the daemon client + toasts; stub both.
const daemonMock = vi.hoisted(() => ({
  config: null as Config | null,
  request: vi.fn(),
}));
vi.mock("@/daemon/client.svelte", () => ({ daemon: daemonMock }));
vi.mock("@/lib/feedback/toasts/toasts.svelte", () => ({
  toast: { success: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}));

import { settings } from "./store.svelte";

function makeConfig(): Config {
  return {
    network: { control_port: 61481, lan_port: 61482, upnp_enabled: true, server_name: "magneto" },
    downloads: {
      dir: "/d",
      fallback_app: "",
      fallback_args: [],
      auto_download: true,
      persist_by_default: false,
      share_by_default: false,
      autoplay: true,
    },
    media: { extensions: ["mkv"] },
    player: { command: "", args: [] },
  };
}

function applied(over: Partial<Config["network"]>, restart: boolean): SetConfigResp {
  const config = makeConfig();
  config.network = { ...config.network, ...over };
  return {
    config,
    restart_required: restart,
    pending_restart: restart ? ["network.control_port"] : [],
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  daemonMock.config = makeConfig();
  settings.end();
});

describe("settings store", () => {
  it("begin seeds committed + working from the live config; not dirty", () => {
    settings.begin();
    expect(settings.working?.serverName).toBe("magneto");
    expect(settings.committed?.serverName).toBe("magneto");
    expect(settings.dirty).toBe(false);
  });

  it("an edit makes it dirty; reset reverts to committed", () => {
    settings.begin();
    settings.working!.serverName = "homelab";
    expect(settings.dirty).toBe(true);
    settings.reset();
    expect(settings.working?.serverName).toBe("magneto");
    expect(settings.dirty).toBe(false);
  });

  it("save sends the full config and re-seeds the baseline from resp.config", async () => {
    settings.begin();
    settings.working!.serverName = "homelab";
    daemonMock.request.mockResolvedValueOnce(applied({ server_name: "homelab" }, false));
    await settings.save();
    expect(daemonMock.request).toHaveBeenCalledWith(
      "set_config",
      expect.objectContaining({ network: expect.objectContaining({ server_name: "homelab" }) }),
    );
    expect(settings.committed?.serverName).toBe("homelab");
    expect(settings.dirty).toBe(false);
  });

  it("a restart-required save fires restart automatically", async () => {
    settings.begin();
    settings.working!.controlPort = "50000";
    daemonMock.request
      .mockResolvedValueOnce(applied({ control_port: 50000 }, true))
      .mockResolvedValueOnce({ ok: true });
    await settings.save();
    expect(daemonMock.request).toHaveBeenNthCalledWith(1, "set_config", expect.anything());
    expect(daemonMock.request).toHaveBeenNthCalledWith(2, "restart");
  });

  it("a hot save does not restart", async () => {
    settings.begin();
    settings.working!.autoplay = false;
    daemonMock.request.mockResolvedValueOnce(applied({}, false));
    await settings.save();
    expect(daemonMock.request).toHaveBeenCalledTimes(1);
  });

  it("a rejected save preserves working and never restarts", async () => {
    settings.begin();
    settings.working!.serverName = "homelab";
    daemonMock.request.mockRejectedValueOnce(new Error("config rejected: nope"));
    await settings.save();
    expect(settings.working?.serverName).toBe("homelab");
    expect(settings.dirty).toBe(true);
    expect(daemonMock.request).toHaveBeenCalledTimes(1);
  });

  it("validate blocks an invalid save before sending", async () => {
    settings.begin();
    settings.working!.controlPort = "0";
    await settings.save();
    expect(daemonMock.request).not.toHaveBeenCalled();
  });
});
