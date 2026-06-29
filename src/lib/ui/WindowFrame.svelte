<script lang="ts">
  import Icon from "../icons/Icon.svelte";
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    subtitle?: string;
    icon?: string;
    onBack?: () => void;
    onClose?: () => void;
    actions?: Snippet;
    children: Snippet;
  }
  let { title, subtitle, icon, onBack, onClose, actions, children }: Props =
    $props();
</script>

<div class="flex flex-col h-full w-full">
  <header
    data-tauri-drag-region
    class="flex items-center gap-2.5 px-5 pt-4 pb-3 shrink-0"
  >
    {#if onBack}
      <button class="icon-btn no-drag" onclick={onBack} aria-label="Back">
        <Icon name="chevron-right" size={18} class="rotate-180" />
      </button>
    {/if}
    {#if icon}
      <span
        class="inline-grid place-items-center w-7 h-7 rounded-full text-white shrink-0"
        style="background: linear-gradient(140deg, var(--color-accent), var(--color-accent-2));"
      >
        <Icon name={icon} size={15} />
      </span>
    {/if}
    <div class="flex-1 leading-tight min-w-0">
      <div class="text-[13px] font-semibold text-ink truncate">{title}</div>
      {#if subtitle}
        <div class="text-[11px] text-ink-faint truncate">{subtitle}</div>
      {/if}
    </div>
    {#if actions}
      <div class="no-drag flex items-center gap-1">{@render actions()}</div>
    {/if}
    {#if onClose}
      <button class="icon-btn no-drag" onclick={onClose} aria-label="Close">
        <Icon name="x" size={17} />
      </button>
    {/if}
  </header>
  <div class="flex-1 min-h-0 overflow-y-auto">
    {@render children()}
  </div>
</div>
