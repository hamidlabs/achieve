<script lang="ts">
  // A small, on-brand custom select (the native <select> replacement). Expands
  // in-flow like the category picker so it never clips inside a scrolling sheet.
  import { slide } from "svelte/transition";
  import Icon from "../icons/Icon.svelte";

  interface Opt {
    value: string;
    label: string;
    hint?: string;
  }
  interface Props {
    value: string;
    options: Opt[];
    onChange: (v: string) => void;
    placeholder?: string;
    ariaLabel?: string;
    compact?: boolean;
  }
  let { value, options, onChange, placeholder = "Select…", ariaLabel, compact = false }: Props = $props();

  let open = $state(false);
  const selected = $derived(options.find((o) => o.value === value) ?? null);

  function pick(v: string) {
    onChange(v);
    open = false;
  }

  // Close when clicking outside this control.
  function outside(node: HTMLElement) {
    const onDoc = (e: MouseEvent) => {
      if (open && !node.contains(e.target as Node)) open = false;
    };
    document.addEventListener("mousedown", onDoc, true);
    return { destroy: () => document.removeEventListener("mousedown", onDoc, true) };
  }
</script>

<div class="sel" class:compact use:outside>
  <button
    class="sel-trigger no-drag"
    class:on={open}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={ariaLabel}
    onclick={() => (open = !open)}
  >
    <span class="sel-value" class:placeholder={!selected}>{selected?.label ?? placeholder}</span>
    <Icon name="chevron-down" size={15} class={open ? "sel-chev rotate-180" : "sel-chev"} />
  </button>

  {#if open}
    <div class="sel-panel" role="listbox" transition:slide={{ duration: 150 }}>
      {#each options as o (o.value)}
        <button class="sel-opt" class:on={o.value === value} role="option" aria-selected={o.value === value} onclick={() => pick(o.value)}>
          <span class="sel-opt-main">
            <span class="sel-opt-label">{o.label}</span>
            {#if o.hint}<span class="sel-opt-hint">{o.hint}</span>{/if}
          </span>
          {#if o.value === value}<Icon name="check" size={14} class="sel-tick" />{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .sel {
    position: relative;
    width: 100%;
  }
  .sel-trigger {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 9px 12px;
    border-radius: var(--radius-md);
    border: 0.5px solid var(--line-strong);
    background: #fff;
    font-size: 13px;
    color: var(--color-ink);
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .compact .sel-trigger {
    padding: 6px 11px;
    font-size: 12px;
    border-radius: var(--radius-pill);
  }
  .sel-trigger:hover {
    border-color: color-mix(in oklab, var(--color-accent) 40%, var(--line-strong));
  }
  .sel-trigger.on {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px color-mix(in oklab, var(--color-accent) 18%, transparent);
  }
  .sel-value {
    flex: 1;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sel-value.placeholder {
    color: var(--color-ink-faint);
  }
  .sel :global(.sel-chev) {
    color: var(--color-ink-ghost);
    transition: transform 0.16s ease;
    flex-shrink: 0;
  }
  /* In-flow (pushes content, never clips inside a scrolling sheet). */
  .sel-panel {
    margin-top: 5px;
    max-height: 232px;
    overflow-y: auto;
    padding: 5px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    background: var(--card-2);
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 6px 18px -12px rgba(38, 33, 90, 0.3);
  }
  .sel-opt {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    font-size: 12.5px;
    color: var(--color-ink);
    text-align: left;
    transition: background 0.1s ease;
  }
  .sel-opt:hover {
    background: var(--violet-50);
  }
  .sel-opt.on {
    background: var(--violet-50);
    color: var(--color-accent-strong);
    font-weight: 600;
  }
  .sel-opt-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .sel-opt-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sel-opt-hint {
    font-size: 10.5px;
    color: var(--color-ink-faint);
    font-weight: 400;
  }
  .sel :global(.sel-tick) {
    color: var(--color-accent);
    flex-shrink: 0;
  }
</style>
