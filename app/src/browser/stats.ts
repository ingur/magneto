// Display formatters, built from structured daemon fields, never parsed
// from strings. Compact, terminal-flavored output.

const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const i = Math.min(UNITS.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  const value = bytes / 1024 ** i;
  const rounded = value >= 100 || i === 0 ? Math.round(value).toString() : value.toFixed(1);
  return `${rounded} ${UNITS[i]}`;
}

export function formatSpeed(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

export function formatPercent(progress: number): string {
  return `${Math.round(progress * 100)}%`;
}

// ETA from remaining bytes and current speed; null when not computable
// (no speed, or nothing left to download).
export function formatEta(remainingBytes: number, bytesPerSec: number): string | null {
  if (bytesPerSec <= 0 || remainingBytes <= 0) return null;
  const secs = Math.round(remainingBytes / bytesPerSec);
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.round(secs / 60)}m`;
  if (secs < 86400) return `${Math.round(secs / 3600)}h`;
  return `${Math.round(secs / 86400)}d`;
}
