<script lang="ts">
  import WindowFrame from "../ui/WindowFrame.svelte";
  import Icon from "../icons/Icon.svelte";
  import Markdown from "../ui/Markdown.svelte";
  import ProgressRing from "../ui/ProgressRing.svelte";
  import ActivityChart from "../ui/ActivityChart.svelte";
  import TaskEditor from "../components/TaskEditor.svelte";
  import DatePopover from "../components/DatePopover.svelte";
  import BreakSettingsPopover from "../components/BreakSettingsPopover.svelte";
  import { api } from "../api";
  import { store, go, refreshSnapshot, refreshTasks } from "../store.svelte";
  import { fmtMin, catColor } from "../format";
  import type { Task, Bar } from "../types";

  // Today's activity, shown as a glass mini-chart under the day summary. Kept
  // live by refetching whenever today's tracked total ticks up.
  let dayBars = $state<Bar[]>([]);
  async function loadBars() {
    try {
      const d = await api.dashboard("day", 0);
      dayBars = d.bars;
    } catch { /* dev / backend unavailable */ }
  }
  let lastTracked = -1;
  $effect(() => {
    const t = store.snapshot?.tracked_today_min ?? 0;
    if (t !== lastTracked) {
      lastTracked = t;
      loadBars();
    }
  });

  let pausing = $state(false);
  let pauseReason = $state("");
  let extending = $state(false);
  let showEditor = $state(false);
  let editTask = $state<Task | null>(null);
  let expanded = $state<Record<number, boolean>>({});
  let reschedFor = $state<Task | null>(null);
  let switchTo = $state<Task | null>(null);
  let switchReason = $state("");
  let switching = $state(false);
  let showBreakSettings = $state(false);
  let editStop = $state(false);
  let stopTime = $state(store.dayplan?.stop_time ?? "18:00");

  // Tabbed task lists: Today (planned) / Upcoming (future) / Done (completed).
  // A single compact row replaces the old stacked + collapsible sections.
  type TabKey = "today" | "upcoming" | "completed";
  let tab = $state<TabKey>("today");
  const tabIndex = $derived(tab === "today" ? 0 : tab === "upcoming" ? 1 : 2);

  // Live 12-hour clock shown at the bottom of the Today card; the chevron
  // collapses the card (its KPIs + chart) down to just this clock bar.
  // Default RESTING state = veiled: the frosted overlay covers ~90% of the
  // section with the time on top; tap it to reveal the KPIs + chart.
  const cardCollapsed = $derived(store.cardCollapsed);
  // Toggle the Today card's chart; the window auto-fits to the new content height.
  function setCard(collapsed: boolean) {
    store.cardCollapsed = collapsed;
  }
  function fmtClock(): string {
    // Simple 12-hour time, no seconds.
    return new Date().toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  }
  let clock = $state(fmtClock());
  $effect(() => {
    const id = setInterval(() => { clock = fmtClock(); }, 15000);
    return () => clearInterval(id);
  });

  // Auto-fit the window to the content height so there's no dead space. Re-fits
  // on any content change (card/sections expand, list changes) and each surface
  // (store.fitTick). Skipped while the editor overlay owns the window size.
  const FRAME_H = 58; // WindowFrame header height
  let contentH = $state(0);
  $effect(() => {
    const h = contentH;
    store.fitTick; // re-fit when the hub is surfaced
    if (showEditor || h <= 0) return;
    api.fitWindow(Math.round(h + FRAME_H)).catch(() => {});
  });

  function todayYmd(): string {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  }
  function fmtDate(ymd: string | null): string {
    if (!ymd) return "Someday";
    const [y, m, dd] = ymd.split("-").map(Number);
    const d = new Date(y, (m ?? 1) - 1, dd ?? 1);
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  }

  const dateLabel = new Date().toLocaleDateString(undefined, {
    weekday: "long",
    month: "long",
    day: "numeric",
  });
  function fmtStop(hhmm: string): string {
    const [h, m] = hhmm.split(":").map(Number);
    const d = new Date();
    d.setHours(h ?? 18, m ?? 0, 0, 0);
    return d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  }

  const snap = $derived(store.snapshot);
  const active = $derived(
    store.tasks.find((t) => t.status === "in_progress" || t.status === "awaiting"),
  );
  const planned = $derived(
    store.tasks.filter((t) => ["pending", "paused", "reopened"].includes(t.status)),
  );
  const completed = $derived(store.tasks.filter((t) => t.status === "completed"));

  // Day summary.
  const committed = $derived(snap?.minutes_committed ?? 0);
  const left = $derived(snap?.minutes_left_in_day ?? 0);
  const over = $derived(committed > left && left > 0);
  const trackedToday = $derived(snap?.tracked_today_min ?? 0);
  const doneCount = $derived(completed.length);
  const totalCount = $derived(store.tasks.length);
  const bufferMin = $derived(Math.max(0, left - committed));

  // Active-task tracking (live via snapshot). When the task reaches its
  // estimate it is paused `awaiting` a decision: the clock is frozen at the
  // estimate and the user extends (+15/+30) or finishes.
  const awaiting = $derived(snap?.active_awaiting ?? false);
  const estMin = $derived(snap?.active_estimate_min ?? active?.estimate_min ?? null);
  // TOTAL time tracked across ALL sessions for this task (closed segments plus
  // the currently-open one). This is what the ring + elapsed must show: pausing
  // then resuming continues from where it left off, never restarts at 0. The
  // engine pauses at the estimate on this same total, so the two stay in sync.
  const trackedMin = $derived(snap?.active_tracked_min ?? snap?.active_since_min ?? 0);
  // Elapsed shown in the ring = cumulative tracked (frozen at the estimate once
  // awaiting; the open segment was capped so the total already equals it).
  const elapsedShown = $derived(trackedMin);
  // Ring fill toward the estimate, by cumulative tracked; full once awaiting.
  const ringFrac = $derived(
    awaiting ? 1 : estMin && estMin > 0 ? Math.min(1, trackedMin / estMin) : 0,
  );

  async function refresh() {
    await Promise.all([refreshSnapshot(), refreshTasks()]);
  }
  async function openAdd() {
    editTask = null;
    showEditor = true;
    try { await api.resizeWindow("editor"); } catch { /* dev */ }
  }
  async function openEdit(t: Task) {
    editTask = t;
    showEditor = true;
    try { await api.resizeWindow("editor"); } catch { /* dev */ }
  }
  async function closeEditor() {
    showEditor = false;
    editTask = null;
    // The auto-fit effect re-runs once the editor is gone and sizes to content.
  }
  async function onEditorSaved() {
    await closeEditor();
    await refresh();
  }

  async function bringToToday(id: number) { await api.setPlanDate(id, todayYmd()); await refresh(); }
  // Starting a task while another is tracking would silently pause the running
  // one. Confirm first (and let the user note a pause reason) instead.
  async function start(t: Task) {
    if (active && active.id !== t.id) {
      switchReason = "";
      switchTo = t;
      return;
    }
    await api.startTask(t.id);
    await refresh();
  }
  async function confirmSwitch() {
    if (!switchTo) return;
    switching = true;
    try {
      // Pause the running task (carrying the reason) then start the new one.
      if (active) await api.pauseTask(active.id, switchReason.trim());
      await api.startTask(switchTo.id);
      switchTo = null;
      switchReason = "";
      await refresh();
    } finally { switching = false; }
  }
  async function complete(id: number) { await api.completeTask(id); await refresh(); }
  async function reopen(id: number) { await api.reopenTask(id); await refresh(); }
  async function confirmPause() {
    if (!active) return;
    await api.pauseTask(active.id, pauseReason.trim());
    pausing = false; pauseReason = "";
    await refresh();
  }
  async function extend(mins: number) {
    if (!active) return;
    extending = true;
    try {
      // Bumps the estimate AND resumes the clock if it was paused at estimate.
      await api.extendActive(active.id, mins);
      await refresh();
    } finally { extending = false; }
  }
  async function pickDate(date: string | null) {
    if (!reschedFor) return;
    await api.setPlanDate(reschedFor.id, date);
    reschedFor = null;
    await refresh();
  }
  async function onStop() {
    editStop = false;
    try { await api.setStopTime(stopTime); await refreshSnapshot(); } catch { /* dev */ }
  }
  function toggle(id: number) { expanded[id] = !expanded[id]; }
</script>

<div class="relative h-full">
<WindowFrame title="Achieve" subtitle={snap?.greeting ?? ""} icon="sparkles" onClose={() => api.dismiss()}>
  {#snippet actions()}
    <button class="icon-btn" title="Add task" onclick={openAdd}>
      <Icon name="plus" size={18} />
    </button>
    <button class="icon-btn" title="Break settings" onclick={() => (showBreakSettings = true)}>
      <Icon name="settings" size={17} />
    </button>
    <button class="icon-btn" title="Dashboard" onclick={() => go("dashboard")}>
      <Icon name="pie-chart" size={17} />
    </button>
  {/snippet}

  <div class="px-4 pb-5 flex flex-col gap-3.5" bind:clientHeight={contentH}>
    <!-- Today card. Veiled = compact (just date + a small clock bar, window
         shorter); revealed = full KPIs + chart (window full height). The window
         resize is animated by niri so the height change is smooth. -->
    <div class="panel rounded-[var(--radius-lg)] px-4 py-3 relative overflow-hidden">
      <!-- header always visible -->
      <div class="flex items-center justify-between">
        <div class="flex items-baseline gap-1.5 min-w-0">
          <span class="text-[9.5px] font-semibold tracking-[0.08em] uppercase text-ink-ghost shrink-0">Today</span>
          <span class="text-[12px] font-semibold text-ink truncate">{dateLabel}</span>
        </div>
        {#if editStop}
          <input type="time" class="field no-drag" style="width:118px; padding:4px 9px; user-select:text;"
            bind:value={stopTime} onchange={onStop} onblur={() => (editStop = false)} />
        {:else}
          <button class="stop-chip no-drag shrink-0" title="Change stop time" onclick={() => (editStop = true)}>
            <Icon name="sunset" size={13} /> Ends {fmtStop(stopTime)}
          </button>
        {/if}
      </div>

      <!-- KPIs: always visible -->
      <div class="flex items-stretch mt-2.5">
        {@render stat(fmtMin(trackedToday), "tracked", "var(--color-accent)")}
        <div class="vrule"></div>
        {@render stat(`${doneCount}/${totalCount}`, "done", "var(--color-positive)")}
        <div class="vrule"></div>
        {#if over}
          {@render stat(fmtMin(committed - left), "over", "var(--color-warn)")}
        {:else}
          {@render stat(fmtMin(bufferMin), "buffer", "var(--color-ink)")}
        {/if}
      </div>

      <!-- Active tracking: merged into the Today card (no shadow), a hairline
           below the KPIs, matching the card's sectioning. -->
      {#if active}
        <div class="mt-3 pt-3" style="border-top: 0.5px solid var(--line);">
          <div class="flex items-center gap-3">
            <ProgressRing value={ringFrac} size={58} stroke={5.5}
              color={awaiting ? "var(--color-warn)" : "var(--color-accent)"}>
              <span class="tabular-nums font-semibold"
                style="font-size:13px; letter-spacing:-0.5px; color: {awaiting ? 'var(--color-warn)' : 'var(--color-ink)'};">
                {fmtMin(elapsedShown)}
              </span>
            </ProgressRing>

            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-1.5 text-[9.5px] font-semibold uppercase tracking-[0.07em] leading-none">
                {#if awaiting}
                  <Icon name="timer" size={11} style="color: var(--color-warn);" />
                  <span style="color: color-mix(in oklab, var(--color-warn) 50%, black);">Estimate reached</span>
                {:else}
                  <span class="relative flex h-1.5 w-1.5">
                    <span class="animate-ping absolute inline-flex h-full w-full rounded-full opacity-70" style="background: var(--color-accent);"></span>
                    <span class="relative inline-flex rounded-full h-1.5 w-1.5" style="background: var(--color-accent);"></span>
                  </span>
                  <span style="color: var(--color-accent);">Tracking</span>
                {/if}
                {#if active.category_name}<span class="text-ink-ghost font-medium normal-case tracking-normal">· {active.category_name}</span>{/if}
              </div>
              <div class="text-[13.5px] font-semibold text-ink truncate mt-1">{active.title}</div>
              <div class="text-[11px] text-ink-faint tabular-nums mt-0.5">
                {fmtMin(elapsedShown)}{#if estMin} <span class="text-ink-ghost">of ~{fmtMin(estMin)}</span>{/if}
              </div>
            </div>
          </div>

          {#if pausing}
            <div class="flex flex-col gap-2 fade mt-2.5">
              <input class="field no-drag" placeholder="Pause reason (optional): coffee, meeting, stuck…"
                bind:value={pauseReason} onkeydown={(e) => e.key === "Enter" && confirmPause()} />
              <div class="flex gap-2">
                <button class="btn btn-soft flex-1" onclick={() => (pausing = false)}>Cancel</button>
                <button class="btn btn-primary flex-1" onclick={confirmPause}>Pause</button>
              </div>
            </div>
          {:else if awaiting}
            <div class="flex gap-2 mt-2.5">
              <button class="btn btn-soft flex-1" disabled={extending} onclick={() => extend(15)}>+15m</button>
              <button class="btn btn-soft flex-1" disabled={extending} onclick={() => extend(30)}>+30m</button>
              <button class="btn btn-positive flex-1" onclick={() => complete(active.id)}><Icon name="check" size={14} /> Done</button>
            </div>
          {:else}
            <div class="flex gap-2 mt-2.5">
              <button class="btn btn-soft flex-1" onclick={() => (pausing = true)}><Icon name="pause" size={14} /> Pause</button>
              <button class="btn btn-positive flex-1" onclick={() => complete(active.id)}><Icon name="check" size={14} /> Done</button>
            </div>
          {/if}
        </div>
      {/if}

      <!-- only the chart collapses -->
      <div class="card-body" class:closed={cardCollapsed}>
        <div class="mt-2.5 pt-2.5" style="border-top: 0.5px solid var(--line);">
          <ActivityChart bars={dayBars} period="day" tone="bare" height={82} compact />
        </div>
      </div>

      <!-- clock bar: tap to toggle the card open/closed -->
      <button class="clock-bar no-drag" class:frosted={cardCollapsed}
        title={cardCollapsed ? "Show today" : "Hide"} onclick={() => setCard(!cardCollapsed)}>
        <span class="clock-txt">{clock}</span>
        <Icon name={cardCollapsed ? "chevron-down" : "chevron-up"} size={15} class="text-ink-faint" />
      </button>
    </div>

    <!-- Tasks panel: one cohesive card (like the Today summary above). The tab
         bar is its header (underline sits on the divider); the rows are flush
         list items split by hairlines, not separate floating cards. -->
    <div class="panel list-panel rounded-[var(--radius-lg)]">
      <div class="tabs no-drag">
        <span class="tab-ind" style="transform: translateX({tabIndex * 100}%);"></span>
        <button class="tab" class:on={tab === "today"} onclick={() => (tab = "today")}>
          {active ? "Up next" : "Today"}
          {#if planned.length}<span class="tab-count">{planned.length}</span>{/if}
        </button>
        <button class="tab" class:on={tab === "upcoming"} onclick={() => (tab = "upcoming")}>
          Upcoming
          {#if store.upcoming.length}<span class="tab-count">{store.upcoming.length}</span>{/if}
        </button>
        <button class="tab" class:on={tab === "completed"} onclick={() => (tab = "completed")}>
          Done
          {#if completed.length}<span class="tab-count">{completed.length}</span>{/if}
        </button>
      </div>

      {#key tab}
      <div class="list-body fade">
        {#if tab === "today"}
          {#each planned as t (t.id)}
            {@const hasBody = !!t.body_md?.trim()}
            <div class="list-item group">
              <div class="list-row px-2.5 py-2.5 flex items-center gap-2.5">
                <button class="check no-drag shrink-0" title="Mark done" onclick={() => complete(t.id)} aria-label="Mark done">
                  <Icon name="check" size={12.5} />
                </button>

                <button class="flex-1 min-w-0 text-left" onclick={() => openEdit(t)}>
                  <div class="text-[13px] font-medium text-ink truncate flex items-center gap-1.5">
                    {t.title}
                    {#if t.recurrence}<Icon name="repeat" size={11} class="text-ink-faint" />{/if}
                    {#if t.status === "paused"}<span class="pill-muted">paused</span>{/if}
                  </div>
                  {#if t.category_name || t.estimate_min || t.tracked_min > 0}
                    <div class="flex items-center gap-1.5 mt-1.5">
                      {@render catBadge(t)}
                      {@render timeBadge(t)}
                    </div>
                  {/if}
                </button>

                {#if hasBody}
                  <button class="icon-btn no-drag shrink-0" style="width:24px;height:24px;"
                    title={expanded[t.id] ? "Hide details" : "Show details"} onclick={() => toggle(t.id)}>
                    <Icon name="chevron-down" size={15} style="transition:transform .2s ease; transform: rotate({expanded[t.id] ? 180 : 0}deg);" />
                  </button>
                {/if}
                <div class="row-tools relative shrink-0 flex items-center">
                  <div class="relative">
                    <button class="icon-btn no-drag" title="Reschedule" onclick={() => (reschedFor = reschedFor?.id === t.id ? null : t)}>
                      <Icon name="calendar-clock" size={16} />
                    </button>
                    {#if reschedFor?.id === t.id}
                      <DatePopover current={t.plan_date} onPick={pickDate} onClose={() => (reschedFor = null)} />
                    {/if}
                  </div>
                  <button class="icon-btn no-drag" title="Edit" onclick={() => openEdit(t)}>
                    <Icon name="pencil" size={15} />
                  </button>
                </div>
                <button class="btn btn-primary no-drag shrink-0" style="padding:6px 11px;" onclick={() => start(t)}>
                  <Icon name="play" size={13} fill /> Start
                </button>
              </div>
              {#if hasBody && expanded[t.id]}
                <div class="px-3 pb-3 pl-[42px] fade">
                  <div class="pt-1.5" style="border-top: 0.5px solid var(--line);">
                    <Markdown source={t.body_md} />
                  </div>
                </div>
              {/if}
            </div>
          {:else}
            <div class="empty flex flex-col items-center gap-2.5">
              <span>{active ? "Nothing else queued." : "All clear. Add the first thing to get done."}</span>
              <button class="btn btn-soft no-drag" onclick={openAdd}><Icon name="plus" size={15} /> Add a task</button>
            </div>
          {/each}

        {:else if tab === "upcoming"}
          {#each store.upcoming as t (t.id)}
            <div class="list-item group">
              <div class="list-row px-2.5 py-2.5 flex items-center gap-2.5">
                <button class="flex-1 min-w-0 text-left" onclick={() => openEdit(t)}>
                  <div class="text-[13px] font-medium text-ink truncate flex items-center gap-1.5">
                    {t.title}
                    {#if t.recurrence}<Icon name="repeat" size={11} class="text-ink-faint" />{/if}
                  </div>
                  <div class="flex items-center gap-1.5 mt-1.5">
                    <span class="badge date">
                      <Icon name="calendar" size={11} /> {fmtDate(t.plan_date)}
                    </span>
                    {@render catBadge(t)}
                    {@render timeBadge(t)}
                  </div>
                </button>
                <div class="row-tools relative shrink-0 flex items-center">
                  <button class="icon-btn no-drag" title="Reschedule" onclick={() => (reschedFor = reschedFor?.id === t.id ? null : t)}>
                    <Icon name="calendar-clock" size={16} />
                  </button>
                  {#if reschedFor?.id === t.id}
                    <DatePopover current={t.plan_date} onPick={pickDate} onClose={() => (reschedFor = null)} />
                  {/if}
                </div>
                <button class="btn btn-soft no-drag shrink-0" style="padding:6px 11px;" onclick={() => bringToToday(t.id)}>
                  <Icon name="arrow-right" size={13} /> Today
                </button>
              </div>
            </div>
          {:else}
            <div class="empty">Nothing scheduled ahead.</div>
          {/each}

        {:else}
          {#each completed as t (t.id)}
            <div class="list-item group/done">
              <div class="list-row px-2.5 py-2 flex items-center gap-2.5 text-[12.5px]">
                <Icon name="check-circle" size={15} class="shrink-0" style="color: var(--color-positive);" />
                <span class="truncate text-ink-ghost line-through flex-1">{t.title}</span>
                {#if t.tracked_min > 0}<span class="text-[11px] text-ink-faint tabular-nums">{fmtMin(t.tracked_min)}</span>{/if}
                <button class="reopen-btn no-drag" title="Reopen to work on it again" onclick={() => reopen(t.id)}>
                  <Icon name="rotate-ccw" size={12} /> Reopen
                </button>
              </div>
            </div>
          {:else}
            <div class="empty">Nothing completed yet.</div>
          {/each}
        {/if}
      </div>
      {/key}
    </div>
  </div>
</WindowFrame>

{#if showEditor}
  <TaskEditor task={editTask} categories={store.categories} onSaved={onEditorSaved} onClose={closeEditor} />
{/if}

{#if showBreakSettings}
  <BreakSettingsPopover onClose={() => (showBreakSettings = false)} />
{/if}

{#if switchTo}
  {@const from = active}
  <button class="catch no-drag" aria-label="Cancel" onclick={() => (switchTo = null)}></button>
  <div class="switch-pop no-drag">
    <div class="flex items-center gap-2 mb-1">
      <span class="grid place-items-center shrink-0" style="width:26px;height:26px;border-radius:8px;background:color-mix(in oklab, var(--color-warn) 14%, white);">
        <Icon name="pause" size={14} style="color: var(--color-warn);" />
      </span>
      <span class="text-[13.5px] font-semibold text-ink">Switch task?</span>
    </div>
    <p class="text-[12.5px] text-ink-soft leading-snug">
      {#if from}<span class="font-medium text-ink">{from.title}</span> is tracking.{/if}
      Pause it and start <span class="font-medium text-ink">{switchTo.title}</span>?
    </p>
    <input class="field no-drag mt-2.5" placeholder="Pause reason (optional): switching focus, blocked…"
      bind:value={switchReason} onkeydown={(e) => e.key === "Enter" && confirmSwitch()} />
    <div class="flex gap-2 mt-3">
      <button class="btn btn-soft flex-1" onclick={() => (switchTo = null)}>Cancel</button>
      <button class="btn btn-primary flex-1" disabled={switching} onclick={confirmSwitch}>
        <Icon name="play" size={13} fill /> Pause & start
      </button>
    </div>
  </div>
{/if}
</div>

{#snippet stat(value: string, label: string, color: string)}
  <div class="flex-1 text-center">
    <div class="text-[19px] font-semibold tabular-nums leading-none" style="color: {color};">{value}</div>
    <div class="text-[10px] tracking-wide uppercase text-ink-ghost mt-1">{label}</div>
  </div>
{/snippet}

<!-- Category chip: the dot carries the only color; the chip itself stays neutral. -->
{#snippet catBadge(t: Task)}
  {#if t.category_name}
    <span class="badge">
      <span class="dot" style="background: {catColor(t.category_color)};"></span>
      {t.category_name}
    </span>
  {/if}
{/snippet}

<!-- Time chip: tracked vs estimate. When work is logged the chip fills like a
     progress bar (tracked / estimate); before that it's just the estimate. -->
{#snippet timeBadge(t: Task)}
  {@const est = t.estimate_min ?? 0}
  {@const tr = t.tracked_min ?? 0}
  {#if est > 0 && tr > 0}
    <span class="badge prog" style="--p: {Math.min(100, Math.round((tr / est) * 100))}%">
      {fmtMin(tr)} / {fmtMin(est)}
    </span>
  {:else if est > 0}
    <span class="badge"><Icon name="timer" size={11} class="text-ink-faint" /> ~{fmtMin(est)}</span>
  {:else if tr > 0}
    <span class="badge"><Icon name="timer" size={11} class="text-ink-faint" /> {fmtMin(tr)}</span>
  {/if}
{/snippet}

<style>
  .vrule {
    width: 0.5px;
    align-self: center;
    height: 26px;
    background: var(--line);
  }
  /* Tasks card: the tab bar + list read as one panel (like the Today card). */
  .list-panel {
    padding: 0;
    overflow: hidden;
  }
  /* Tab bar = the card header. An underline indicator rides the bottom divider
     and slides between the three equal cells. */
  .tabs {
    position: relative;
    display: flex;
    padding: 0 6px;
    border-bottom: 0.5px solid var(--line);
  }
  .tab-ind {
    position: absolute;
    bottom: -0.5px;
    left: 6px;
    height: 2px;
    width: calc((100% - 12px) / 3);
    border-radius: 2px 2px 0 0;
    background: var(--color-accent);
    transition: transform 0.24s cubic-bezier(0.4, 0, 0.2, 1);
    will-change: transform;
  }
  .tab {
    position: relative;
    z-index: 1;
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 11px 4px;
    font-size: 12px;
    font-weight: 500;
    color: var(--color-ink-faint);
    transition: color 0.18s ease;
  }
  .tab:hover {
    color: var(--color-ink-soft);
  }
  .tab.on {
    color: var(--color-ink);
    font-weight: 600;
  }
  .tab-count {
    font-size: 10px;
    font-weight: 600;
    line-height: 1;
    padding: 2px 5px;
    border-radius: 999px;
    color: var(--color-ink-faint);
    background: color-mix(in oklab, var(--color-ink) 9%, transparent);
    font-variant-numeric: tabular-nums;
    transition: all 0.18s ease;
  }
  .tab.on .tab-count {
    color: var(--color-accent);
    background: color-mix(in oklab, var(--color-accent) 13%, white);
  }
  /* Flush list rows inside the card, divided by hairlines. */
  .list-body {
    display: flex;
    flex-direction: column;
  }
  .list-item + .list-item {
    border-top: 0.5px solid var(--line);
  }
  .list-row {
    transition: background 0.12s ease;
  }
  .list-item:hover {
    background: color-mix(in oklab, var(--color-ink) 2.5%, transparent);
  }
  .empty {
    padding: 30px 16px;
    text-align: center;
    font-size: 12px;
    color: var(--color-ink-faint);
  }
  /* Collapsible KPI + chart body. Veiled = max-height 0 (card shrinks, window
     follows); revealed = full. Smooth height transition. */
  /* Instant collapse (no height animation) so the window doesn't resize every
     frame; niri animates the window resize for the smooth feel. */
  .card-body {
    overflow: hidden;
    max-height: 340px;
  }
  .card-body.closed {
    max-height: 0;
    opacity: 0;
  }
  /* Bottom clock bar: tap to toggle the card. Frosted hint when collapsed. */
  .clock-bar {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    margin-top: 9px;
    padding-top: 9px;
    border-top: 0.5px solid var(--line);
    cursor: pointer;
    transition: opacity 0.12s ease;
  }
  .clock-bar.frosted { margin-top: 4px; border-top: none; }
  .clock-bar:hover { opacity: 0.7; }
  .clock-txt {
    font-size: 19px;
    font-weight: 650;
    letter-spacing: -0.3px;
    color: var(--color-ink);
    font-variant-numeric: tabular-nums;
  }
  .stop-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--color-ink-soft);
    background: rgba(0, 0, 0, 0.04);
    border: 0.5px solid var(--line);
    border-radius: 999px;
    padding: 4px 10px;
    transition: background 0.12s ease;
  }
  .stop-chip:hover {
    background: rgba(0, 0, 0, 0.07);
    color: var(--color-ink);
  }
  /* Done checkbox: neutral square; the check reveals on hover, fills on click. */
  .check {
    width: 20px;
    height: 20px;
    border-radius: 7px;
    border: 1.5px solid var(--line-strong);
    display: grid;
    place-items: center;
    color: transparent;
    transition: all 0.13s ease;
  }
  .check:hover {
    border-color: var(--color-positive);
    background: color-mix(in oklab, var(--color-positive) 12%, #fff);
    color: var(--color-positive);
  }
  /* Metadata badges. */
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 10.5px;
    font-weight: 500;
    line-height: 1;
    white-space: nowrap;
    padding: 3px 8px;
    border-radius: 999px;
    color: var(--color-ink-soft);
    background: color-mix(in oklab, var(--color-ink) 5.5%, transparent);
  }
  .badge .dot {
    width: 7px;
    height: 7px;
    border-radius: 999px;
    flex-shrink: 0;
  }
  .badge.date {
    color: var(--color-ink-soft);
  }
  /* Progress badge: the background fills left-to-right to the tracked/estimate
     ratio (the --p custom property), so the chip itself is the progress bar. */
  .badge.prog {
    color: var(--color-ink);
    font-variant-numeric: tabular-nums;
    background: linear-gradient(
      to right,
      color-mix(in oklab, var(--color-accent) 22%, #fff) var(--p),
      color-mix(in oklab, var(--color-ink) 6%, #fff) var(--p)
    );
  }
  .pill-muted {
    font-size: 9.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-ink-ghost);
    background: color-mix(in oklab, var(--color-ink) 7%, transparent);
    padding: 2px 6px;
    border-radius: 999px;
  }
  /* Secondary tools (reschedule, edit): present but dim, brightening on hover
     so the row stays calm without hiding the controls. */
  .row-tools {
    opacity: 0.5;
    transition: opacity 0.12s ease;
  }
  .group:hover .row-tools,
  .row-tools:focus-within {
    opacity: 1;
  }
  .reopen-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10.5px;
    font-weight: 500;
    color: var(--color-ink-faint);
    padding: 2px 7px;
    border-radius: 999px;
    transition: all 0.12s ease;
  }
  .reopen-btn:hover {
    color: var(--color-accent);
    background: color-mix(in oklab, var(--color-accent) 12%, white);
  }
  /* Switch-task confirmation: centered card over a transparent click-catcher
     (no dark scrim), matching DatePopover. */
  .catch {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: transparent;
    cursor: default;
  }
  .switch-pop {
    position: fixed;
    z-index: 50;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    width: 300px;
    background: #fff;
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: 0 16px 40px -10px rgba(0, 0, 0, 0.38);
    padding: 14px;
    animation: fade 0.16s ease both;
  }
</style>
