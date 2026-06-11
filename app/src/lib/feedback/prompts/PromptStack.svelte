<script lang="ts">
  import { kb, type Binding, type LayerHandle } from "@/lib/kb/kb.svelte";
  import { verticalNavWithButtons } from "@/lib/kb/vertical-nav";
  import { prompts } from "./prompts.svelte";
  import Button from "@/lib/ui/Button.svelte";
  import ButtonRow from "@/lib/ui/ButtonRow.svelte";
  import KbLayer from "@/lib/kb/KbLayer.svelte";
  import Dropdown from "@/lib/ui/controls/Dropdown.svelte";
  import TextInput from "@/lib/ui/controls/TextInput.svelte";
  import Dialog from "@/lib/surface/Dialog.svelte";
  import DialogDescription from "@/lib/surface/DialogDescription.svelte";
  import DialogTitle from "@/lib/surface/DialogTitle.svelte";

  // Renderer reads `prompts.current` and shows the head of the queue. When
  // resolved, the head is shifted off and the next one (if any) takes
  // over. Each prompt mounts its own KbLayer named "prompt" (keyed on
  // current.id) so cursor/state is per-prompt and never leaks between
  // queued prompts.

  const current = $derived(prompts.current);

  // Per-prompt mutable state; resets when `current.id` changes (the
  // KbLayer below is keyed on it, which forces a fresh mount).
  let textValue = $state("");
  let choiceValue = $state("");
  let dropdownOpen = $state(false);
  let lastButton = $state<"cancel" | "confirm">("cancel");
  let inputEl = $state<HTMLInputElement>();
  let handle = $state<LayerHandle | undefined>();

  // Initialize working values whenever a new prompt becomes current.
  $effect(() => {
    if (!current) return;
    if (current.opts.type === "text") {
      textValue = current.opts.default ?? "";
    } else if (current.opts.type === "choice") {
      choiceValue = current.opts.default ?? current.opts.choices[0] ?? "";
    }
  });

  // Reset lastButton each time a new prompt opens.
  $effect(() => {
    if (current) lastButton = "cancel";
  });

  function cancel() {
    prompts.resolve(null);
  }
  function confirm() {
    if (!current) return;
    if (current.opts.type === "confirm") prompts.resolve(true);
    else if (current.opts.type === "text") prompts.resolve(textValue);
    else if (current.opts.type === "choice") prompts.resolve(choiceValue);
  }

  // Initial cursor: confirm dialogs land on Cancel for safety; prompts
  // with an input/dropdown land on the input so j-into-buttons is a clear
  // mode switch. Info with an action lands on the button (only kb item);
  // info without an action has no items at all, cursor stays null.
  const initialCursor = $derived.by(() => {
    if (!current) return null;
    if (current.opts.type === "text") return "input";
    if (current.opts.type === "choice") return "choice";
    if (current.opts.type === "info") return current.opts.action ? "action" : null;
    return "cancel";
  });

  // Logic shared with Settings; here h/l carry the 'navigate' label
  // because j/k are unlabeled on prompts (the input row is single, so
  // there's no list to navigate; the meaningful nav is between buttons).
  const nav = verticalNavWithButtons({
    rowGroup: "input",
    buttonGroup: "buttons",
    leftButtonId: "cancel",
    rightButtonId: "confirm",
    getLastButton: () => lastButton,
    setLastButton: (id) => (lastButton = id as "cancel" | "confirm"),
  });

  // Info bindings split:
  //   * info-with-action: enter/space activate the cursored button
  //     (calls action.onClick + dismisses); esc dismisses without firing
  //     the action. Esc is the safe out, never invokes outbound work.
  //   * info-only:        enter/space/escape all dismiss; nothing to
  //     navigate, so any "done reading" key closes.
  //
  // Only escape opts into clickable hints: pressing the StatusBar's
  // "esc cancel/close" should dismiss; "enter confirm" specifically does
  // NOT, because clicking small statusbar text shouldn't be how a user
  // confirms a destructive action.
  const bindings = $derived.by((): Record<string, Binding> => {
    const c = current?.opts;
    if (c?.type === "info") {
      if (c.action) {
        return {
          enter: { label: "confirm", priority: 80, run: () => kb.activate() },
          space: { run: () => kb.activate() },
          escape: { label: "close", priority: 80, clickable: true, run: cancel },
        };
      }
      return {
        enter: { label: "close", priority: 80, clickable: true, run: cancel },
        space: { run: cancel },
        escape: { label: "close", priority: 80, clickable: true, run: cancel },
      };
    }
    return {
      j: { run: nav.j },
      k: { run: nav.k },
      h: { label: "navigate", priority: 50, run: nav.h },
      l: { label: "navigate", priority: 50, run: nav.l },
      // Enter and Space activate the cursored item, same primitive as
      // every other layer. For destructive prompts the cursor defaults to
      // Cancel, so a stray Enter cancels (safe). h/l moves to Confirm
      // before Enter to commit. Esc is the always-cancel hatch.
      enter: { label: "confirm", priority: 80, run: () => kb.activate() },
      space: { run: () => kb.activate() },
      escape: { label: "cancel", priority: 80, clickable: true, run: cancel },
    };
  });

  // Info-action click: run the user's onClick THEN resolve. Resolve
  // (rather than just leaving the prompt up) so the Promise consumers
  // get unblocked; info-with-action still resolves to null (no input
  // to capture; the action's effect is its own state change).
  function activateInfoAction() {
    if (current?.opts.type === "info" && current.opts.action) {
      current.opts.action.onClick();
    }
    cancel();
  }
</script>

{#if current}
  {@const c = current.opts}
  {#key current.id}
    <KbLayer name="prompt" {bindings} cursorId={initialCursor} bind:handle>
      <Dialog onClose={cancel}>
        <DialogTitle>{c.title}</DialogTitle>
        {#if c.description}
          <DialogDescription>{c.description}</DialogDescription>
        {/if}

        {#if c.type === "text"}
          <TextInput
            bind:value={textValue}
            bind:el={inputEl}
            placeholder={c.placeholder}
            size="lg"
            kbItem={{ id: "input", group: "input" }}
          />
        {:else if c.type === "choice"}
          <Dropdown
            bind:value={choiceValue}
            bind:open={dropdownOpen}
            options={c.choices}
            size="lg"
            kbItem={{
              id: "choice",
              group: "input",
              activate: () => (dropdownOpen = true),
            }}
          />
        {/if}

        {#if c.type === "info" && c.action}
          {@const Icon = c.action.icon}
          <ButtonRow>
            <Button
              tint={c.action.tint ?? "accent"}
              kbItem={{ id: "action", group: "buttons", activate: activateInfoAction }}
              onclick={activateInfoAction}
            >
              {#if Icon}
                <Icon size={12} strokeWidth={2} />
              {/if}
              {c.action.label}
            </Button>
          </ButtonRow>
        {:else if c.type !== "info"}
          <ButtonRow>
            <Button
              kbItem={{ id: "cancel", group: "buttons", activate: cancel }}
              onclick={() => {
                lastButton = "cancel";
                cancel();
              }}
            >
              {c.cancelLabel ?? "Cancel"}
            </Button>
            <Button
              tint={c.type === "confirm" ? (c.tint ?? "accent") : "accent"}
              kbItem={{ id: "confirm", group: "buttons", activate: confirm }}
              onclick={() => {
                lastButton = "confirm";
                confirm();
              }}
            >
              {c.confirmLabel ?? "OK"}
            </Button>
          </ButtonRow>
        {/if}
      </Dialog>
    </KbLayer>
  {/key}
{/if}
