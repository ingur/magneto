import { describe, expect, it } from "vitest";

import type { Config } from "@/daemon/protocol";
import { joinArgs, splitArgs, toConfig, toEditable, validate } from "./config";

function sampleConfig(): Config {
  return {
    network: { control_port: 61481, lan_port: 61482, upnp_enabled: true, server_name: "magneto" },
    downloads: {
      dir: "/home/u/downloads/magneto",
      fallback_app: "xdg-open",
      fallback_args: ["--foo", "bar"],
      auto_download: true,
      persist_by_default: false,
      share_by_default: false,
      autoplay: true,
    },
    media: { extensions: ["mkv", "mp4", "avi"] },
    player: { command: "mpv", args: ["--fs", "--no-border"] },
  };
}

describe("toEditable / toConfig", () => {
  it("round-trips a config through the editable form", () => {
    const c = sampleConfig();
    expect(toConfig(toEditable(c))).toEqual(c);
  });

  it("joins arrays + ports for display", () => {
    const e = toEditable(sampleConfig());
    expect(e.mediaExtensions).toBe("mkv, mp4, avi");
    expect(e.playerArgs).toBe("--fs --no-border");
    expect(e.fallbackArgs).toBe("--foo bar");
    expect(e.controlPort).toBe("61481");
  });

  it("normalizes extensions (lowercase, trim, drop empties)", () => {
    const e = { ...toEditable(sampleConfig()), mediaExtensions: " MKV , mp4 ,, AvI " };
    expect(toConfig(e).media.extensions).toEqual(["mkv", "mp4", "avi"]);
  });

  it("splits args on any whitespace, dropping empties", () => {
    const e = { ...toEditable(sampleConfig()), playerArgs: "  --fs   --no-border  " };
    expect(toConfig(e).player.args).toEqual(["--fs", "--no-border"]);
  });

  it("round-trips args containing whitespace through quoting", () => {
    const args = ["--title=My Player", "--fs", 'has"quote', "C:\\Program Files\\x"];
    expect(splitArgs(joinArgs(args))).toEqual(args);
  });

  it("leaves plain args (incl. unspaced Windows paths) unquoted", () => {
    expect(joinArgs(["--fs", "C:\\vlc.exe"])).toBe("--fs C:\\vlc.exe");
  });

  it("preserves an explicitly-quoted empty arg but drops whitespace gaps", () => {
    expect(splitArgs('a "" b')).toEqual(["a", "", "b"]);
    expect(splitArgs("  a   b  ")).toEqual(["a", "b"]);
  });

  it("parses ports back to numbers", () => {
    const e = { ...toEditable(sampleConfig()), controlPort: "50000", lanPort: "50001" };
    const c = toConfig(e);
    expect(c.network.control_port).toBe(50000);
    expect(c.network.lan_port).toBe(50001);
  });

  it("maps autoplay + fallback under downloads (UI-section divergence)", () => {
    const c = toConfig(toEditable(sampleConfig()));
    expect(c.downloads.autoplay).toBe(true);
    expect(c.downloads.fallback_app).toBe("xdg-open");
  });
});

describe("validate", () => {
  const base = toEditable(sampleConfig());

  it("accepts a valid form", () => {
    expect(validate(base)).toBeNull();
  });

  it("rejects a non-numeric or out-of-range port", () => {
    expect(validate({ ...base, controlPort: "0" })).toMatch(/control port/i);
    expect(validate({ ...base, controlPort: "70000" })).toMatch(/control port/i);
    expect(validate({ ...base, lanPort: "abc" })).toMatch(/lan port/i);
  });

  it("rejects non-decimal port forms Number() would silently accept", () => {
    expect(validate({ ...base, controlPort: "0x1388" })).toMatch(/control port/i);
    expect(validate({ ...base, controlPort: "1e4" })).toMatch(/control port/i);
  });

  it("rejects equal ports only when UPnP is enabled", () => {
    expect(validate({ ...base, controlPort: "5000", lanPort: "5000", upnpEnabled: true })).toMatch(
      /differ/i,
    );
    expect(
      validate({ ...base, controlPort: "5000", lanPort: "5000", upnpEnabled: false }),
    ).toBeNull();
  });

  it("rejects an empty extension list", () => {
    expect(validate({ ...base, mediaExtensions: "  , ,, " })).toMatch(/extension/i);
  });
});
