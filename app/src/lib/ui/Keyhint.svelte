<script lang="ts">
  interface Props {
    keys: string;
    label: string;
  }

  let { keys, label }: Props = $props();
</script>

<!-- Keybinding hint: highlighted key on the left (primary text), dim label
     on the right (muted). Used in StatusBar and anywhere we surface shortcuts. -->
<span class="text-muted whitespace-nowrap text-xs">
  <span class="text-fg">{keys}</span>
  {label}
</span>
