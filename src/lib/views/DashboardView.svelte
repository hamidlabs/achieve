<script lang="ts">
  import { onMount } from "svelte";
  import WindowFrame from "../ui/WindowFrame.svelte";
  import Icon from "../icons/Icon.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import ActivityChart from "../ui/ActivityChart.svelte";
  import Markdown from "../ui/Markdown.svelte";
  import { api } from "../api";
  import { go } from "../store.svelte";
  import { fmtMin, appName, appColor, catColor } from "../format";
  import type { Dashboard, PlannedActual } from "../types";

  // Click a task in the breakdown to read its full detail (description + apps).
  let detail = $state<PlannedActual | null>(null);

  type Period = "day" | "week" | "month";
  let d = $state<Dashboard | null>(null);
  let loading = $state(true);
  let period = $state<Period>("day");
  let offset = $state(0); // periods back from now (0 = current)
  let weekStart = $state(1); // 0=Sun..6=Sat; which day the week begins on
  const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

  function parseYmd(s: string): Date {
    const [y, m, dd] = s.split("-").map(Number);
    return new Date(y, (m ?? 1) - 1, dd ?? 1);
  }
  // The human label for the window being viewed.
  const rangeLabel = $derived.by(() => {
    if (!d) return "";
    const start = parseYmd(d.start_date);
    if (period === "month") {
      return start.toLocaleDateString(undefined, { month: "long", year: "numeric" });
    }
    if (period === "week") {
      const end = parseYmd(d.end_date);
      const a = start.toLocaleDateString(undefined, { month: "short", day: "numeric" });
      const b = end.toLocaleDateString(undefined, { month: "short", day: "numeric" });
      return `${a} – ${b}`;
    }
    if (offset === 0) return "Today";
    if (offset === 1) return "Yesterday";
    return start.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" });
  });

  async function load() {
    loading = true;
    try {
      d = await api.dashboard(period, offset);
    } catch {
      d = null;
    }
    loading = false;
  }
  onMount(async () => {
    try { weekStart = await api.weekStart(); } catch { /* dev */ }
    load();
  });
  function setPeriod(p: string) {
    if (p === period) return;
    period = p as Period;
    offset = 0;
    load();
  }
  async function onWeekStartChange() {
    try { await api.setWeekStart(weekStart); } catch { /* dev */ }
    offset = 0;
    load();
  }
  function step(delta: number) {
    const next = offset + delta;
    if (next < 0) return;
    offset = next;
    load();
  }

  const focusPct = $derived.by(() => {
    if (!d) return 0;
    const t = d.focus_min + d.distraction_min;
    return t > 0 ? Math.round((d.focus_min / t) * 100) : 0;
  });

  // ---- breakdown columns ----
  type Row = { name: string; color: string; minutes: number };
  const catRows = $derived<Row[]>(
    d ? d.by_category.map((c) => ({ name: c.name, color: catColor(c.color), minutes: c.minutes })) : [],
  );
  const appRows = $derived<Row[]>(
    d ? d.by_app.map((a) => ({ name: appName(a.app), color: appColor(a.app), minutes: a.minutes })) : [],
  );
  const tasks = $derived(d ? d.planned_actual.filter((p) => p.tracked_min > 0) : []);
  const taskTotal = $derived(Math.max(1, tasks.reduce((s, p) => s + p.tracked_min, 0)));

</script>

<WindowFrame title="History" subtitle={rangeLabel} icon="pie-chart" onClose={() => api.dismiss()}>
  {#snippet actions()}
    <Segmented
      options={[
        { value: "day", label: "Day" },
        { value: "week", label: "Week" },
        { value: "month", label: "Month" },
      ]}
      value={period}
      onChange={setPeriod}
    />
    <button class="icon-btn" title="Back to tasks" onclick={() => go("nudge")}>
      <Icon name="list-checks" size={17} />
    </button>
  {/snippet}

  {#if loading}
    <div class="grid place-items-center h-full text-ink-faint text-[13px]">Crunching the numbers…</div>
  {:else if d}
    <div class="px-5 pb-5 flex flex-col gap-3.5">
      <!-- History navigation -->
      <div class="flex items-center justify-between -mb-0.5">
        <div class="flex items-center gap-1">
          <button class="nav-btn" title="Previous {period}" onclick={() => step(1)}>
            <Icon name="chevron-right" size={16} class="rotate-180" />
          </button>
          <button class="nav-btn" title="Next {period}" disabled={offset === 0} onclick={() => step(-1)}>
            <Icon name="chevron-right" size={16} />
          </button>
          <span class="text-[13px] font-semibold text-ink ml-1.5">{rangeLabel}</span>
        </div>
        <div class="flex items-center gap-2.5">
          {#if period === "week"}
            <select class="wk-select no-drag" bind:value={weekStart} onchange={onWeekStartChange}
              title="Which day the week starts on">
              {#each WEEKDAYS as name, i (i)}
                <option value={i}>Starts {name}</option>
              {/each}
            </select>
          {/if}
          {#if offset !== 0}
            <button class="text-[11.5px] font-medium no-drag hover:underline"
              style="color: var(--color-accent);" onclick={() => { offset = 0; load(); }}>
              Jump to {period === "month" ? "this month" : period === "week" ? "this week" : "today"}
            </button>
          {/if}
        </div>
      </div>

      <!-- Stat tiles -->
      <div class="grid grid-cols-4 gap-3">
        {@render tile("Tracked", fmtMin(d.total_tracked_min), "var(--color-accent)")}
        {@render tile("Focus", `${focusPct}%`, "var(--color-positive)")}
        {@render tile("Untracked", fmtMin(d.distraction_min), "var(--color-warn)")}
        {@render tile("Completed", `${d.completed}/${d.total_tasks}`, "var(--color-accent-2)")}
      </div>

      <!-- Hero activity chart: real day timeline, bucketed for week/month -->
      <ActivityChart bars={d.bars} timeline={d.timeline} dayEndMin={d.day_end_min} {period} />

      <!-- Breakdown columns -->
      <div class="grid grid-cols-3 gap-3">
        {@render column("Categories", "layout-dashboard", catRows)}
        {@render column("Applications", "layout-dashboard", appRows)}

        <!-- Tasks: click a row to read its description + where the time went -->
        <div class="panel rounded-[var(--radius-lg)] p-3.5 min-h-[176px]">
          <div class="text-[10.5px] font-semibold tracking-wide uppercase text-ink-faint mb-2.5 flex items-center gap-1.5">
            <Icon name="list-checks" size={12} /> Tasks
          </div>
          {#if tasks.length}
            <div class="flex flex-col gap-0.5">
              {#each tasks.slice(0, 6) as p (p.title)}
                <button class="task-row no-drag flex items-center gap-2 text-[12px] text-left"
                  onclick={() => (detail = p)} title={p.untracked ? "" : "View details"}>
                  <span class="tabular-nums text-ink-faint w-7 text-right">
                    {Math.round((p.tracked_min / taskTotal) * 100)}%
                  </span>
                  <div class="w-12 h-1.5 rounded-full overflow-hidden shrink-0"
                    style="background: color-mix(in oklab, var(--color-ink) 8%, transparent);">
                    <div class="h-full rounded-full"
                      style="width: {(p.tracked_min / taskTotal) * 100}%; background: {p.untracked ? '#c2c6cf' : catColor(p.color)};"></div>
                  </div>
                  <span class="flex-1 min-w-0 truncate {p.untracked ? 'text-ink-faint italic' : 'text-ink-soft'}">{p.title}</span>
                  {#if !p.untracked && p.body_md.trim()}
                    <Icon name="book-open" size={11} class="text-ink-ghost shrink-0" />
                  {/if}
                  <span class="tabular-nums text-ink-faint shrink-0">{fmtMin(p.tracked_min)}</span>
                </button>
              {/each}
            </div>
          {:else}
            <div class="text-[12px] text-ink-faint py-2">Nothing tracked yet.</div>
          {/if}
        </div>
      </div>
    </div>
  {:else}
    <div class="grid place-items-center h-full text-ink-faint text-[13px]">No data yet.</div>
  {/if}
</WindowFrame>

{#if detail}
  {@const p = detail}
  <button class="catch no-drag" aria-label="Close" onclick={() => (detail = null)}></button>
  <div class="detail no-drag">
    <div class="flex items-start gap-2.5 mb-3">
      <span class="w-2.5 h-2.5 rounded-full shrink-0 mt-1.5" style="background: {p.untracked ? '#9aa0aa' : catColor(p.color)};"></span>
      <div class="flex-1 min-w-0">
        <div class="text-[14px] font-semibold text-ink leading-snug">{p.title}</div>
        <div class="text-[11px] text-ink-faint mt-0.5 flex items-center gap-1.5 flex-wrap">
          {#if p.category}<span>{p.category}</span><span class="text-ink-ghost">·</span>{/if}
          <span class="tabular-nums">{fmtMin(p.tracked_min)} tracked{#if p.estimate_min} of ~{fmtMin(p.estimate_min)}{/if}</span>
          {#if p.done}<span class="done-chip"><Icon name="check" size={10} /> Done</span>{/if}
        </div>
      </div>
      <button class="icon-btn shrink-0" aria-label="Close" onclick={() => (detail = null)}><Icon name="x" size={16} /></button>
    </div>

    <div class="detail-body">
      {#if p.untracked}
        <p class="text-[12.5px] text-ink-faint">Active time that wasn't tracked against any task.</p>
      {:else if p.body_md.trim()}
        <Markdown source={p.body_md} />
      {:else}
        <p class="text-[12.5px] text-ink-ghost italic">No description.</p>
      {/if}

      {#if p.apps.length}
        <div class="mt-3.5 pt-3" style="border-top: 0.5px solid var(--line);">
          <div class="text-[9.5px] uppercase tracking-wide text-ink-faint mb-2">Where the time went</div>
          <div class="flex flex-col gap-1.5">
            {#each p.apps as a (a.app)}
              <div class="flex items-center gap-1.5 text-[12px]">
                <span class="w-2 h-2 rounded-full shrink-0" style="background: {appColor(a.app)};"></span>
                <span class="text-ink-soft">{appName(a.app)}</span>
                <span class="ml-auto pl-3 tabular-nums text-ink-faint">{fmtMin(a.minutes)}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}

{#snippet tile(label: string, value: string, color: string)}
  <div class="panel-raised rounded-[var(--radius-md)] px-3.5 py-3">
    <div class="text-[9.5px] font-semibold tracking-wider uppercase text-ink-faint">{label}</div>
    <div class="text-[22px] font-semibold text-ink mt-1.5 leading-none tabular-nums" style="color: {color};">
      {value}
    </div>
  </div>
{/snippet}

{#snippet column(title: string, icon: string, rows: { name: string; color: string; minutes: number }[])}
  {@const total = Math.max(1, rows.reduce((s, r) => s + r.minutes, 0))}
  <div class="panel rounded-[var(--radius-lg)] p-3.5 min-h-[176px]">
    <div class="text-[10.5px] font-semibold tracking-wide uppercase text-ink-faint mb-2.5 flex items-center gap-1.5">
      <Icon name={icon} size={12} /> {title}
    </div>
    {#if rows.length}
      <div class="flex flex-col gap-2">
        {#each rows.slice(0, 6) as r (r.name)}
          <div class="flex items-center gap-2 text-[12px]">
            <span class="tabular-nums text-ink-faint w-7 text-right">
              {Math.round((r.minutes / total) * 100)}%
            </span>
            <div class="w-12 h-1.5 rounded-full overflow-hidden shrink-0"
              style="background: color-mix(in oklab, var(--color-ink) 8%, transparent);">
              <div class="h-full rounded-full"
                style="width: {(r.minutes / total) * 100}%; background: {r.color};"></div>
            </div>
            <span class="flex-1 min-w-0 truncate text-ink-soft">{r.name}</span>
            <span class="tabular-nums text-ink-faint shrink-0">{fmtMin(r.minutes)}</span>
          </div>
        {/each}
      </div>
    {:else}
      <div class="text-[12px] text-ink-faint py-2">Nothing yet.</div>
    {/if}
  </div>
{/snippet}

<style>
  /* Clickable task row in the breakdown column. */
  .task-row {
    width: 100%;
    padding: 4px 6px;
    margin: 0 -6px;
    border-radius: 7px;
    transition: background 0.12s ease;
  }
  .task-row:hover {
    background: color-mix(in oklab, var(--color-ink) 4%, transparent);
  }
  .done-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 9.5px;
    font-weight: 600;
    color: var(--color-positive);
    background: color-mix(in oklab, var(--color-positive) 12%, white);
    padding: 1px 6px;
    border-radius: 999px;
  }
  /* Task detail card: centered over a transparent catcher (no dark scrim). */
  .catch {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: transparent;
    cursor: default;
  }
  .detail {
    position: fixed;
    z-index: 50;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    width: 420px;
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    background: linear-gradient(170deg, var(--glass-top), var(--glass-bot));
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: 0 18px 50px -14px rgba(0, 0, 0, 0.42);
    padding: 16px;
    animation: fade 0.16s ease both;
  }
  .detail-body {
    overflow-y: auto;
    min-height: 0;
  }
  .nav-btn {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 7px;
    color: var(--color-ink-faint);
    transition: all 0.12s ease;
  }
  .nav-btn:hover:not(:disabled) {
    color: var(--color-ink);
    background: rgba(0, 0, 0, 0.06);
  }
  .nav-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .wk-select {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--color-ink-soft);
    background: rgba(0, 0, 0, 0.04);
    border: 0.5px solid var(--line);
    border-radius: 999px;
    padding: 3px 9px;
    outline: none;
    cursor: pointer;
    transition: background 0.12s ease;
  }
  .wk-select:hover {
    background: rgba(0, 0, 0, 0.07);
    color: var(--color-ink);
  }
</style>
