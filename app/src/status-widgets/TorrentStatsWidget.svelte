<script lang="ts">
  import { daemon } from "@/daemon/client.svelte";
  import { nav } from "@/torrents/nav.svelte";
  import StatusItem from "@/lib/status/StatusItem.svelte";

  // Reflects the current page: root counts torrents, inside a torrent counts
  // the projected folders/files. Click the count to mark all visible rows.
  const rows = $derived(nav.currentRows);
  const isRoot = $derived(nav.pathIds.length === 0);
  const total = $derived(rows.length);
  const noun = $derived.by(() => {
    if (isRoot) return "torrents";
    if (rows.every((r) => r.kind === "folder")) return "folders";
    if (rows.every((r) => r.kind === "file")) return "files";
    return "items";
  });
  // Activity counts are live claims: hidden while the socket is down rather
  // than frozen at their last reading. Stalled is downloading that receives
  // nothing, so it counts.
  const live = $derived(daemon.status === "connected");
  const downloading = $derived(
    rows.filter((r) => r.state === "downloading" || r.state === "stalled").length,
  );
  // Seeding is a torrent-row fact (there is no per-file seeding on the wire).
  const seeding = $derived(rows.filter((r) => r.kind === "torrent" && r.isSeeding).length);

  function selectAllVisible() {
    nav.markAll(rows.map((r) => r.id));
  }
</script>

{#if !nav.filter.active}
  <StatusItem priority={50}>
    <button
      type="button"
      tabindex={-1}
      onclick={selectAllVisible}
      class="hover:text-fg cursor-default"
    >
      {total}
      {noun}
    </button>
  </StatusItem>
  {#if downloading > 0}
    <StatusItem priority={30} class="text-info">↓ {downloading} downloading</StatusItem>
  {/if}
  {#if seeding > 0}
    <StatusItem priority={30} class="text-success">↑ {seeding} seeding</StatusItem>
  {/if}
{/if}
