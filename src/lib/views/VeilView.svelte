<script lang="ts">
  // Passive dimming overlay shown on the SECOND monitor during a rest break.
  // No controls (those live on the primary break window); it just darkens the
  // other screen and mirrors the countdown so both monitors read as "on a break".
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import type { Snapshot } from "../types";

  // Calm violet glow for the rest surface (on-brand with the app accent, but
  // soft against the near-black dim). The var is still named --teal downstream.
  const TEAL = "#8a7ef2";

  let remaining = $state(0); // seconds left in the break
  let onBreak = $state(false);

  // Anchor the countdown to an absolute end time so a fresh snapshot doesn't
  // yank the display; re-anchor only on real drift (same trick as BreakView).
  let endAt: number | null = null;
  let nowMs = $state(Date.now());

  onMount(() => {
    const un = listen<Snapshot>("snapshot", (e) => {
      const s = e.payload;
      onBreak = s.on_break;
      if (!s.on_break) { endAt = null; return; }
      const serverEnd = Date.now() + Math.max(0, s.break_remaining_sec) * 1000;
      if (endAt == null || Math.abs(serverEnd - endAt) > 1500) endAt = serverEnd;
    });
    const id = setInterval(() => { nowMs = Date.now(); }, 250);
    return () => { un.then((f) => f()); clearInterval(id); };
  });

  $effect(() => {
    remaining = endAt != null ? Math.max(0, Math.ceil((endAt - nowMs) / 1000)) : 0;
  });

  const done = $derived(onBreak && remaining <= 0);
  const mmss = $derived(`${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`);
</script>

<div class="veil-surface" style="--teal: {TEAL};">
  <div class="glow" aria-hidden="true"></div>
  <div class="content">
    <div class="eyebrow">On a break</div>
    {#if done}
      <div class="big" style="color: var(--teal);">Rested</div>
      <div class="hint">ready when you are</div>
    {:else}
      <div class="big tabular-nums">{mmss}</div>
      <div class="hint">look away from the screen</div>
    {/if}
  </div>
</div>

<style>
  .veil-surface {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    overflow: hidden;
    background:
      radial-gradient(60% 45% at 50% 40%, color-mix(in oklab, var(--teal) 9%, transparent), transparent 62%),
      linear-gradient(180deg, rgba(10, 12, 16, 0.992), rgba(6, 8, 11, 0.995));
  }
  .glow {
    position: absolute;
    inset: 0;
    background: radial-gradient(26% 20% at 50% 42%, color-mix(in oklab, var(--teal) 14%, transparent), transparent 72%);
    animation: breathe 7s ease-in-out infinite;
  }
  @keyframes breathe {
    0%, 100% { transform: scale(0.92); opacity: 0.4; }
    50% { transform: scale(1.1); opacity: 0.7; }
  }
  .content {
    position: relative;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .eyebrow {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: rgba(255, 255, 255, 0.42);
  }
  .big {
    font-size: 72px;
    font-weight: 600;
    letter-spacing: -1.5px;
    line-height: 1;
    color: #f4f6f7;
  }
  .hint {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.45);
    margin-top: 4px;
  }
</style>
