<script lang="ts">
  // Passive dimming overlay shown on the SECOND monitor during a rest break.
  // No controls (those live on the primary break window); it darkens the other
  // screen ~halfway and mirrors the countdown so both monitors read as "on a
  // break".
  //
  // Robustness: the veil is a normally-hidden window, so it can miss the pushed
  // snapshot that fires the instant a break starts. It therefore (1) fetches the
  // snapshot itself on mount, (2) polls it on a slow cadence as a safety net, and
  // (3) listens for live pushes. Any of the three anchors the countdown, which
  // then runs locally off an absolute end time (no per-tick server dependency).
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import ProgressRing from "../ui/ProgressRing.svelte";
  import type { Snapshot, BreakSettings } from "../types";

  const ACCENT = "#8a7ef2";

  let onBreak = $state(false);
  let endAt = $state<number | null>(null);
  let nowMs = $state(Date.now());
  let totalSec = $state(300); // for the ring fraction; grows to the real duration

  // Anchor the countdown to an absolute end time so a fresh reading doesn't yank
  // the display; re-anchor only on real drift (>1.5s), same trick as BreakView.
  function anchor(s: Snapshot) {
    onBreak = s.on_break;
    if (!s.on_break) {
      endAt = null;
      return;
    }
    const rem = Math.max(0, s.break_remaining_sec);
    if (rem > totalSec) totalSec = rem;
    const serverEnd = Date.now() + rem * 1000;
    if (endAt == null || Math.abs(serverEnd - endAt) > 1500) endAt = serverEnd;
  }

  async function poll() {
    try {
      anchor(await invoke<Snapshot>("get_snapshot"));
    } catch {
      /* backend momentarily unavailable; the next poll retries */
    }
  }

  onMount(() => {
    // Real break length for an accurate ring, and an immediate anchor.
    invoke<BreakSettings>("get_break_settings")
      .then((b) => { totalSec = Math.max(totalSec, b.duration_min * 60); })
      .catch(() => {});
    poll();

    const un = listen<Snapshot>("snapshot", (e) => anchor(e.payload));
    const tick = setInterval(() => { nowMs = Date.now(); }, 250);
    const pollId = setInterval(poll, 1500);
    return () => {
      un.then((f) => f());
      clearInterval(tick);
      clearInterval(pollId);
    };
  });

  const remaining = $derived(
    endAt != null ? Math.max(0, Math.ceil((endAt - nowMs) / 1000)) : 0,
  );
  const done = $derived(onBreak && remaining <= 0);
  const frac = $derived(
    done ? 1 : onBreak ? Math.max(0, Math.min(1, remaining / Math.max(1, totalSec))) : 0,
  );
  const mmss = $derived(`${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`);
</script>

<div class="veil-surface" style="--accent: {ACCENT};">
  <div class="glow" aria-hidden="true"></div>
  <div class="content">
    <div class="eyebrow">{done ? "Break complete" : "On a break"}</div>
    <ProgressRing value={frac} size={228} stroke={12} color={ACCENT} track="rgba(255,255,255,0.12)">
      {#if done}
        <div class="rested" style="color: var(--accent);">Rested</div>
        <div class="sub">ready when you are</div>
      {:else}
        <div class="big tabular-nums">{mmss}</div>
        <div class="sub">look away from the screen</div>
      {/if}
    </ProgressRing>
  </div>
</div>

<style>
  .veil-surface {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    overflow: hidden;
    /* ~half-opacity dim: the other monitor stays faintly visible behind it. */
    background:
      radial-gradient(60% 45% at 50% 40%, color-mix(in oklab, var(--accent) 16%, transparent), transparent 62%),
      linear-gradient(180deg, rgba(12, 12, 20, 0.5), rgba(8, 8, 14, 0.56));
  }
  .glow {
    position: absolute;
    inset: 0;
    background: radial-gradient(26% 20% at 50% 42%, color-mix(in oklab, var(--accent) 20%, transparent), transparent 72%);
    animation: breathe 7s ease-in-out infinite;
  }
  @keyframes breathe {
    0%, 100% { transform: scale(0.92); opacity: 0.35; }
    50% { transform: scale(1.1); opacity: 0.6; }
  }
  @media (prefers-reduced-motion: reduce) {
    .glow { animation: none; }
  }
  .content {
    position: relative;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 20px;
  }
  .eyebrow {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: rgba(255, 255, 255, 0.5);
  }
  .big {
    font-size: 60px;
    font-weight: 600;
    letter-spacing: -1.5px;
    line-height: 1;
    color: #f4f6f7;
  }
  .rested {
    font-size: 34px;
    font-weight: 700;
    line-height: 1;
  }
  .sub {
    font-size: 12.5px;
    color: rgba(255, 255, 255, 0.5);
    margin-top: 10px;
  }
</style>
