<script lang="ts">
  // Inline reminder editor (no nested overlay, so the dialog's auto-fit sizing
  // stays correct). Emits a ReminderSpec; the parent persists it now (existing
  // task) or defers it until the new task is created.
  import { slide } from "svelte/transition";
  import Icon from "../icons/Icon.svelte";
  import type { Reminder, ReminderChannel, ReminderSpec } from "../types";

  interface Props {
    existing?: Reminder | null;
    onSave: (spec: ReminderSpec) => void;
    onCancel: () => void;
  }
  let { existing = null, onSave, onCancel }: Props = $props();

  function ymd(d: Date): string {
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    return `${d.getFullYear()}-${m}-${day}`;
  }
  const tomorrow = (() => {
    const d = new Date();
    d.setDate(d.getDate() + 1);
    return ymd(d);
  })();

  // Parse an existing reminder, else sensible defaults (tomorrow 9am).
  const initDate = existing ? existing.remind_at_local.split(" ")[0] : tomorrow;
  const initTime = existing ? existing.remind_at_local.split(" ")[1] : "09:00";

  type RepeatMode =
    | "none" | "daily" | "weekdays" | "weekly" | "biweekly" | "monthly" | "yearly" | "custom";
  type CustomUnit = "days" | "weeks" | "months";

  function parseRepeat(rrule: string | null): { mode: RepeatMode; n: number; unit: CustomUnit } {
    if (!rrule) return { mode: "none", n: 2, unit: "weeks" };
    if (rrule.startsWith("every:")) {
      const [, nStr, unit] = rrule.split(":");
      return { mode: "custom", n: Number(nStr) || 2, unit: (unit as CustomUnit) || "weeks" };
    }
    const known = ["daily", "weekdays", "weekly", "biweekly", "monthly", "yearly"];
    return { mode: (known.includes(rrule) ? rrule : "none") as RepeatMode, n: 2, unit: "weeks" };
  }
  const initRepeat = parseRepeat(existing?.rrule ?? null);

  // End condition.
  type EndMode = "never" | "on" | "after";
  const initEnd: EndMode = existing?.rrule_until ? "on" : existing?.rrule_count != null ? "after" : "never";

  let date = $state(initDate);
  let time = $state(initTime);
  let mode = $state<RepeatMode>(initRepeat.mode);
  let customN = $state(initRepeat.n);
  let customUnit = $state<CustomUnit>(initRepeat.unit);
  let endMode = $state<EndMode>(initEnd);
  let endDate = $state(existing?.rrule_until ?? "");
  // Backend stores "fires after the first"; show total occurrences.
  let occurrences = $state((existing?.rrule_count ?? 4) + 1);
  let channel = $state<ReminderChannel>(existing?.channel ?? "both");
  let note = $state(existing?.note ?? "");

  const repeats = $derived(mode !== "none");

  function toRrule(): string | null {
    if (mode === "none") return null;
    if (mode === "custom") return `every:${Math.max(1, customN)}:${customUnit}`;
    return mode;
  }

  // Client-side "this is in the past" hint for a one-shot.
  const isPast = $derived.by(() => {
    const dt = new Date(`${date}T${time || "00:00"}:00`);
    return !repeats && dt.getTime() < Date.now();
  });

  function submit() {
    if (!date || !time) return;
    const spec: ReminderSpec = {
      remind_at: `${date} ${time}`,
      rrule: toRrule(),
      until: repeats && endMode === "on" && endDate ? endDate : null,
      count: repeats && endMode === "after" ? Math.max(1, occurrences) - 1 : null,
      channel,
      note: note.trim() || null,
    };
    onSave(spec);
  }

  const REPEATS: { v: RepeatMode; label: string }[] = [
    { v: "none", label: "Does not repeat" },
    { v: "daily", label: "Every day" },
    { v: "weekdays", label: "Every weekday (Mon–Fri)" },
    { v: "weekly", label: "Every week" },
    { v: "biweekly", label: "Every 2 weeks" },
    { v: "monthly", label: "Every month" },
    { v: "yearly", label: "Every year" },
    { v: "custom", label: "Custom…" },
  ];
  const CHANNELS: { v: ReminderChannel; label: string; icon: string }[] = [
    { v: "notification", label: "Notify", icon: "bell" },
    { v: "email", label: "Email", icon: "mail" },
    { v: "both", label: "Both", icon: "check" },
  ];
</script>

<div class="re" transition:slide={{ duration: 160 }}>
  <!-- When -->
  <div class="grid grid-cols-2 gap-2">
    <label class="fld">
      <span class="lbl">Date</span>
      <input type="date" class="field no-drag" bind:value={date} min={ymd(new Date())} />
    </label>
    <label class="fld">
      <span class="lbl">Time</span>
      <input type="time" class="field no-drag" bind:value={time} />
    </label>
  </div>

  {#if isPast}
    <p class="past"><Icon name="clock" size={12} /> That time has passed — it'll send right away.</p>
  {/if}

  <!-- Repeat -->
  <label class="fld">
    <span class="lbl">Repeat</span>
    <select class="field no-drag" bind:value={mode}>
      {#each REPEATS as r (r.v)}
        <option value={r.v}>{r.label}</option>
      {/each}
    </select>
  </label>

  {#if mode === "custom"}
    <div class="flex items-center gap-2" transition:slide={{ duration: 140 }}>
      <span class="lbl shrink-0">Every</span>
      <input type="number" min="1" max="365" class="field no-drag w-16" bind:value={customN} />
      <select class="field no-drag flex-1" bind:value={customUnit}>
        <option value="days">day(s)</option>
        <option value="weeks">week(s)</option>
        <option value="months">month(s)</option>
      </select>
    </div>
  {/if}

  <!-- Ends (only when repeating) -->
  {#if repeats}
    <div class="fld" transition:slide={{ duration: 140 }}>
      <span class="lbl">Ends</span>
      <div class="flex items-center gap-2 flex-wrap">
        <div class="seg">
          {#each [["never", "Never"], ["on", "On date"], ["after", "After"]] as [v, l] (v)}
            <button class="seg-b" class:on={endMode === v} onclick={() => (endMode = v as EndMode)}>{l}</button>
          {/each}
        </div>
        {#if endMode === "on"}
          <input type="date" class="field no-drag" bind:value={endDate} min={date} />
        {:else if endMode === "after"}
          <span class="flex items-center gap-1.5 text-[12px] text-ink-faint">
            <input type="number" min="1" max="999" class="field no-drag w-16" bind:value={occurrences} /> times
          </span>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Channel -->
  <div class="fld">
    <span class="lbl">Notify via</span>
    <div class="seg">
      {#each CHANNELS as c (c.v)}
        <button class="seg-b" class:on={channel === c.v} onclick={() => (channel = c.v)}>
          <Icon name={c.icon} size={12} /> {c.label}
        </button>
      {/each}
    </div>
  </div>

  <!-- Note -->
  <label class="fld">
    <span class="lbl">Note <span class="text-ink-ghost">(optional)</span></span>
    <input class="field no-drag" style="user-select:text;" placeholder="e.g. Bring the signed copy"
      bind:value={note} onkeydown={(e) => e.key === "Enter" && submit()} />
  </label>

  <div class="flex items-center gap-2 pt-0.5">
    <div class="flex-1 text-[11px] text-ink-ghost">
      {existing ? "Editing reminder" : "New reminder"}
    </div>
    <button class="btn btn-soft btn-sm" onclick={onCancel}>Cancel</button>
    <button class="btn btn-primary btn-sm" onclick={submit} disabled={!date || !time}>
      <Icon name="check" size={13} /> {existing ? "Save" : "Add"}
    </button>
  </div>
</div>

<style>
  .re {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 11px;
    border: 0.5px solid var(--line);
    border-radius: var(--radius-md);
    background: color-mix(in oklab, var(--color-accent) 4%, white);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .lbl {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--color-ink-faint);
  }
  .past {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    color: var(--color-warn);
    margin: -2px 0 0;
  }
  /* Segmented control. */
  .seg {
    display: inline-flex;
    padding: 2px;
    gap: 2px;
    background: color-mix(in oklab, var(--color-ink) 7%, transparent);
    border-radius: var(--radius-sm);
  }
  .seg-b {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 9px;
    font-size: 11.5px;
    font-weight: 550;
    color: var(--color-ink-faint);
    border-radius: calc(var(--radius-sm) - 2px);
    transition: all 0.12s ease;
  }
  .seg-b.on {
    background: #fff;
    color: var(--color-ink);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
  }
  .btn-sm {
    padding: 5px 11px;
    font-size: 12px;
  }
  .w-16 {
    width: 62px;
  }
</style>
