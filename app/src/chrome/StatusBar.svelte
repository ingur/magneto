<script lang="ts">
  import StatusBar from "@/lib/status/StatusBar.svelte";
  import KbHintsWidget from "@/status-widgets/KbHintsWidget.svelte";
  import MarkedInfoWidget from "@/status-widgets/MarkedInfoWidget.svelte";
  import TorrentStatsWidget from "@/status-widgets/TorrentStatsWidget.svelte";
  import FilterWidget from "@/status-widgets/FilterWidget.svelte";

  // Magneto's status bar layout. The generic shell + fit algorithm lives in
  // lib/status; this file just plugs in our widgets. Adding a widget = drop
  // it into a snippet; the fit algorithm picks it up by priority.
</script>

<StatusBar>
  {#snippet left()}
    <MarkedInfoWidget />
    <TorrentStatsWidget />
    <FilterWidget />
  {/snippet}
  {#snippet right()}
    <KbHintsWidget />
  {/snippet}
</StatusBar>
