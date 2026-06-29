<script lang="ts">
  import { onMount } from "svelte";
  import WindowFrame from "../ui/WindowFrame.svelte";
  import Icon from "../icons/Icon.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import ActivityChart from "../ui/ActivityChart.svelte";
  import { api } from "../api";
  import { go } from "../store.svelte";
  import { fmtMin, appName, appColor, catColor } from "../format";
  import type { Dashboard } from "../types";

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

      <!-- Hero activity area chart -->
      <ActivityChart bars={d.bars} {period} />

      <!-- Breakdown columns -->
      <div class="grid grid-cols-3 gap-3">
        {@render column("Categories", "layout-dashboard", catRows)}
        {@render column("Applications", "layout-dashboard", appRows)}

        <!-- Tasks: hover a row to see which apps the time went to -->
        <div class="panel rounded-[var(--radius-lg)] p-3.5 min-h-[176px]">
          <div class="text-[10.5px] font-semibold tracking-wide uppercase text-ink-faint mb-2.5 flex items-center gap-1.5">
            <Icon name="list-checks" size={12} /> Tasks
          </div>
          {#if tasks.length}
            <div class="flex flex-col gap-2">
              {#each tasks.slice(0, 6) as p (p.title)}
                <div class="relative group/task flex items-center gap-2 text-[12px]">
                  <span class="tabular-nums text-ink-faint w-7 text-right">
                    {Math.round((p.tracked_min / taskTotal) * 100)}%
                  </span>
                  <div class="w-12 h-1.5 rounded-full overflow-hidden shrink-0"
                    style="background: color-mix(in oklab, var(--color-ink) 8%, transparent);">
                    <div class="h-full rounded-full"
                      style="width: {(p.tracked_min / taskTotal) * 100}%; background: {p.untracked ? '#c2c6cf' : catColor(p.color)};"></div>
                  </div>
                  <span class="flex-1 min-w-0 truncate {p.untracked ? 'text-ink-faint italic' : 'text-ink-soft'}">{p.title}</span>
                  <span class="tabular-nums text-ink-faint shrink-0">{fmtMin(p.tracked_min)}</span>

                  {#if p.apps.length}
                    <div class="hidden group-hover/task:block absolute bottom-full left-6 mb-1 z-30 min-w-[160px]
                                rounded-[var(--radius-md)] px-2.5 py-2 shadow-lg"
                      style="background: white; border: 0.5px solid var(--line-strong);">
                      <div class="text-[9.5px] uppercase tracking-wide text-ink-faint mb-1.5">Apps used</div>
                      <div class="flex flex-col gap-1">
                        {#each p.apps as a (a.app)}
                          <div class="flex items-center gap-1.5 text-[11px] whitespace-nowrap">
                            <span class="w-2 h-2 rounded-full shrink-0" style="background: {appColor(a.app)};"></span>
                            <span class="text-ink-soft">{appName(a.app)}</span>
                            <span class="ml-auto pl-3 tabular-nums text-ink-faint">{fmtMin(a.minutes)}</span>
                          </div>
                        {/each}
                      </div>
                    </div>
                  {/if}
                </div>
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
