<script lang="ts">
  // Inline reminder editor. Emits a ReminderSpec; the parent persists it now
  // (existing task) or defers it until the new task is created. All controls are
  // on-brand custom pickers (calendar, time, select) — no native date/time/select.
  import { slide } from "svelte/transition";
  import Icon from "../icons/Icon.svelte";
  import DatePicker from "../ui/DatePicker.svelte";
  import TimePicker from "../ui/TimePicker.svelte";
  import Select from "../ui/Select.svelte";
  import { fmtDay, fmtTime } from "../reminders";
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

  type EndMode = "never" | "on" | "after";
  const initEnd: EndMode = existing?.rrule_until ? "on" : existing?.rrule_count != null ? "after" : "never";

  let date = $state(initDate);
  let time = $state(initTime);
  let mode = $state<RepeatMode>(initRepeat.mode);
  let customN = $state(initRepeat.n);
  let customUnit = $state<CustomUnit>(initRepeat.unit);
  let endMode = $state<EndMode>(initEnd);
  let endDate = $state(existing?.rrule_until ?? "");
  let occurrences = $state((existing?.rrule_count ?? 4) + 1);
  let channel = $state<ReminderChannel>(existing?.channel ?? "both");
  let note = $state(existing?.note ?? "");

  let showCal = $state(false);
  let showEndCal = $state(false);

  const repeats = $derived(mode !== "none");

  function toRrule(): string | null {
    if (mode === "none") return null;
    if (mode === "custom") return `every:${Math.max(1, customN)}:${customUnit}`;
    return mode;
  }

  const isPast = $derived.by(() => {
    const dt = new Date(`${date}T${time || "00:00"}:00`);
    return !repeats && dt.getTime() < Date.now();
  });

  function pickDate(d: string | null) {
    if (d) date = d;
    showCal = false;
  }
  function pickEnd(d: string | null) {
    endDate = d ?? "";
    showEndCal = false;
  }

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

  const REPEATS: { value: string; label: string }[] = [
    { value: "none", label: "Does not repeat" },
    { value: "daily", label: "Every day" },
    { value: "weekdays", label: "Every weekday (Mon–Fri)" },
    { value: "weekly", label: "Every week" },
    { value: "biweekly", label: "Every 2 weeks" },
    { value: "monthly", label: "Every month" },
    { value: "yearly", label: "Every year" },
    { value: "custom", label: "Custom…" },
  ];
  const UNITS = [
    { value: "days", label: "day(s)" },
    { value: "weeks", label: "week(s)" },
    { value: "months", label: "month(s)" },
  ];
  const CHANNELS: { v: ReminderChannel; label: string; icon: string }[] = [
    { v: "notification", label: "Notify", icon: "bell" },
    { v: "email", label: "Email", icon: "mail" },
    { v: "both", label: "Both", icon: "check" },
  ];
</script>

<div class="re" transition:slide={{ duration: 160 }}>
  <!-- When: calendar + time -->
  <div class="fld">
    <span class="lbl">When</span>
    <button class="date-row no-drag" class:on={showCal} onclick={() => (showCal = !showCal)}>
      <Icon name="calendar" size={14} style="color: var(--color-accent);" />
      <span class="date-val">{fmtDay(date)}</span>
      <span class="date-time">{fmtTime(time)}</span>
      <Icon name="chevron-down" size={14} class={showCal ? "rotate-180 text-ink-faint" : "text-ink-faint"} />
    </button>
    {#if showCal}
      <div class="cal" transition:slide={{ duration: 150 }}>
        <DatePicker current={date} onPick={pickDate} allowNone={false} />
      </div>
    {/if}
    <div class="mt-2">
      <TimePicker value={time} onChange={(v) => (time = v)} />
    </div>
  </div>

  {#if isPast}
    <p class="past"><Icon name="clock" size={12} /> That time has passed — it'll send right away.</p>
  {/if}

  <!-- Repeat -->
  <div class="fld">
    <span class="lbl">Repeat</span>
    <Select value={mode} options={REPEATS} ariaLabel="Repeat" onChange={(v) => (mode = v as RepeatMode)} />
  </div>

  {#if mode === "custom"}
    <div class="flex items-center gap-2" transition:slide={{ duration: 140 }}>
      <span class="lbl shrink-0 !normal-case !text-[12px] !tracking-normal !text-ink-soft">Every</span>
      <input type="number" min="1" max="365" class="num no-drag" bind:value={customN} aria-label="Interval" />
      <div class="flex-1">
        <Select value={customUnit} options={UNITS} ariaLabel="Unit" compact onChange={(v) => (customUnit = v as CustomUnit)} />
      </div>
    </div>
  {/if}

  <!-- Ends (only when repeating) -->
  {#if repeats}
    <div class="fld" transition:slide={{ duration: 140 }}>
      <span class="lbl">Ends</span>
      <div class="tabs3">
        {#each [["never", "Never"], ["on", "On date"], ["after", "After"]] as [v, l] (v)}
          <button class="tab3" class:on={endMode === v} onclick={() => (endMode = v as EndMode)}>{l}</button>
        {/each}
      </div>
      {#if endMode === "on"}
        <div transition:slide={{ duration: 130 }}>
          <button class="date-row no-drag mt-2" class:on={showEndCal} onclick={() => (showEndCal = !showEndCal)}>
            <Icon name="calendar-clock" size={14} style="color: var(--color-accent);" />
            <span class="date-val">{endDate ? fmtDay(endDate) : "Pick an end date"}</span>
            <Icon name="chevron-down" size={14} class={showEndCal ? "rotate-180 text-ink-faint" : "text-ink-faint"} />
          </button>
          {#if showEndCal}
            <div class="cal" transition:slide={{ duration: 150 }}>
              <DatePicker current={endDate || date} onPick={pickEnd} allowNone={false} />
            </div>
          {/if}
        </div>
      {:else if endMode === "after"}
        <div class="flex items-center gap-2 mt-2 text-[12.5px] text-ink-soft" transition:slide={{ duration: 130 }}>
          <input type="number" min="1" max="999" class="num no-drag" bind:value={occurrences} aria-label="Occurrences" /> times
        </div>
      {/if}
    </div>
  {/if}

  <!-- Notify via -->
  <div class="fld">
    <span class="lbl">Notify via</span>
    <div class="chtabs">
      {#each CHANNELS as c (c.v)}
        <button class="chtab" class:on={channel === c.v} onclick={() => (channel = c.v)} aria-pressed={channel === c.v}>
          <Icon name={c.icon} size={15} />
          <span>{c.label}</span>
        </button>
      {/each}
    </div>
  </div>

  <!-- Note -->
  <div class="fld">
    <span class="lbl">Note <span class="text-ink-ghost normal-case tracking-normal">optional</span></span>
    <input class="field no-drag" style="user-select:text;" placeholder="e.g. Bring the signed copy"
      bind:value={note} onkeydown={(e) => e.key === "Enter" && submit()} />
  </div>

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
    gap: 12px;
    padding: 13px;
    border: 0.5px solid color-mix(in oklab, var(--color-accent) 22%, var(--line));
    border-radius: var(--radius-lg);
    background: var(--violet-50);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .lbl {
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--color-ink-faint);
  }
  .date-row {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 9px 12px;
    border-radius: var(--radius-md);
    border: 0.5px solid var(--line-strong);
    background: #fff;
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .date-row:hover { border-color: color-mix(in oklab, var(--color-accent) 40%, var(--line-strong)); }
  .date-row.on { border-color: var(--color-accent); box-shadow: 0 0 0 3px color-mix(in oklab, var(--color-accent) 18%, transparent); }
  .date-val { font-size: 13px; font-weight: 600; color: var(--color-ink); }
  .date-time { margin-left: auto; font-size: 12.5px; font-weight: 600; color: var(--color-accent-strong); font-variant-numeric: tabular-nums; }
  .cal {
    margin-top: 8px;
    padding: 10px;
    border: 0.5px solid var(--line);
    border-radius: var(--radius-md);
    background: #fff;
  }
  .num {
    width: 60px;
    padding: 7px 10px;
    font-size: 12.5px;
    text-align: center;
    color: var(--color-ink);
    background: #fff;
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-md);
    outline: none;
  }
  .num:focus { border-color: var(--color-accent); box-shadow: 0 0 0 3px color-mix(in oklab, var(--color-accent) 18%, transparent); }
  .past {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    color: var(--color-warn);
    margin: -4px 0 0;
  }
  /* small 3-way tab strip (Ends) */
  .tabs3 {
    display: inline-flex;
    padding: 2px;
    gap: 2px;
    background: color-mix(in oklab, var(--color-ink) 7%, transparent);
    border-radius: var(--radius-md);
  }
  .tab3 {
    padding: 5px 11px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--color-ink-faint);
    border-radius: calc(var(--radius-md) - 3px);
    transition: all 0.12s ease;
  }
  .tab3.on {
    background: #fff;
    color: var(--color-accent-strong);
    box-shadow: 0 1px 3px rgba(28, 27, 42, 0.12);
  }
  /* notify-via: three equal, prominent tabs */
  .chtabs {
    display: flex;
    gap: 6px;
  }
  .chtab {
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 9px 6px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-ink-faint);
    background: #fff;
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-md);
    transition: all 0.14s ease;
  }
  .chtab:hover { border-color: color-mix(in oklab, var(--color-accent) 40%, var(--line-strong)); color: var(--color-ink); }
  .chtab.on {
    color: #fff;
    background: var(--color-accent);
    border-color: transparent;
    box-shadow: 0 4px 12px -5px rgba(92, 79, 214, 0.5);
  }
  .btn-sm {
    padding: 6px 12px;
    font-size: 12px;
  }
</style>
