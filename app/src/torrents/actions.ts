// Torrent actions: the daemon seam plus the orchestration above it.
//
// Two layers, one home:
//   * Low-level senders map row IDs → Targets, gate file/folder ops on torrent
//     readiness, send the command, and refetch detail for the no-event
//     mutations. Private; the run* commands are the surface.
//   * run* commands own the user-facing orchestration (confirm prompt, toast
//     copy, clear-selection-after-bulk). Mouse (Row) and keyboard (bindings)
//     both build an ActionTargets and call the same run*: the targeting
//     differs, the orchestration does not, so the two can never drift.

import { daemon } from "@/daemon/client.svelte";
import type { AffectedResp, ResolveLocalPathResp, Target, TorrentSummary } from "@/daemon/protocol";
import { revealPath } from "@/daemon/tauri";
import { kb } from "@/lib/kb/kb.svelte";
import { prompt } from "@/lib/feedback/prompts/prompts.svelte";
import { toast } from "@/lib/feedback/toasts/toasts.svelte";

import { data, type FilePatch } from "./data.svelte";
import { idToInfoHash, idToTarget } from "./ids";
import { nav } from "./nav.svelte";
import { targetFiles, type Row } from "./projection";

// What an action operates on: the resolved target ids, the page rows (for
// per-target state lookups), the leader row (sets toggle direction / kind
// branch), and a human subject for the toast/prompt copy.
export interface ActionTargets {
  ids: string[];
  rows: Row[];
  leader: Row | undefined;
  subject: string;
  // True when the action targets the marked selection. Destructive actions
  // clear it afterward; acting on a non-selected row must NOT wipe an
  // unrelated selection.
  bulk: boolean;
}

// Keyboard targeting: the marked selection if any, else the cursored row.
export function resolveTargets(): ActionTargets {
  const rows = nav.currentRows;
  const cursorRow = rows.find((r) => r.id === kb.cursor());
  const bulk = nav.selection.size > 0;
  const ids = bulk ? [...nav.selection] : cursorRow ? [cursorRow.id] : [];
  return { ids, rows, leader: cursorRow, subject: subjectFor(ids, cursorRow), bulk };
}

// Row targeting (mouse): the selection if this row is part of it, else just
// this row. The clicked row is always the leader.
export function rowTargets(row: Row, marked: boolean): ActionTargets {
  const bulk = marked && nav.selection.size > 0;
  const ids = bulk ? [...nav.selection] : [row.id];
  return { ids, rows: nav.currentRows, leader: row, subject: subjectFor(ids, row), bulk };
}

function subjectFor(ids: string[], leader: Row | undefined): string {
  return ids.length > 1 ? "selection" : (leader?.kind ?? "item");
}

// --- orchestration (toast / confirm / clear-after-bulk live here, once) ---

export async function runPlay(t: ActionTargets): Promise<void> {
  if (t.ids.length === 0) return;
  if (await play(t.ids)) toast.info(`Playing ${t.subject}`);
}

export async function runTogglePersist(t: ActionTargets): Promise<void> {
  if (!t.leader) return;
  const next = t.leader.persisted !== "all"; // leader sets the direction
  if (await setPersist(t.ids, next)) {
    toast.info(`${next ? "Saved" : "Unsaved"} ${t.subject}`);
  }
}

export async function runToggleShare(t: ActionTargets): Promise<void> {
  if (!t.leader) return;
  const next = t.leader.shared !== "all";
  if (await setShared(t.ids, next)) {
    toast.info(`${next ? "Shared" : "Unshared"} ${t.subject}`);
  }
}

// A complete torrent with undownloaded media: resume means "download the rest"
// (the daemon expands a fully-complete selection to all media).
export function downloadRemaining(r: Row): boolean {
  return (
    r.kind === "torrent" && r.state === "complete" && (r.completeCount ?? 0) < (r.fileCount ?? 0)
  );
}

// The toggle's direction is a function of the targets, not the cursor: if
// anything targeted is active (downloading, or queued, already wanted),
// the toggle stops it all; otherwise it starts everything stopped. One press
// always converges the targets to one coherent state.
export function pauseDirection(t: ActionTargets): boolean {
  return t.ids.some((id) => {
    const s = t.rows.find((row) => row.id === id)?.state;
    return s === "downloading" || s === "queued";
  });
}

export async function runToggleDownload(t: ActionTargets): Promise<void> {
  if (!t.leader) return;
  const isPause = pauseDirection(t);
  // Only flip rows in the matching direction; an errored row resumes too (the
  // daemon retries it with a fresh re-check).
  const valid = t.ids.filter((id) => {
    const r = t.rows.find((row) => row.id === id);
    if (!r) return false;
    return isPause
      ? r.state === "downloading" || r.state === "queued"
      : r.state === "paused" || r.state === "idle" || r.state === "error" || downloadRemaining(r);
  });
  if (valid.length === 0) return;
  if (isPause ? await pause(valid) : await resume(valid)) {
    // An idle or download-remaining target was never (fully) wanted, so it
    // "starts", not "resumes"; an errored one retries.
    const verb = isPause
      ? "Paused"
      : t.leader.state === "idle" || t.leader.state === "complete"
        ? "Started"
        : t.leader.state === "error"
          ? "Retrying"
          : "Resumed";
    toast.info(`${verb} ${t.subject}`);
  }
}

// Seeding control for complete torrents: the download toggle has no direction
// on a complete row, so the menu drives this. Pause is an engine-level stop;
// resume restarts the seed without downloading anything new.
export async function runToggleSeeding(t: ActionTargets): Promise<void> {
  if (!t.leader) return;
  const isPause = t.leader.isSeeding === true;
  const valid = t.ids.filter((id) => {
    const r = t.rows.find((row) => row.id === id);
    if (!r || r.kind !== "torrent" || r.state !== "complete") return false;
    return isPause ? r.isSeeding === true : r.isPaused === true;
  });
  if (valid.length === 0) return;
  if (isPause ? await pause(valid) : await resume(valid)) {
    toast.info(`${isPause ? "Paused" : "Resumed"} seeding`);
  }
}

export async function runDelete(t: ActionTargets): Promise<void> {
  if (t.ids.length === 0 || !t.leader) return;
  const ok = await prompt({
    type: "confirm",
    title: `Delete ${t.subject}?`,
    description: "Downloaded files are deleted too.\nThis action cannot be undone.",
    confirmLabel: "Delete",
    tint: "danger",
  });
  if (!ok) return;
  if (t.leader.kind === "torrent") {
    const { removed, failed, error } = await removeTorrents(t.ids);
    if (removed > 0) {
      clearAfterBulk(t);
      toast.success(failed > 0 ? `Deleted ${removed}, ${failed} failed` : `Deleted ${t.subject}`);
    } else if (failed > 0) {
      toast.error(error ?? `Couldn't delete ${t.subject}`);
    }
  } else if (await dropTargets(t.ids)) {
    clearAfterBulk(t);
    toast.success(`Deleted ${t.subject}`);
  }
}

// Reveal a row's downloaded data in the OS file manager. Single-item (the
// cursor / clicked row); revealing a selection would spawn N windows. `exists`
// is best-effort: false means "not on disk yet", not "deleted".
export async function runReveal(row: Row): Promise<void> {
  const target = idToTarget(row.id);
  if (!target) return;
  try {
    const resp = await daemon.request<ResolveLocalPathResp>("resolve_local_path", { target });
    if (!resp.exists) toast.warn("Not downloaded yet");
    else await revealPath(resp.path, resp.kind);
  } catch (e) {
    toast.error(message(e));
  }
}

// Copy a shareable magnet link per targeted torrent (deduped to info_hashes).
// A magnet-added torrent keeps its original (trackers and all); others get a
// minimal magnet from the info hash + name.
export function buildMagnet(summary: TorrentSummary): string {
  if (summary.source_kind === "magnet" && summary.source) return summary.source;
  const dn = summary.name ? `&dn=${encodeURIComponent(summary.name)}` : "";
  return `magnet:?xt=urn:btih:${summary.info_hash}${dn}`;
}

export async function runCopyMagnet(t: ActionTargets): Promise<void> {
  const magnets = [...new Set(t.ids.map(idToInfoHash).filter((h): h is string => h !== null))]
    .map((h) => daemon.torrents[h])
    .filter((s): s is TorrentSummary => s !== undefined)
    .map(buildMagnet);
  if (magnets.length === 0) return;
  try {
    await navigator.clipboard.writeText(magnets.join("\n"));
    toast.success(
      magnets.length === 1 ? "Copied magnet link" : `Copied ${magnets.length} magnet links`,
    );
  } catch {
    toast.error("Couldn't copy to clipboard");
  }
}

// Only destructive actions clear the selection: deleted rows leave the list,
// so their marks would be stale. Everything else keeps the marks so a
// selection can be acted on repeatedly (start, then persist, then share).
function clearAfterBulk(t: ActionTargets): void {
  if (t.bulk) nav.clearSelection();
}

// --- low-level daemon senders (private) ---

function toTargets(ids: string[]): Target[] {
  return ids.map(idToTarget).filter((t): t is Target => t !== null);
}

// Drop file/folder targets whose torrent isn't ready (loaded + not
// reinitializing): the daemon would reject or misapply them, and it pushes
// no event for the mutations. Torrent targets pass through (root-level ops
// are valid mid-initialization; the daemon errors clearly if there's nothing
// to act on).
function gateReady(ids: string[]): string[] {
  return ids.filter((id) => {
    const t = idToTarget(id);
    if (!t) return false;
    return t.kind === "torrent" || data.ready(t.info_hash);
  });
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

// Refetch detail for every open torrent touched by these ids; the daemon
// pushes no event for persist/share/pause/resume.
async function refetchAffected(ids: string[]): Promise<void> {
  const hashes = new Set<string>();
  for (const id of ids) {
    const h = idToInfoHash(id);
    if (h && data.detail(h)) hashes.add(h);
  }
  await Promise.all([...hashes].map((h) => data.load(h).catch(() => {})));
}

async function sendTargets(
  command: string,
  ids: string[],
  extra: Record<string, unknown> = {},
): Promise<boolean> {
  const targets = toTargets(gateReady(ids));
  if (targets.length === 0) {
    // All file/folder targets gated as not-ready: say why nothing happened
    // (loading vs broken) instead of a silent no-op.
    if (ids.length > 0) toast.warn("Torrent still loading");
    return false;
  }
  try {
    await daemon.request(command, { targets, ...extra });
    return true;
  } catch (e) {
    toast.error(message(e));
    return false;
  }
}

async function play(ids: string[]): Promise<boolean> {
  return sendTargets("play", ids);
}

// pause/resume/drop report how many targets they actually changed; treat
// affected:0 as "nothing happened" (no success toast, no refetch), e.g.
// pausing a file the daemon can't pause, or resuming an already-selected one.
async function sendAffected(
  command: string,
  ids: string[],
  extra: Record<string, unknown> = {},
): Promise<boolean> {
  const targets = toTargets(gateReady(ids));
  if (targets.length === 0) {
    if (ids.length > 0) toast.warn("Torrent still loading");
    return false;
  }
  try {
    const resp = await daemon.request<AffectedResp>(command, { targets, ...extra });
    if (resp.affected > 0) await refetchAffected(ids);
    return resp.affected > 0;
  } catch (e) {
    toast.error(message(e));
    return false;
  }
}

async function pause(ids: string[]): Promise<boolean> {
  return sendAffected("pause", ids);
}

async function resume(ids: string[]): Promise<boolean> {
  return sendAffected("resume", ids);
}

async function dropTargets(ids: string[]): Promise<boolean> {
  return sendAffected("drop_targets", ids);
}

// Apply a predicted local change immediately, send, and undo it only on the rare
// failure. The optimistic state matches what the command does and the daemon
// emits no event for these, so there's nothing to reconcile on success; the
// next stats tick / snapshot stays authoritative.
async function optimistic(apply: () => () => void, send: () => Promise<boolean>): Promise<boolean> {
  const revert = apply();
  const ok = await send();
  if (!ok) revert();
  return ok;
}

// Reflect a per-file boolean toggle immediately: flip the open detail's file
// flags (the file/folder rows inside a torrent), and for a whole-torrent target
// also set its summary count so the root torrent row updates without waiting for
// the next stats tick. Returns one combined revert.
function applyToggle(
  ids: string[],
  filePatch: FilePatch,
  summaryPatch: (s: TorrentSummary) => Partial<TorrentSummary>,
): () => void {
  const reverts: Array<() => void> = [];
  for (const id of ids) {
    const target = idToTarget(id);
    if (!target) continue;
    const detail = data.detail(target.info_hash);
    if (detail) {
      const indices = new Set(targetFiles(detail, target));
      reverts.push(data.patchFiles(target.info_hash, indices, filePatch));
    }
    if (target.kind === "torrent") {
      const summary = daemon.torrents[target.info_hash];
      if (summary) reverts.push(daemon.patchSummary(target.info_hash, summaryPatch(summary)));
    }
  }
  return () => reverts.forEach((r) => r());
}

function setPersist(ids: string[], persisted: boolean): Promise<boolean> {
  return optimistic(
    () =>
      applyToggle(ids, { persisted }, (s) => ({ persisted_count: persisted ? s.file_count : 0 })),
    () => sendTargets("set_persist", ids, { persisted }),
  );
}

function setShared(ids: string[], shared: boolean): Promise<boolean> {
  return optimistic(
    () => applyToggle(ids, { shared }, (s) => ({ shared_count: shared ? s.file_count : 0 })),
    () => sendTargets("set_shared", ids, { shared }),
  );
}

// Whole-torrent deletion (root rows): removes the torrent and its files from
// disk. allSettled so a partial bulk delete is reported, not discarded. Returns
// the tally; the caller (runDelete) owns the single toast for the gesture.
async function removeTorrents(
  ids: string[],
): Promise<{ removed: number; failed: number; error: string | null }> {
  const hashes = [...new Set(ids.map(idToInfoHash).filter((h): h is string => h !== null))];
  if (hashes.length === 0) return { removed: 0, failed: 0, error: null };
  const results = await Promise.allSettled(
    hashes.map((h) => daemon.request("remove_torrent", { info_hash: h, delete_files: true })),
  );
  const rejected = results.filter((r) => r.status === "rejected") as PromiseRejectedResult[];
  return {
    removed: hashes.length - rejected.length,
    failed: rejected.length,
    error: rejected.length > 0 ? message(rejected[0].reason) : null,
  };
}
