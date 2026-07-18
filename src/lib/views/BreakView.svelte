<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../icons/Icon.svelte";
  import ProgressRing from "../ui/ProgressRing.svelte";
  import { api } from "../api";
  import { store, refreshSnapshot } from "../store.svelte";
  import { breakStart } from "../sound";
  import { assessTasks, dayBuffer } from "../risk";
  import { fmtMin, catColor } from "../format";
  import type { BreakSettings } from "../types";

  // Calm violet for the rest surface (on-brand with the app accent). The var is
  // still threaded through as --teal downstream; only the value changed.
  const TEAL = "#8a7ef2";
  const URGENT = "#ff6b6b";
  const BEHIND = "#f0a437";

  const snap = $derived(store.snapshot);
  const onBreak = $derived(snap?.on_break ?? false);
  const workedMin = $derived(snap?.worked_since_break_min ?? 0);

  let settings = $state<BreakSettings>({ enabled: true, work_min: 50, duration_min: 5, snooze_min: 5 });
  onMount(async () => {
    try { settings = await api.breakSettings(); } catch { /* dev */ }
    // Chime when the break prompt appears. The "break over" cue is played by
    // the engine at the exact end of the timer (reliable even when hidden).
    if (!snap?.on_break) breakStart();
  });

  // Countdown anchored to an absolute end time. Deriving "left" from
  // (endAt - now) and re-anchoring only on real drift (>1.5s) means a fresh
  // snapshot never yanks the display back a second: no flicker.
  let endAt = $state<number | null>(null);
  let nowMs = $state(Date.now());
  $effect(() => {
    const s = snap;
    if (!s?.on_break) { endAt = null; return; }
    if (s.break_remaining_sec <= 0) return; // over; hold the local clock at 0
    const serverEnd = Date.now() + s.break_remaining_sec * 1000;
    if (endAt == null || Math.abs(serverEnd - endAt) > 1500) endAt = serverEnd;
  });
  $effect(() => {
    if (!onBreak) return;
    const id = setInterval(() => { nowMs = Date.now(); }, 250);
    return () => clearInterval(id);
  });

  const totalSec = $derived(Math.max(1, settings.duration_min * 60));
  const left = $derived(
    endAt != null
      ? Math.max(0, Math.ceil((endAt - nowMs) / 1000))
      : Math.max(0, onBreak ? (snap?.break_remaining_sec ?? 0) : 0),
  );
  const done = $derived(onBreak && left <= 0);
  const ringFrac = $derived(onBreak && !done ? Math.max(0, Math.min(1, left / totalSec)) : 0);
  const mmss = $derived(`${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`);

  // At-risk work to surface while resting, most urgent first.
  const risky = $derived(assessTasks(store.tasks, snap).filter((r) => r.level !== "ok").slice(0, 4));
  const buffer = $derived(dayBuffer(snap));
  function riskColor(level: string): string {
    return level === "urgent" ? URGENT : BEHIND;
  }

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
  <div class="relative flex items-center justify-end px-4 pt-3" data-tauri-drag-region>
    <button class="icon-btn no-drag" aria-label="Close" onclick={() => api.dismiss()}>
      <Icon name="chevron-down" size={18} />
    </button>
  </div>

  <div class="relative flex-1 flex flex-col items-center justify-center text-center px-6 pb-10 gap-7">
    <div class="flex flex-col items-center">
      {#if !onBreak}
        <!-- Prompt -->
        <div class="orb mb-6"><Icon name="moon-star" size={36} /></div>
        <div class="text-[28px] font-semibold leading-tight" style="color: #f4f6f7;">Time for a break</div>
        <p class="text-[15px] leading-relaxed mt-3 max-w-[360px]" style="color: rgba(255,255,255,0.62);">
          You've focused for <span class="font-semibold" style="color: #dfe6e5;">{workedLabel(workedMin)}</span> straight.
          A few minutes away keeps you sharp.
        </p>
        <div class="w-full max-w-[320px] mt-7 flex flex-col gap-2.5">
          <button class="btn-break-primary" onclick={take}>Take a {settings.duration_min}-minute break</button>
          <div class="flex gap-2.5">
            <button class="btn-ghost-d flex-1" onclick={snooze}>Snooze {settings.snooze_min}m</button>
            <button class="btn-ghost-d flex-1" onclick={skip}>Skip</button>
          </div>
        </div>
      {:else}
        <!-- Countdown -->
        <ProgressRing value={done ? 1 : ringFrac} size={260} stroke={13} color={TEAL} track="rgba(255,255,255,0.1)">
          <div class="flex flex-col items-center leading-none">
            {#if done}
              <div class="text-[26px] font-semibold" style="color: var(--teal);">Rested</div>
              <div class="text-[12px] mt-2" style="color: rgba(255,255,255,0.5);">ready when you are</div>
            {:else}
              <div class="tabular-nums font-semibold" style="font-size:64px; letter-spacing:-1.5px; color:#f4f6f7;">{mmss}</div>
              <div class="text-[12px] mt-2" style="color: rgba(255,255,255,0.5);">look away from the screen</div>
            {/if}
          </div>
        </ProgressRing>
        <div class="text-[14px] font-semibold mt-5" style="color:#eef1f2;">{done ? "Break complete" : "On a break"}</div>
        <div class="w-full max-w-[300px] mt-6 flex flex-col gap-2.5">
          <button class="btn-break-primary" onclick={backToWork}>Back to work</button>
          {#if !done}
            <button class="btn-ghost-d" onclick={keepResting}>Keep resting</button>
          {/if}
        </div>
      {/if}
    </div>

    <!-- At-risk work: what to attack the moment the break ends. -->
    {#if risky.length > 0}
      <div class="risk-panel">
        <div class="risk-head">
          <span>Needs your focus</span>
          {#if buffer <= 0}
            <span class="buffer-chip over">Buffer gone · {fmtMin(-buffer)} over</span>
          {:else}
            <span class="buffer-chip">Buffer {fmtMin(buffer)}</span>
          {/if}
        </div>
        <div class="risk-list">
          {#each risky as r (r.task.id)}
            <div class="risk-row" style="--rc: {riskColor(r.level)};">
              <span class="risk-bar"></span>
              <span class="risk-dot" style="background: {catColor(r.task.category_color)};"></span>
              <div class="risk-main">
                <div class="risk-title">{r.task.title}</div>
                <div class="risk-sub">
                  {r.task.tracked_min === 0 ? "not started" : `${fmtMin(r.task.tracked_min)} of ${fmtMin(r.task.estimate_min ?? 0)}`}
                  · {fmtMin(r.remaining)} left
                </div>
              </div>
              <span class="risk-badge" style="color: {riskColor(r.level)}; border-color: color-mix(in oklab, {riskColor(r.level)} 45%, transparent); background: color-mix(in oklab, {riskColor(r.level)} 14%, transparent);">
                {r.level === "urgent" ? "Urgent" : "Behind"}
              </span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .break-surface {
    /* Full-screen dimmed overlay (SafeEyes style): a near-opaque dark wash with
       only a faint teal glow, so it reads as "step away from the screen". */
    background:
      radial-gradient(70% 55% at 50% 32%, color-mix(in oklab, var(--teal) 12%, transparent), transparent 60%),
      linear-gradient(180deg, rgba(12, 14, 18, 0.985), rgba(7, 9, 12, 0.99));
    overflow: hidden;
  }
  .breath {
    background: radial-gradient(30% 24% at 50% 40%, color-mix(in oklab, var(--teal) 18%, transparent), transparent 72%);
    animation: breathe 7s ease-in-out infinite;
  }
  @keyframes breathe {
    0%, 100% { transform: scale(0.9); opacity: 0.4; }
    50% { transform: scale(1.14); opacity: 0.75; }
  }
  .orb {
    width: 96px; height: 96px;
    display: grid; place-items: center;
    border-radius: 999px;
    color: var(--teal);
    background: color-mix(in oklab, var(--teal) 20%, transparent);
    border: 0.5px solid color-mix(in oklab, var(--teal) 45%, transparent);
    box-shadow: 0 8px 30px -8px color-mix(in oklab, var(--teal) 70%, transparent);
    animation: breathe 7s ease-in-out infinite;
  }
  :global(.break-surface .icon-btn) { color: rgba(255, 255, 255, 0.55); }
  :global(.break-surface .icon-btn:hover) { color: rgba(255, 255, 255, 0.9); }

  .btn-break-primary {
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 13.5px; font-weight: 600; color: white;
    padding: 10px 16px; border-radius: 9px;
    background: linear-gradient(180deg, color-mix(in oklab, var(--teal) 86%, white), var(--teal));
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.4), 0 8px 20px -8px color-mix(in oklab, var(--teal) 80%, transparent);
    transition: filter 0.12s ease;
  }
  .btn-break-primary:hover { filter: brightness(1.06); }
  .btn-break-primary:active { filter: brightness(0.96); }
  .btn-ghost-d {
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 13px; font-weight: 500;
    padding: 9px 14px; border-radius: 9px;
    color: rgba(255, 255, 255, 0.82);
    background: rgba(255, 255, 255, 0.06);
    border: 0.5px solid rgba(255, 255, 255, 0.12);
    transition: background 0.12s ease, color 0.12s ease;
  }
  .btn-ghost-d:hover { background: rgba(255, 255, 255, 0.11); color: white; }

  /* At-risk panel */
  .risk-panel {
    width: 100%;
    max-width: 440px;
    border-radius: 14px;
    padding: 12px;
    background: rgba(255, 255, 255, 0.045);
    border: 0.5px solid rgba(255, 255, 255, 0.1);
    text-align: left;
  }
  .risk-head {
    display: flex; align-items: center; justify-content: space-between;
    font-size: 10.5px; font-weight: 700; letter-spacing: 0.08em; text-transform: uppercase;
    color: rgba(255, 255, 255, 0.5);
    margin-bottom: 8px;
  }
  .buffer-chip {
    font-weight: 600; letter-spacing: 0.02em; text-transform: none;
    padding: 2px 8px; border-radius: 999px;
    color: rgba(255, 255, 255, 0.7);
    background: rgba(255, 255, 255, 0.07);
  }
  .buffer-chip.over { color: #ffd0d0; background: color-mix(in oklab, #ff6b6b 22%, transparent); }
  .risk-list { display: flex; flex-direction: column; gap: 6px; }
  .risk-row {
    position: relative;
    display: flex; align-items: center; gap: 9px;
    padding: 8px 10px 8px 12px;
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.04);
    overflow: hidden;
  }
  .risk-bar { position: absolute; left: 0; top: 0; bottom: 0; width: 3px; background: var(--rc); }
  .risk-dot { width: 8px; height: 8px; border-radius: 999px; flex-shrink: 0; }
  .risk-main { flex: 1; min-width: 0; }
  .risk-title { font-size: 13px; font-weight: 600; color: #eef1f2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .risk-sub { font-size: 11px; color: rgba(255, 255, 255, 0.5); margin-top: 1px; }
  .risk-badge {
    flex-shrink: 0;
    font-size: 10px; font-weight: 700; letter-spacing: 0.04em;
    padding: 3px 8px; border-radius: 999px;
    border: 0.5px solid transparent;
  }
</style>
