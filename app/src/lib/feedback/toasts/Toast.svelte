<script lang="ts">
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import Info from "@lucide/svelte/icons/info";
  import type { Toast, ToastKind } from "./toasts.svelte";

  interface Props {
    toast: Toast;
    onDismiss: () => void;
  }

  let { toast, onDismiss }: Props = $props();

  // Map kind → icon component + accent class. Tailwind needs the full
  // class strings to be discoverable, so each kind lists its classes
  // verbatim rather than interpolating.
  const icons: Record<ToastKind, typeof Info> = {
    info: Info,
    success: CircleCheck,
    warn: CircleAlert,
    error: CircleX,
  };

  const accentText: Record<ToastKind, string> = {
    info: "text-info",
    success: "text-success",
    warn: "text-warning",
    error: "text-danger",
  };

  const Icon = $derived(icons[toast.kind]);
</script>

<!-- Single toast: neutral opaque popover surface, with kind expressed by
     the icon/action color only. This keeps notifications in the same
     visual family as menus/dropdowns instead of introducing glassy tinted
     cards over the torrent list. -->
<div class="flex items-center gap-2 rounded bg-panel p-2 text-xs text-fg shadow ring-1 ring-raised">
  <Icon size={14} class={["shrink-0", accentText[toast.kind]]} />
  <span class="min-w-0 max-w-64 break-words">{toast.message}</span>
  {#if toast.action}
    <button
      type="button"
      tabindex={-1}
      onclick={() => {
        toast.action?.onClick();
        onDismiss();
      }}
      class={["ml-1 shrink-0 hover:underline", accentText[toast.kind]]}
    >
      {toast.action.label}
    </button>
  {/if}
</div>
