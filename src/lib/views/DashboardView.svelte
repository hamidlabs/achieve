<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../icons/Icon.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Select from "../ui/Select.svelte";
  import ActivityChart from "../ui/ActivityChart.svelte";
  import Markdown from "../ui/Markdown.svelte";
  import { api } from "../api";
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

  type Row = { name: string; color: string; minutes: number };
  const catRows = $derived<Row[]>(
    d ? d.by_category.map((c) => ({ name: c.name, color: catColor(c.color), minutes: c.minutes })) : [],
  );
  const appRows = $derived<Row[]>(
    d ? d.by_app.map((a) => ({ name: appName(a.app), color: appColor(a.app), minutes: a.minutes })) : [],
  );
  const tasks = $derived(d ? d.planned_actual.filter((p) => p.tracked_min > 0) : []);
  const taskTotal = $derived(Math.max(1, tasks.reduce((s, p) => s + p.tracked_min, 0)));

  const AUTO_PAUSE_LABELS: Record<string, string> = {
    "auto-idle": "Went idle",
    "auto-suspend": "System asleep",
    "day-rollover": "Rolled to next day",
    "reached-estimate": "Reached estimate",
    "rescheduled": "Rescheduled",
    "deleted": "Deleted",
    "break-start": "Break started",
    "break-end": "Break ended",
    "capped-suspend": "Capped after suspend",
    "capped-runaway": "Capped runaway timer",
  };
  const pauseLabel = (r: string, auto: boolean) =>
    auto ? (AUTO_PAUSE_LABELS[r] ?? r) : r;
  const pauseTotal = $derived(detail ? detail.pauses.reduce((s, p) => s + p.count, 0) : 0);
</script>

<div class="h-full flex flex-col">
  <header class="app-bar" data-tauri-drag-region>
    <span class="app-bar-badge no-drag"><Icon name="chart-column" size={18} /></span>
    <div class="flex-1 min-w-0">
      <div class="text-[16px] font-bold text-ink leading-tight">Insights</div>
      <div class="text-[11.5px] text-ink-faint truncate">{rangeLabel || "Where your time goes"}</div>
    </div>
    <button class="icon-btn no-drag shrink-0" title="Hide" aria-label="Hide" onclick={() => api.dismiss()}>
      <Icon name="chevron-down" size={18} />
    </button>
  </header>

  {#if loading}
    <div class="flex-1 grid place-items-center text-ink-faint text-[13px]">Crunching the numbers…</div>
  {:else if d}
    <div class="flex-1 min-h-0 overflow-y-auto px-4 pb-4">
      <div class="flex flex-col gap-3.5 pt-0.5">
        <!-- Period + history navigation -->
        <div class="no-drag flex items-center gap-2">
          <Segmented
            options={[
              { value: "day", label: "Day" },
              { value: "week", label: "Week" },
              { value: "month", label: "Month" },
            ]}
            value={period}
            onChange={setPeriod}
          />
          <div class="ml-auto flex items-center gap-1">
            <button class="nav-btn" title="Previous {period}" onclick={() => step(1)}>
              <Icon name="chevron-right" size={16} class="rotate-180" />
            </button>
            <button class="nav-btn" title="Next {period}" disabled={offset === 0} onclick={() => step(-1)}>
              <Icon name="chevron-right" size={16} />
            </button>
          </div>
        </div>

        {#if period === "week" || offset !== 0}
          <div class="flex items-center gap-2.5 -mt-1">
            {#if period === "week"}
              <div style="width: 172px;">
                <Select
                  value={String(weekStart)}
                  options={WEEKDAYS.map((name, i) => ({ value: String(i), label: `Starts ${name}` }))}
                  ariaLabel="Which day the week starts on"
                  compact
                  onChange={(v) => { weekStart = Number(v); onWeekStartChange(); }}
                />
              </div>
            {/if}
            {#if offset !== 0}
              <button class="jump-link no-drag" onclick={() => { offset = 0; load(); }}>
                Jump to {period === "month" ? "this month" : period === "week" ? "this week" : "today"}
              </button>
            {/if}
          </div>
        {/if}

        <!-- Hero: total tracked + focus share -->
        <div class="hero card">
          <div class="flex items-start justify-between">
            <div>
              <div class="text-[10px] font-bold tracking-[0.1em] uppercase" style="color: color-mix(in oklab, var(--color-accent) 60%, #fff);">Tracked</div>
              <div class="text-[34px] font-bold leading-none mt-1.5 text-white tabular-nums">{fmtMin(d.total_tracked_min)}</div>
            </div>
            <div class="focus-chip">
              <span class="text-[19px] font-bold tabular-nums leading-none">{focusPct}%</span>
              <span class="text-[9.5px] font-semibold uppercase tracking-wide opacity-80">focus</span>
            </div>
          </div>
          <div class="focus-track mt-3.5">
            <div class="focus-fill" style="transform: scaleX({focusPct / 100});"></div>
          </div>
          <div class="text-[11px] mt-2" style="color: rgba(255,255,255,0.72);">
            {fmtMin(d.focus_min)} on task · {fmtMin(d.distraction_min)} untracked
          </div>
        </div>

        <!-- 2×2 stat grid -->
        <div class="grid grid-cols-2 gap-3">
          {@render tile("Focus time", fmtMin(d.focus_min), "var(--color-accent)")}
          {@render tile("Untracked", fmtMin(d.distraction_min), "var(--color-warn)")}
          {@render tile("Away", fmtMin(d.away_min), "#9aa0aa")}
          {@render tile("Completed", `${d.completed}/${d.total_tasks}`, "var(--color-positive)")}
        </div>

        <!-- Activity chart -->
        <div class="card p-4">
          <div class="section-label mb-2"><Icon name="bar-chart" size={12} /> Activity</div>
          <ActivityChart bars={d.bars} timeline={d.timeline} dayEndMin={d.day_end_min} {period} />
        </div>

        <!-- Breakdown: categories, applications, tasks -->
        {@render column("Categories", "layout-dashboard", catRows)}
        {@render column("Applications", "bar-chart", appRows)}

        <div class="card p-4">
          <div class="section-label mb-2.5"><Icon name="list-checks" size={12} /> Tasks</div>
          {#if tasks.length}
            <div class="flex flex-col gap-0.5">
              {#each tasks.slice(0, 8) as p (p.title)}
                <button class="brk-row no-drag" onclick={() => (detail = p)} title={p.untracked ? "" : "View details"}>
                  <span class="tabular-nums text-ink-faint w-8 text-right text-[12px]">
                    {Math.round((p.tracked_min / taskTotal) * 100)}%
                  </span>
                  <div class="brk-bar">
                    <div class="brk-fill" style="width: {(p.tracked_min / taskTotal) * 100}%; background: {p.untracked ? '#c2c6cf' : catColor(p.color)};"></div>
                  </div>
                  <span class="flex-1 min-w-0 truncate text-[12.5px] {p.untracked ? 'text-ink-faint italic' : 'text-ink-soft'}">{p.title}</span>
                  {#if !p.untracked && p.body_md.trim()}
                    <Icon name="book-open" size={11} class="text-ink-ghost shrink-0" />
                  {/if}
                  <span class="tabular-nums text-ink-faint shrink-0 text-[12px]">{fmtMin(p.tracked_min)}</span>
                </button>
              {/each}
            </div>
          {:else}
            <div class="text-[12.5px] text-ink-faint py-2">Nothing tracked yet.</div>
          {/if}
        </div>
      </div>
    </div>
  {:else}
    <div class="flex-1 grid place-items-center text-ink-faint text-[13px]">No data yet.</div>
  {/if}
</div>

{#if detail}
  {@const p = detail}
  <button class="catch no-drag" aria-label="Close" onclick={() => (detail = null)}></button>
  <div class="detail no-drag">
    <div class="flex items-start gap-2.5 mb-3">
      <span class="w-2.5 h-2.5 rounded-full shrink-0 mt-1.5" style="background: {p.untracked ? '#9aa0aa' : catColor(p.color)};"></span>
      <div class="flex-1 min-w-0">
        <div class="text-[14.5px] font-semibold text-ink leading-snug">{p.title}</div>
        <div class="text-[11px] text-ink-faint mt-1 flex items-center gap-1.5 flex-wrap">
          {#if p.category}
            <span class="cat-badge" style="--c: {catColor(p.color)}">{p.category}</span>
          {/if}
          <span class="tabular-nums">{fmtMin(p.tracked_min)} tracked{#if p.estimate_min}{" of ~" + fmtMin(p.estimate_min)}{/if}</span>
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
          <div class="text-[9.5px] uppercase tracking-wide text-ink-faint mb-2 font-semibold">Where the time went</div>
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

      {#if p.pauses.length}
        <div class="mt-3.5 pt-3" style="border-top: 0.5px solid var(--line);">
          <div class="flex items-center gap-1.5 mb-2">
            <div class="text-[9.5px] uppercase tracking-wide text-ink-faint font-semibold">Pauses</div>
            <span class="pause-total tabular-nums">{pauseTotal}</span>
          </div>
          <div class="flex flex-col gap-1.5">
            {#each p.pauses as pz (pz.reason)}
              <div class="pause-row" class:auto={pz.auto}>
                <Icon name="pause" size={11} class="shrink-0" fill={!pz.auto} />
                <span class="pause-reason">{pauseLabel(pz.reason, pz.auto)}</span>
                {#if pz.auto}<span class="pause-tag">auto</span>{/if}
                {#if pz.count > 1}
                  <span class="ml-auto pl-3 tabular-nums pause-count">×{pz.count}</span>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}

{#snippet tile(label: string, value: string, color: string)}
  <div class="card px-4 py-3.5">
    <div class="text-[9.5px] font-bold tracking-wider uppercase text-ink-faint">{label}</div>
    <div class="text-[23px] font-bold mt-1.5 leading-none tabular-nums" style="color: {color};">
      {value}
    </div>
  </div>
{/snippet}

{#snippet column(title: string, icon: string, rows: { name: string; color: string; minutes: number }[])}
  {@const total = Math.max(1, rows.reduce((s, r) => s + r.minutes, 0))}
  <div class="card p-4">
    <div class="section-label mb-2.5"><Icon name={icon} size={12} /> {title}</div>
    {#if rows.length}
      <div class="flex flex-col gap-2.5">
        {#each rows.slice(0, 6) as r (r.name)}
          <div class="flex items-center gap-2.5 text-[12.5px]">
            <span class="tabular-nums text-ink-faint w-8 text-right">
              {Math.round((r.minutes / total) * 100)}%
            </span>
            <div class="brk-bar">
              <div class="brk-fill" style="width: {(r.minutes / total) * 100}%; background: {r.color};"></div>
            </div>
            <span class="flex-1 min-w-0 truncate text-ink-soft">{r.name}</span>
            <span class="tabular-nums text-ink-faint shrink-0">{fmtMin(r.minutes)}</span>
          </div>
        {/each}
      </div>
    {:else}
      <div class="text-[12.5px] text-ink-faint py-2">Nothing yet.</div>
    {/if}
  </div>
{/snippet}

<style>
  .section-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--color-ink-faint);
  }
  /* Hero tracked card: the one place a saturated violet fill carries the metric. */
  .hero {
    padding: 16px 18px;
    background: linear-gradient(150deg, var(--color-accent-bright), var(--color-accent-strong));
    border: none;
    box-shadow: 0 10px 28px -12px rgba(74, 62, 194, 0.55);
  }
  .focus-chip {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 8px 12px;
    border-radius: var(--radius-md);
    color: #fff;
    background: rgba(255, 255, 255, 0.16);
  }
  .focus-track {
    height: 7px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.22);
    overflow: hidden;
  }
  .focus-fill {
    width: 100%;
    height: 100%;
    border-radius: 999px;
    background: #fff;
    transform-origin: left center;
    transition: transform 0.6s cubic-bezier(0.22, 1, 0.36, 1);
  }
  /* Breakdown mini bars. */
  .brk-row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    text-align: left;
    padding: 4px 6px;
    margin: 0 -6px;
    border-radius: 8px;
    transition: background 0.12s ease;
  }
  .brk-row:hover { background: color-mix(in oklab, var(--color-accent) 5%, transparent); }
  .brk-bar {
    width: 46px;
    height: 6px;
    border-radius: 999px;
    overflow: hidden;
    flex-shrink: 0;
    background: color-mix(in oklab, var(--color-ink) 8%, transparent);
  }
  .brk-fill { height: 100%; border-radius: 999px; }
  .nav-btn {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius-md);
    color: var(--color-ink-faint);
    transition: all 0.12s ease;
  }
  .nav-btn:hover:not(:disabled) { color: var(--color-accent); background: var(--violet-50); }
  .nav-btn:disabled { opacity: 0.3; cursor: default; }
  .jump-link {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--color-accent);
    padding: 2px 2px;
  }
  .jump-link:hover { color: var(--color-accent-strong); text-decoration: underline; }

  .done-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 9.5px;
    font-weight: 700;
    color: var(--color-positive);
    background: color-mix(in oklab, var(--color-positive) 12%, white);
    padding: 1px 6px;
    border-radius: 999px;
  }
  .cat-badge {
    display: inline-flex;
    align-items: center;
    font-size: 10px;
    font-weight: 700;
    line-height: 1;
    padding: 3px 8px;
    border-radius: 999px;
    color: color-mix(in oklab, var(--c) 68%, var(--color-ink));
    background: color-mix(in oklab, var(--c) 14%, white);
    border: 0.5px solid color-mix(in oklab, var(--c) 32%, transparent);
    white-space: nowrap;
  }
  .pause-total {
    font-size: 9.5px;
    font-weight: 700;
    color: var(--color-ink-soft);
    background: color-mix(in oklab, var(--color-ink) 8%, transparent);
    padding: 0 6px;
    min-width: 18px;
    height: 15px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
  }
  .pause-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--color-ink-soft);
  }
  .pause-row.auto { color: var(--color-ink-faint); }
  .pause-reason {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pause-tag {
    font-size: 8.5px;
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--color-ink-ghost);
    border: 0.5px solid var(--line);
    border-radius: 4px;
    padding: 0 4px;
    line-height: 14px;
  }
  .pause-count { color: var(--color-ink-faint); }
  /* Task detail card: centered over a transparent catcher. */
  .catch {
    position: fixed;
    inset: 0;
    z-index: var(--z-scrim);
    background: color-mix(in oklab, #1a1830 20%, transparent);
    cursor: default;
  }
  .detail {
    position: fixed;
    z-index: var(--z-pop);
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    width: calc(100% - 28px);
    max-width: 440px;
    max-height: calc(100% - 40px);
    display: flex;
    flex-direction: column;
    background: var(--card);
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-pop);
    padding: 16px;
    animation: popCenter 0.18s cubic-bezier(0.22, 1, 0.36, 1) both;
  }
  .detail-body { overflow-y: auto; min-height: 0; }
</style>
