<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../icons/Icon.svelte";
  import Overlay from "../ui/Overlay.svelte";
  import { api } from "../api";
  import { isMuted, setMuted, breakStart } from "../sound";
  import type { BreakSettings } from "../types";

  let { onClose }: { onClose: () => void } = $props();

  let soundOn = $state(!isMuted());
  function toggleSound() {
    soundOn = !soundOn;
    setMuted(!soundOn);
    if (soundOn) breakStart(); // preview the cue when enabling
  }

  let s = $state<BreakSettings>({ enabled: true, work_min: 50, duration_min: 5, snooze_min: 5 });
  onMount(async () => {
    try { s = await api.breakSettings(); } catch { /* dev */ }
  });

  // Persist on every change so it takes effect immediately.
  async function save() {
    try { await api.setBreakSettings(s); } catch { /* dev */ }
  }
  function bump(key: "work_min" | "duration_min" | "snooze_min", delta: number, lo: number, hi: number) {
    s = { ...s, [key]: Math.max(lo, Math.min(hi, s[key] + delta)) };
    save();
  }
  function toggle() {
    s = { ...s, enabled: !s.enabled };
    save();
  }
</script>

<Overlay variant="popover" maxWidth={280} pad={12} {onClose}>
  <div class="flex items-center gap-2 mb-3">
    <span class="orb"><Icon name="coffee" size={14} /></span>
    <div class="text-[13px] font-semibold text-ink flex-1">Breaks</div>
    <button class="icon-btn" aria-label="Close" onclick={onClose}><Icon name="x" size={15} /></button>
  </div>

  <!-- enable toggle -->
  <button class="srow" onclick={toggle}>
    <div class="text-left flex-1">
      <div class="text-[12.5px] font-medium text-ink">Remind me to take breaks</div>
      <div class="text-[10.5px] text-ink-faint">Ultradian rest after focused work</div>
    </div>
    <span class="switch" class:on={s.enabled}><span class="knob"></span></span>
  </button>

  <!-- sound toggle -->
  <button class="srow" onclick={toggleSound}>
    <div class="text-left flex-1">
      <div class="text-[12.5px] font-medium text-ink">Sound cues</div>
      <div class="text-[10.5px] text-ink-faint">Gentle chimes for breaks and completed tasks</div>
    </div>
    <span class="switch" class:on={soundOn}><span class="knob"></span></span>
  </button>

  <div class="my-2 h-px" style="background: var(--line);"></div>

  <div class="flex flex-col gap-1" class:disabled={!s.enabled}>
    {@render stepper("Work interval", s.work_min, "min", () => bump("work_min", -5, 5, 240), () => bump("work_min", 5, 5, 240))}
    {@render stepper("Break length", s.duration_min, "min", () => bump("duration_min", -1, 1, 60), () => bump("duration_min", 1, 1, 60))}
    {@render stepper("Snooze", s.snooze_min, "min", () => bump("snooze_min", -1, 1, 60), () => bump("snooze_min", 1, 1, 60))}
  </div>

  <button class="done" onclick={onClose}>Done</button>
</Overlay>

{#snippet stepper(label: string, value: number, unit: string, dec: () => void, inc: () => void)}
  <div class="flex items-center justify-between py-1">
    <span class="text-[12.5px] text-ink-soft">{label}</span>
    <div class="flex items-center gap-1.5">
      <button class="stp" onclick={dec} aria-label="Decrease {label}"><Icon name="minus" size={14} /></button>
      <span class="text-[12.5px] font-semibold text-ink tabular-nums w-[52px] text-center">{value} {unit}</span>
      <button class="stp" onclick={inc} aria-label="Increase {label}"><Icon name="plus" size={14} /></button>
    </div>
  </div>
{/snippet}

<style>
  .orb {
    width: 24px; height: 24px; display: grid; place-items: center; border-radius: 999px;
    color: #2aa39a; background: color-mix(in oklab, #2aa39a 12%, white);
  }
  .srow {
    width: 100%; display: flex; align-items: center; gap: 10px;
    padding: 6px 4px; border-radius: 8px; transition: background 0.12s ease;
  }
  .srow:hover { background: rgba(0, 0, 0, 0.03); }
  .switch {
    width: 38px; height: 22px; border-radius: 999px; flex-shrink: 0;
    background: rgba(0, 0, 0, 0.14); position: relative; transition: background 0.16s ease;
  }
  .switch.on { background: #2aa39a; }
  .knob {
    position: absolute; top: 2px; left: 2px; width: 18px; height: 18px; border-radius: 999px;
    background: #fff; box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25); transition: transform 0.16s ease;
  }
  .switch.on .knob { transform: translateX(16px); }
  .disabled { opacity: 0.4; pointer-events: none; }
  .stp {
    width: 24px; height: 24px; display: grid; place-items: center; border-radius: 6px;
    color: var(--color-ink-soft); background: rgba(0, 0, 0, 0.04);
    border: 0.5px solid var(--line); transition: background 0.12s ease;
  }
  .stp:hover { background: rgba(0, 0, 0, 0.08); color: var(--color-ink); }
  .done {
    width: 100%; margin-top: 10px; padding: 7px; border-radius: 8px;
    font-size: 13px; font-weight: 600; color: white;
    background: linear-gradient(180deg, #2bbcb1, var(--color-accent));
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.45), 0 1px 1.5px rgba(0, 0, 0, 0.18);
  }
  .done:hover { filter: brightness(1.04); }
</style>
