<script lang="ts">
  // The one overlay shell for the whole app, sized to the fixed window frame.
  //
  //  - "sheet"   (task editor): a centered modal card that fills most of the
  //    frame and scrolls internally.
  //  - "popover" (reschedule calendar, switch-task, break settings): a smaller
  //    centered card over a dimmed scrim.
  //
  // Both are centered with a flex WRAPPER (not a transform on the card), so the
  // card is free to animate scale/opacity on entry without losing its position.
  import { onMount } from "svelte";
  import type { Snippet } from "svelte";
  import { store } from "../store.svelte";

  interface Props {
    onClose: () => void;
    /** "sheet" = big editor card; "popover" = compact card. ("dialog" kept as an alias for sheet.) */
    variant?: "sheet" | "popover" | "dialog";
    maxWidth?: number;
    /** Card inner padding in px. Sheets manage their own (0); popovers get a default. */
    pad?: number;
    children: Snippet;
  }
  let {
    onClose,
    variant = "popover",
    maxWidth,
    pad,
    children,
  }: Props = $props();

  const isSheet = $derived(variant === "sheet" || variant === "dialog");
  const cardMax = $derived(maxWidth ?? (isSheet ? 396 : 300));
  const cardPad = $derived(pad ?? (isSheet ? 0 : 12));

  onMount(() => {
    store.overlayCount += 1;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      store.overlayCount = Math.max(0, store.overlayCount - 1);
    };
  });
</script>

<div class="ov-wrap no-drag" class:sheet={isSheet}>
  <button class="ov-scrim" aria-label="Close" onclick={onClose}></button>
  <div class="ov-card" class:card-sheet={isSheet} style="max-width:{cardMax}px; padding:{cardPad}px;">
    {@render children()}
  </div>
</div>

<style>
  .ov-wrap {
    position: fixed;
    inset: 0;
    z-index: var(--z-pop);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
  }
  .ov-wrap.sheet {
    padding: 14px;
  }
  .ov-scrim {
    position: absolute;
    inset: 0;
    background: color-mix(in oklab, #171526 30%, transparent);
    -webkit-backdrop-filter: blur(2px);
    backdrop-filter: blur(2px);
    cursor: default;
    animation: fade 0.16s ease both;
  }
  .ov-card {
    position: relative;
    z-index: 1;
    width: 100%;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    background: var(--card);
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-pop);
    transform-origin: center;
    animation: pop 0.2s cubic-bezier(0.22, 1, 0.36, 1) both;
  }
  .ov-card.card-sheet {
    border-radius: var(--radius-card);
    overflow: hidden; /* the sheet manages its own inner scroll region */
  }
  @media (prefers-reduced-motion: reduce) {
    .ov-scrim,
    .ov-card {
      animation: none !important;
    }
  }
</style>
