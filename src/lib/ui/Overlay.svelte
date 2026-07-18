<script lang="ts">
  // The one overlay shell for the whole app: consistent styling from the shared
  // tokens + correct window sizing so nothing clips in the short auto-fit hub.
  //
  //  - "dialog"  (task editor): a FULL-BLEED panel that fills the window edge to
  //    edge; the window is sized exactly to the panel's content, so there's no
  //    stray background showing around it.
  //  - "popover" (reschedule calendar, switch-task, break settings): a floating
  //    centered card over a transparent click-catch; the window only GROWS to fit
  //    it (never shrinks), so the list stays visible behind it.
  //
  // Resize is debounced: content height ticks many times during a step/dropdown
  // transition, and firing set_size + re-center on every tick makes the window
  // crawl. Coalescing to the settled height gives one smooth resize.
  import { onMount } from "svelte";
  import type { Snippet } from "svelte";
  import { store } from "../store.svelte";
  import { api } from "../api";

  interface Props {
    onClose: () => void;
    variant?: "dialog" | "popover";
    maxWidth?: number;
    /** Card inner padding in px. Dialogs manage their own (0); popovers get a default. */
    pad?: number;
    children: Snippet;
  }
  let {
    onClose,
    variant = "popover",
    maxWidth = variant === "dialog" ? 468 : 288,
    pad = variant === "dialog" ? 0 : 8,
    children,
  }: Props = $props();

  const isDialog = variant === "dialog";
  let cardH = $state(0);
  let fitTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const h = cardH;
    if (h <= 0) return;
    clearTimeout(fitTimer);
    fitTimer = setTimeout(() => {
      const needed = h + (isDialog ? 0 : 24); // popover: 12px card offset top+bottom
      const target = isDialog ? needed : Math.max(window.innerHeight, needed);
      api.fitWindow(Math.round(target)).catch(() => {});
    }, 55);
  });

  onMount(() => {
    store.overlayCount += 1;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      clearTimeout(fitTimer);
      store.overlayCount = Math.max(0, store.overlayCount - 1);
      store.fitTick += 1; // let the hub re-fit to the list once we're gone
    };
  });
</script>

{#if !isDialog}
  <button class="ov-catch no-drag" aria-label="Close" onclick={onClose}></button>
{/if}
<div
  class="ov-card no-drag"
  class:dialog={isDialog}
  style="max-width:{maxWidth}px; padding:{pad}px;"
  bind:clientHeight={cardH}
>
  {@render children()}
</div>

<style>
  .ov-catch {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: transparent;
    cursor: default;
  }
  .ov-card {
    position: fixed;
    z-index: 50;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    width: calc(100% - 24px);
    /* Fixed (not 100vh): a viewport-relative cap would clamp clientHeight to the
       current window, so content that grows (e.g. the category dropdown opening)
       could never measure taller than the window and the window never grew. 720
       stays under the backend's 760px fit_window clamp. */
    max-height: 720px;
    display: flex;
    flex-direction: column;
    background: linear-gradient(170deg, var(--glass-top), var(--glass-bot));
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: 0 16px 40px -12px rgba(0, 0, 0, 0.4);
    overflow: hidden;
    animation: fade 0.16s ease both;
  }
  /* Full-bleed dialog: fill the window edge to edge (top-anchored, full width,
     content height). The window is sized to it, so it reads as the whole window. */
  .ov-card.dialog {
    left: 0;
    top: 0;
    transform: none;
    width: 100%;
    max-width: none !important;
    height: auto;
    border: none;
    border-radius: var(--radius-card);
    box-shadow: none;
  }
</style>
