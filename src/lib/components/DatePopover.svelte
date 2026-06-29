<script lang="ts">
  import Icon from "../icons/Icon.svelte";

  interface Props {
    onPick: (date: string | null) => void;
    onClose: () => void;
    current?: string | null;
  }
  let { onPick, onClose, current = null }: Props = $props();

  // Local YYYY-MM-DD (never UTC, to match the backend's localtime dates).
  function ymd(d: Date): string {
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    return `${d.getFullYear()}-${m}-${day}`;
  }
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  function addDays(n: number): Date {
    const d = new Date(today);
    d.setDate(d.getDate() + n);
    return d;
  }
  // Next Saturday (this weekend); if today is Sat/Sun, the coming Saturday.
  function thisWeekend(): Date {
    const d = new Date(today);
    const delta = (6 - d.getDay() + 7) % 7 || 7;
    d.setDate(d.getDate() + delta);
    return d;
  }
  // Next Monday.
  function nextWeek(): Date {
    const d = new Date(today);
    const delta = (1 - d.getDay() + 7) % 7 || 7;
    d.setDate(d.getDate() + delta);
    return d;
  }

  const short = (d: Date) =>
    d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });

  const quick = [
    { label: "Today", icon: "target", date: () => today },
    { label: "Tomorrow", icon: "arrow-right", date: () => addDays(1) },
    { label: "This weekend", icon: "sun", date: () => thisWeekend() },
    { label: "Next week", icon: "calendar", date: () => nextWeek() },
  ];

  // Mini month calendar.
  let view = $state(new Date(today.getFullYear(), today.getMonth(), 1));
  const monthLabel = $derived(
    view.toLocaleDateString(undefined, { month: "long", year: "numeric" }),
  );
  const cells = $derived.by(() => {
    const first = new Date(view.getFullYear(), view.getMonth(), 1);
    const start = new Date(first);
    start.setDate(1 - first.getDay()); // back to the Sunday of the first row
    return Array.from({ length: 42 }, (_, i) => {
      const d = new Date(start);
      d.setDate(start.getDate() + i);
      return d;
    });
  });
  function shiftMonth(n: number) {
    view = new Date(view.getFullYear(), view.getMonth() + n, 1);
  }
  const isToday = (d: Date) => ymd(d) === ymd(today);
  const inMonth = (d: Date) => d.getMonth() === view.getMonth();
  const isCurrent = (d: Date) => current != null && ymd(d) === current;
  const W = ["S", "M", "T", "W", "T", "F", "S"];
</script>

<!-- transparent click-catcher (no dark scrim) -->
<button class="catch no-drag" aria-label="Close" onclick={onClose}></button>

<div class="pop no-drag">
  <div class="flex flex-col gap-0.5">
    {#each quick as q (q.label)}
      <button class="row" onclick={() => onPick(ymd(q.date()))}>
        <Icon name={q.icon} size={14} class="text-ink-faint" />
        <span class="flex-1 text-left">{q.label}</span>
        <span class="text-[11px] text-ink-ghost">{short(q.date())}</span>
      </button>
    {/each}
    <button class="row" onclick={() => onPick(null)}>
      <Icon name="x" size={14} class="text-ink-faint" />
      <span class="flex-1 text-left">No date</span>
    </button>
  </div>

  <div class="my-2 h-px" style="background: var(--line);"></div>

  <div class="px-1">
    <div class="flex items-center justify-between mb-1.5">
      <span class="text-[12px] font-medium text-ink">{monthLabel}</span>
      <div class="flex gap-0.5">
        <button class="nav" title="Previous month" onclick={() => shiftMonth(-1)}>
          <Icon name="chevron-right" size={15} class="rotate-180" />
        </button>
        <button class="nav" title="Next month" onclick={() => shiftMonth(1)}>
          <Icon name="chevron-right" size={15} />
        </button>
      </div>
    </div>
    <div class="grid grid-cols-7 gap-0.5 mb-0.5">
      {#each W as w, i (i)}
        <div class="text-[9.5px] text-ink-ghost text-center font-medium">{w}</div>
      {/each}
    </div>
    <div class="grid grid-cols-7 gap-0.5">
      {#each cells as d (d.getTime())}
        <button
          class="day"
          class:dim={!inMonth(d)}
          class:today={isToday(d)}
          class:sel={isCurrent(d)}
          onclick={() => onPick(ymd(d))}
        >
          {d.getDate()}
        </button>
      {/each}
    </div>
  </div>
</div>

<style>
  .catch {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: transparent;
    cursor: default;
  }
  .pop {
    position: fixed;
    z-index: 50;
    width: 244px;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    background: #fff;
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: 0 16px 40px -10px rgba(0, 0, 0, 0.38);
    padding: 8px;
    animation: fade 0.16s ease both;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    font-size: 12.5px;
    color: var(--color-ink);
    transition: background 0.1s ease;
  }
  .row:hover {
    background: rgba(0, 0, 0, 0.05);
  }
  .nav {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    color: var(--color-ink-faint);
  }
  .nav:hover {
    background: rgba(0, 0, 0, 0.06);
    color: var(--color-ink);
  }
  .day {
    aspect-ratio: 1;
    display: grid;
    place-items: center;
    font-size: 11.5px;
    color: var(--color-ink-soft);
    border-radius: 6px;
    transition: background 0.1s ease;
  }
  .day:hover {
    background: color-mix(in oklab, var(--color-accent) 14%, white);
  }
  .day.dim {
    color: var(--color-ink-ghost);
  }
  .day.today {
    font-weight: 700;
    color: var(--color-accent);
  }
  .day.sel {
    background: var(--color-accent);
    color: #fff;
  }
</style>
