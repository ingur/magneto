<script lang="ts">
  import { kb, type LayerHandle } from "@/lib/kb/kb.svelte";
  import { cheatsheetFor } from "@/lib/kb/cheatsheet";
  import KbLayer from "@/lib/kb/KbLayer.svelte";
  import { useScrollFollowCursor } from "@/lib/kb/use-scroll-follow-cursor.svelte";
  import Overlay from "@/lib/surface/Overlay.svelte";
  import OverlayTitle from "@/lib/surface/OverlayTitle.svelte";
  import Button from "@/lib/ui/Button.svelte";
  import ButtonRow from "@/lib/ui/ButtonRow.svelte";
  import ExternalLink from "@lucide/svelte/icons/external-link";

  import { showAbout, openRepository } from "./about";
  import { helpOpen } from "./open.svelte";
  import { helpBindings, type HelpButtonId } from "./bindings";
  import HelpSection from "./HelpSection.svelte";
  import HelpEntry from "./HelpEntry.svelte";

  // The cheatsheet renders the main app controls: the Browser layer's binding
  // metadata, read directly (no duplicated keymap). A stable v1 overview page.
  const sections = cheatsheetFor(kb.layer("browser"));

  let handle = $state<LayerHandle | undefined>();
  let lastButton = $state<HelpButtonId>("about");

  useScrollFollowCursor(() => handle);

  const bindings = helpBindings({
    getLastButton: () => lastButton,
    setLastButton: (id) => (lastButton = id),
    close: () => (helpOpen.value = false),
  });

  function runAbout() {
    lastButton = "about";
    void showAbout();
  }
  function runRepository() {
    lastButton = "repo";
    void openRepository();
  }
</script>

<KbLayer name="help" {bindings} bind:handle>
  <Overlay onClose={() => (helpOpen.value = false)}>
    {#snippet title()}
      <OverlayTitle>Help</OverlayTitle>
    {/snippet}

    <div class="flex flex-col gap-6">
      {#each sections as section (section.category)}
        <HelpSection title={section.category}>
          {#each section.entries as entry (entry.description)}
            <HelpEntry
              description={entry.description}
              keys={entry.keys}
              kbItem={{ id: `${section.category}:${entry.description}`, group: "help" }}
            />
          {/each}
        </HelpSection>
      {/each}
    </div>

    {#snippet actions()}
      <ButtonRow>
        <Button kbItem={{ id: "about", group: "buttons", activate: runAbout }} onclick={runAbout}>
          About
        </Button>
        <Button
          tint="accent"
          kbItem={{ id: "repo", group: "buttons", activate: runRepository }}
          onclick={runRepository}
        >
          <ExternalLink size={12} strokeWidth={2} />
          Repository
        </Button>
      </ButtonRow>
    {/snippet}
  </Overlay>
</KbLayer>
