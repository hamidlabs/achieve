<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../icons/Icon.svelte";
  import ProgressRing from "../ui/ProgressRing.svelte";
  import { api } from "../api";
  import { store, refreshSnapshot } from "../store.svelte";
  import { breakStart } from "../sound";
  import type { BreakSettings } from "../types";

  const TEAL = "#2aa39a";

  const snap = $derived(store.snapshot);
  const onBreak = $derived(snap?.on_break ?? false);
  const workedMin = $derived(snap?.worked_since_break_min ?? 0);

  let settings = $state<BreakSettings>({ enabled: true, work_min: 50, duration_min: 5, snooze_min: 5 });
  onMount(async () => {
    try { settings = await api.breakSettings(); } catch { /* dev */ }
    // Chime when the break prompt appears. The "break over" cue is played by
    // the engine at the exact end of the timer, so it fires reliably even when
    // this window is hidden or throttled.
    if (!snap?.on_break) breakStart();
  });

  // Smooth local countdown, re-synced to the backend's value each snapshot tick.
  let remaining = $state(0);
  $effect(() => {
    if (snap) remaining = snap.break_remaining_sec;
  });
  $effect(() => {
    if (!onBreak) return;
    const id = setInterval(() => { remaining -= 1; }, 1000);
    return () => clearInterval(id);
  });

  const totalSec = $derived(Math.max(1, settings.duration_min * 60));
  const left = $derived(Math.max(0, remaining));
  const done = $derived(onBreak && remaining <= 0);
  // Ring drains as the break elapses.
  const ringFrac = $derived(onBreak ? Math.max(0, Math.min(1, left / totalSec)) : 0);
  const mmss = $derived(
    `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`,
  );

  function workedLabel(m: number): string {
    if (m >= 60) {
      const h = Math.floor(m / 60), r = m % 60;
      return r ? `${h}h ${r}m` : `${h}h`;
    }
    return `${m}m`;
  }

  async function take() {
    await api.startBreak();
    await refreshSnapshot();
  }
  async function snooze() {
    await api.snoozeBreak(settings.snooze_min);
    api.dismiss();
  }
  async function skip() {
    await api.skipBreak();
    api.dismiss();
  }
  async function backToWork() {
    await api.endBreak(true);
    api.dismiss();
  }
  async function keepResting() {
    api.dismiss();
  }
</script>

<div class="break-surface relative h-full w-full flex flex-col" style="--teal: {TEAL};">
  <div class="absolute inset-0 breath" aria-hidden="true"></div>

  <!-- minimal drag bar + close -->
  <div class="relative flex items-center justify-end px-3 pt-3" data-tauri-drag-region>
    <button class="icon-btn no-drag" aria-label="Close" onclick={() => api.dismiss()}>
      <Icon name="chevron-down" size={18} />
    </button>
  </div>

  <div class="relative flex-1 flex flex-col items-center justify-center text-center px-7 pb-8 -mt-4">
    {#if !onBreak}
      <!-- Prompt -->
      <div class="orb mb-5">
        <Icon name="moon-star" size={30} />
      </div>
      <div class="text-[20px] font-semibold text-ink leading-tight">Time for a break</div>
      <p class="text-[13px] text-ink-faint leading-relaxed mt-2 max-w-[280px]">
        You've focused for <span class="font-semibold text-ink-soft">{workedLabel(workedMin)}</span> straight.
        A few minutes away keeps you sharp, your eyes and your focus.
      </p>

      <div class="w-full max-w-[300px] mt-7 flex flex-col gap-2.5">
        <button class="btn-break-primary" onclick={take}>
          Take a {settings.duration_min}-minute break
        </button>
        <div class="flex gap-2.5">
          <button class="btn btn-soft flex-1" onclick={snooze}>Snooze {settings.snooze_min}m</button>
          <button class="btn btn-soft flex-1" onclick={skip}>Skip</button>
        </div>
      </div>
    {:else}
      <!-- Countdown -->
      <ProgressRing value={done ? 1 : ringFrac} size={184} stroke={11} color={TEAL}>
        <div class="flex flex-col items-center leading-none">
          {#if done}
            <div class="text-[19px] font-semibold" style="color: var(--teal);">Rested</div>
            <div class="text-[11px] text-ink-faint mt-1.5">ready when you are</div>
          {:else}
            <div class="tabular-nums font-semibold text-ink" style="font-size:42px; letter-spacing:-1px;">{mmss}</div>
            <div class="text-[11px] text-ink-faint mt-1.5">look away from the screen</div>
          {/if}
        </div>
      </ProgressRing>

      <div class="text-[14px] font-semibold text-ink mt-5">
        {done ? "Break complete" : "On a break"}
      </div>

      <div class="w-full max-w-[300px] mt-6 flex flex-col gap-2.5">
        <button class="btn-break-primary" onclick={backToWork}>Back to work</button>
        {#if !done}
          <button class="btn btn-soft" onclick={keepResting}>Keep resting</button>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .break-surface {
    background:
      radial-gradient(120% 90% at 50% 0%, color-mix(in oklab, var(--teal) 14%, white), transparent 60%),
      linear-gradient(180deg, #ffffff, #f3f7f6);
    overflow: hidden;
  }
  /* Signature: a slow breathing glow behind the content. */
  .breath {
    background: radial-gradient(40% 32% at 50% 42%, color-mix(in oklab, var(--teal) 22%, transparent), transparent 70%);
    animation: breathe 7s ease-in-out infinite;
  }
  @keyframes breathe {
    0%, 100% { transform: scale(0.9); opacity: 0.55; }
    50% { transform: scale(1.12); opacity: 0.95; }
  }
  .orb {
    width: 78px;
    height: 78px;
    display: grid;
    place-items: center;
    border-radius: 999px;
    color: var(--teal);
    background: color-mix(in oklab, var(--teal) 12%, white);
    border: 0.5px solid color-mix(in oklab, var(--teal) 30%, white);
    box-shadow: 0 8px 22px -10px color-mix(in oklab, var(--teal) 60%, transparent);
    animation: breathe 7s ease-in-out infinite;
  }
  .btn-break-primary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 13.5px;
    font-weight: 600;
    color: white;
    padding: 9px 16px;
    border-radius: 8px;
    background: linear-gradient(180deg, color-mix(in oklab, var(--teal) 86%, white), var(--teal));
    box-shadow:
      inset 0 0.5px 0 rgba(255, 255, 255, 0.4),
      0 6px 16px -8px color-mix(in oklab, var(--teal) 80%, transparent);
    transition: filter 0.12s ease;
  }
  .btn-break-primary:hover { filter: brightness(1.04); }
  .btn-break-primary:active { filter: brightness(0.96); }
</style>
