// The Settings boundary between the flat editing form the controls bind to and
// the daemon's nested Config. toEditable/toConfig are the only place the two
// shapes meet; validate is a light pre-check for instant feedback, the daemon's
// set_config stays the authoritative gate.

import type { Config } from "@/daemon/protocol";

export interface EditableConfig {
  // Downloads
  downloadsDir: string;
  autoDownload: boolean;
  persistDefault: boolean;
  shareDefault: boolean;
  // Player
  playerCommand: string;
  playerArgs: string;
  mediaExtensions: string;
  autoplay: boolean;
  // Fallback
  fallbackApp: string;
  fallbackArgs: string;
  // Network
  controlPort: string;
  lanPort: string;
  serverName: string;
  upnpEnabled: boolean;
}

export function toEditable(c: Config): EditableConfig {
  return {
    downloadsDir: c.downloads.dir,
    autoDownload: c.downloads.auto_download,
    persistDefault: c.downloads.persist_by_default,
    shareDefault: c.downloads.share_by_default,
    playerCommand: c.player.command,
    playerArgs: joinArgs(c.player.args),
    mediaExtensions: c.media.extensions.join(", "),
    autoplay: c.downloads.autoplay,
    fallbackApp: c.downloads.fallback_app,
    fallbackArgs: joinArgs(c.downloads.fallback_args),
    controlPort: String(c.network.control_port),
    lanPort: String(c.network.lan_port),
    serverName: c.network.server_name,
    upnpEnabled: c.network.upnp_enabled,
  };
}

export function toConfig(e: EditableConfig): Config {
  return {
    network: {
      control_port: parsePort(e.controlPort),
      lan_port: parsePort(e.lanPort),
      upnp_enabled: e.upnpEnabled,
      server_name: e.serverName,
    },
    downloads: {
      dir: e.downloadsDir,
      fallback_app: e.fallbackApp,
      fallback_args: splitArgs(e.fallbackArgs),
      auto_download: e.autoDownload,
      persist_by_default: e.persistDefault,
      share_by_default: e.shareDefault,
      autoplay: e.autoplay,
    },
    media: { extensions: splitExtensions(e.mediaExtensions) },
    player: { command: e.playerCommand, args: splitArgs(e.playerArgs) },
  };
}

// Argument list <-> editable string with minimal shell-style quoting, so an
// arg containing whitespace (e.g. `--title=My Player`) survives a round-trip
// instead of being split on the next unrelated save. An arg is quoted only when
// it must be (whitespace, a quote, or empty); inside quotes, `"` and `\` are
// backslash-escaped. Unquoted backslashes stay literal so plain Windows paths
// are untouched.
export function joinArgs(args: string[]): string {
  return args
    .map((a) => (a !== "" && !/[\s"]/.test(a) ? a : `"${a.replace(/(["\\])/g, "\\$1")}"`))
    .join(" ");
}

export function splitArgs(s: string): string[] {
  const args: string[] = [];
  let cur = "";
  let inQuotes = false;
  let hasToken = false;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (inQuotes) {
      if (c === "\\" && (s[i + 1] === '"' || s[i + 1] === "\\")) cur += s[++i];
      else if (c === '"') inQuotes = false;
      else cur += c;
      hasToken = true;
    } else if (c === '"') {
      inQuotes = true;
      hasToken = true;
    } else if (/\s/.test(c)) {
      if (hasToken) {
        args.push(cur);
        cur = "";
        hasToken = false;
      }
    } else {
      cur += c;
      hasToken = true;
    }
  }
  if (hasToken) args.push(cur);
  return args;
}

// Comma-separated extensions, normalized to the daemon's rules (lowercase, no
// surrounding space, no empties). The daemon re-validates and rejects anything
// still malformed (an internal space, a dot, a slash).
function splitExtensions(s: string): string[] {
  return s
    .split(",")
    .map((x) => x.trim().toLowerCase())
    .filter(Boolean);
}

// Light pre-validation for instant feedback. Mirrors only the daemon's cheapest
// reject reasons; the daemon's set_config remains the authoritative gate.
export function validate(e: EditableConfig): string | null {
  const control = parsePort(e.controlPort);
  const lan = parsePort(e.lanPort);
  if (!isPort(control)) return "Control port must be a number between 1 and 65535";
  if (!isPort(lan)) return "LAN port must be a number between 1 and 65535";
  if (e.upnpEnabled && control === lan)
    return "Control and LAN ports must differ when UPnP is enabled";
  if (splitExtensions(e.mediaExtensions).length === 0) return "Add at least one media extension";
  return null;
}

// Strict decimal parse: rejects forms Number() silently accepts (e.g. "0x1388"
// -> 5000, "1e4" -> 10000). NaN then fails isPort.
function parsePort(s: string): number {
  return /^\d+$/.test(s.trim()) ? Number(s.trim()) : NaN;
}

function isPort(n: number): boolean {
  return Number.isInteger(n) && n >= 1 && n <= 65535;
}
