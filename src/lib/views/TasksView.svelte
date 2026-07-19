<script lang="ts">
  import Icon from "../icons/Icon.svelte";
  import Markdown from "../ui/Markdown.svelte";
  import ProgressRing from "../ui/ProgressRing.svelte";
  import ActivityChart from "../ui/ActivityChart.svelte";
  import Overlay from "../ui/Overlay.svelte";
  import DatePicker from "../ui/DatePicker.svelte";
  import { api } from "../api";
  import { store, refreshSnapshot, refreshTasks, openTaskEditor } from "../store.svelte";
  import { taskDone } from "../sound";
  import { assessTasks } from "../risk";
  import { fmtMin, catColor } from "../format";
  import type { Task, Bar, TimelineSpan } from "../types";

  // Today's activity, shown as a mini-chart under the day summary. Kept live by
  // refetching whenever today's tracked total ticks up.
  let dayBars = $state<Bar[]>([]);
  let dayTimeline = $state<TimelineSpan[]>([]);
  let dayEnd = $state(0);
  async function loadBars() {
    try {
      const d = await api.dashboard("day", 0);
      dayBars = d.bars;
      dayTimeline = d.timeline;
      dayEnd = d.day_end_min;
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
  let expanded = $state<Record<number, boolean>>({});
  let reschedFor = $state<Task | null>(null);
  let switchTo = $state<Task | null>(null);
  let switchReason = $state("");
  let switching = $state(false);
  let editStop = $state(false);
  let savingStop = $state(false);
  let stopTime = $state(store.dayplan?.stop_time ?? "18:00");
  // store.dayplan loads asynchronously after mount (and refreshes across days),
  // so keep the displayed stop time synced unless the user is mid-edit.
  $effect(() => {
    const s = store.dayplan?.stop_time;
    if (s && !editStop && !savingStop) stopTime = s;
  });

  // Tabbed task lists: Today (planned) / Upcoming (future) / Done (completed).
  type TabKey = "today" | "upcoming" | "completed";
  let tab = $state<TabKey>("today");
  const tabIndex = $derived(tab === "today" ? 0 : tab === "upcoming" ? 1 : 2);

  // Live 12-hour clock; the clock bar collapses the Today card's chart.
  const cardCollapsed = $derived(store.cardCollapsed);
  function setCard(collapsed: boolean) {
    store.cardCollapsed = collapsed;
  }
  function fmtClock(): string {
    return new Date().toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  }
  let clock = $state(fmtClock());
  $effect(() => {
    const id = setInterval(() => { clock = fmtClock(); }, 15000);
    return () => clearInterval(id);
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
  const awayToday = $derived(snap?.away_today_min ?? 0);
  const doneCount = $derived(completed.length);
  const totalCount = $derived(store.tasks.length);

  // Risk levels for the planned list, so tasks that need focus stand out.
  const riskLevels = $derived(
    new Map(assessTasks(store.tasks, snap).map((r) => [r.task.id, r.level])),
  );

  const awaiting = $derived(snap?.active_awaiting ?? false);
  const estMin = $derived(snap?.active_estimate_min ?? active?.estimate_min ?? null);
  const trackedMin = $derived(snap?.active_tracked_min ?? snap?.active_since_min ?? 0);
  const elapsedShown = $derived(trackedMin);
  const ringFrac = $derived(
    awaiting ? 1 : estMin && estMin > 0 ? Math.min(1, trackedMin / estMin) : 0,
  );

  async function refresh() {
    await Promise.all([refreshSnapshot(), refreshTasks()]);
  }

  async function bringToToday(id: number) { await api.setPlanDate(id, todayYmd()); await refresh(); }
  // Starting a task while another tracks would silently pause it — confirm first.
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
      if (active) await api.pauseTask(active.id, switchReason.trim());
      await api.startTask(switchTo.id);
      switchTo = null;
      switchReason = "";
      await refresh();
    } finally { switching = false; }
  }
  async function complete(id: number) { taskDone(); await api.completeTask(id); await refresh(); }
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
    if (!editStop) return;
    editStop = false;
    const val = stopTime;
    savingStop = true;
    if (store.dayplan) store.dayplan.stop_time = val;
    try {
      await api.setStopTime(val);
      await refreshSnapshot();
    } catch (e) {
      console.error("stop time save failed", e);
    } finally {
      savingStop = false;
    }
  }
  function toggle(id: number) { expanded[id] = !expanded[id]; }
</script>

<div class="h-full flex flex-col">
  <!-- Greeting app bar (also the window drag region). -->
  <header class="app-bar" data-tauri-drag-region>
    <span class="app-bar-badge no-drag"><Icon name="sparkles" size={18} /></span>
    <div class="flex-1 min-w-0">
      <div class="text-[16px] font-bold text-ink leading-tight truncate">
        {snap?.greeting ?? "Let's make today count"}
      </div>
      <div class="text-[11.5px] text-ink-faint truncate">{dateLabel}</div>
    </div>
    {#if editStop}
      <input type="time" class="field no-drag" style="width:120px; padding:6px 10px; user-select:text;"
        bind:value={stopTime} onchange={onStop} onblur={onStop} />
    {:else}
      <button class="stop-chip no-drag shrink-0" title="Change stop time" onclick={() => (editStop = true)}>
        <Icon name="sunset" size={13} /> {fmtStop(stopTime)}
      </button>
    {/if}
    <button class="icon-btn no-drag shrink-0" title="Hide" aria-label="Hide" onclick={() => api.dismiss()}>
      <Icon name="chevron-down" size={18} />
    </button>
  </header>

  <div class="flex-1 min-h-0 overflow-y-auto px-4 pb-4">
    <div class="flex flex-col gap-3.5 pt-0.5">
      <!-- Today card -->
      <div class="card px-4 py-3.5 relative overflow-hidden">
        <div class="flex items-center justify-between">
          <span class="text-[10px] font-bold tracking-[0.1em] uppercase text-ink-ghost">Today's focus</span>
          {#if !active}
            <span class="text-[11px] text-ink-faint">{planned.length} planned</span>
          {/if}
        </div>

        <!-- KPIs -->
        <div class="flex items-stretch mt-3">
          {@render stat(fmtMin(trackedToday), "tracked", "var(--color-accent)")}
          <div class="vrule"></div>
          {@render stat(fmtMin(awayToday), "away", "#9aa0aa")}
          <div class="vrule"></div>
          {@render stat(`${doneCount}/${totalCount}`, "done", "var(--color-positive)")}
          <div class="vrule"></div>
          {@render stat(fmtMin(left), "left", over ? "var(--color-warn)" : "var(--color-ink)")}
        </div>

        <!-- Active tracking -->
        {#if active}
          <div class="mt-3.5 pt-3.5" style="border-top: 0.5px solid var(--line);">
            <div class="flex items-center gap-3.5">
              <ProgressRing value={ringFrac} size={60} stroke={5.5}
                color={awaiting ? "var(--color-warn)" : "var(--color-accent)"}>
                <span class="tabular-nums font-bold"
                  style="font-size:13px; letter-spacing:-0.5px; color: {awaiting ? 'var(--color-warn)' : 'var(--color-ink)'};">
                  {fmtMin(elapsedShown)}
                </span>
              </ProgressRing>

              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-1.5 text-[9.5px] font-bold uppercase tracking-[0.07em] leading-none">
                  {#if awaiting}
                    <Icon name="timer" size={11} style="color: var(--color-warn);" />
                    <span style="color: color-mix(in oklab, var(--color-warn) 55%, black);">Estimate reached</span>
                  {:else}
                    <span class="relative flex h-1.5 w-1.5">
                      <span class="animate-ping absolute inline-flex h-full w-full rounded-full opacity-70" style="background: var(--color-accent);"></span>
                      <span class="relative inline-flex rounded-full h-1.5 w-1.5" style="background: var(--color-accent);"></span>
                    </span>
                    <span style="color: var(--color-accent-strong);">Tracking now</span>
                  {/if}
                  {#if active.category_name}<span class="text-ink-ghost font-medium normal-case tracking-normal">· {active.category_name}</span>{/if}
                </div>
                <div class="text-[14px] font-semibold text-ink truncate mt-1">{active.title}</div>
                <div class="text-[11px] text-ink-faint tabular-nums mt-0.5">
                  {fmtMin(elapsedShown)}{#if estMin} <span class="text-ink-ghost">of ~{fmtMin(estMin)}</span>{/if}
                </div>
              </div>
            </div>

            {#if pausing}
              <div class="flex flex-col gap-2 fade mt-3">
                <input class="field no-drag" placeholder="Pause reason (optional): coffee, meeting, stuck…"
                  bind:value={pauseReason} onkeydown={(e) => e.key === "Enter" && confirmPause()} />
                <div class="flex gap-2">
                  <button class="btn btn-soft flex-1" onclick={() => (pausing = false)}>Cancel</button>
                  <button class="btn btn-primary flex-1" onclick={confirmPause}>Pause</button>
                </div>
              </div>
            {:else if awaiting}
              <div class="flex gap-2 mt-3">
                <button class="btn btn-soft flex-1" disabled={extending} onclick={() => extend(15)}>+15m</button>
                <button class="btn btn-soft flex-1" disabled={extending} onclick={() => extend(30)}>+30m</button>
                <button class="btn btn-positive flex-1" onclick={() => complete(active.id)}><Icon name="check" size={14} /> Done</button>
              </div>
            {:else}
              <div class="flex gap-2 mt-3">
                <button class="btn btn-soft flex-1" onclick={() => (pausing = true)}><Icon name="pause" size={14} /> Pause</button>
                <button class="btn btn-positive flex-1" onclick={() => complete(active.id)}><Icon name="check" size={14} /> Done</button>
              </div>
            {/if}
          </div>
        {/if}

        <!-- Collapsible chart -->
        <div class="card-body" class:closed={cardCollapsed}>
          <div class="mt-3 pt-3" style="border-top: 0.5px solid var(--line);">
            <ActivityChart bars={dayBars} timeline={dayTimeline} dayEndMin={dayEnd} period="day" tone="bare" height={82} compact />
          </div>
        </div>

        <!-- Clock bar: tap to toggle the chart -->
        <button class="clock-bar no-drag" class:frosted={cardCollapsed}
          title={cardCollapsed ? "Show today's timeline" : "Hide"} onclick={() => setCard(!cardCollapsed)}>
          <span class="clock-txt">{clock}</span>
          <Icon name={cardCollapsed ? "chevron-down" : "chevron-up"} size={15} class="text-ink-faint" />
        </button>
      </div>

      <!-- Tasks card -->
      <div class="card list-panel">
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
              {@const rl = riskLevels.get(t.id)}
              <div class="list-item group">
                <div class="ptop">
                  <button class="check no-drag shrink-0" title="Mark done" onclick={() => complete(t.id)} aria-label="Mark done">
                    <Icon name="check" size={12.5} />
                  </button>

                  <button class="ptitle no-drag" onclick={() => openTaskEditor(t)}>
                    <span class="ptitle-text">{t.title}</span>
                    {#if t.recurrence}<Icon name="repeat" size={11} class="text-ink-faint shrink-0" />{/if}
                    {#if t.status === "paused"}<span class="pill-muted shrink-0">paused</span>{/if}
                  </button>

                  <div class="pactions shrink-0">
                    {#if hasBody}
                      <button class="icon-btn no-drag" style="width:28px;height:28px;"
                        title={expanded[t.id] ? "Hide details" : "Show details"} onclick={() => toggle(t.id)}>
                        <Icon name="chevron-down" size={15} style="transition:transform .2s ease; transform: rotate({expanded[t.id] ? 180 : 0}deg);" />
                      </button>
                    {/if}
                    <div class="row-tools relative flex items-center">
                      <button class="icon-btn no-drag" title="Reschedule" onclick={() => (reschedFor = reschedFor?.id === t.id ? null : t)}>
                        <Icon name="calendar-clock" size={16} />
                      </button>
                      <button class="icon-btn no-drag" title="Edit" onclick={() => openTaskEditor(t)}>
                        <Icon name="pencil" size={15} />
                      </button>
                    </div>
                    <button class="btn btn-primary no-drag" style="padding:7px 13px;" onclick={() => start(t)}>
                      <Icon name="play" size={13} fill /> Start
                    </button>
                  </div>
                </div>

                {#if rl === "urgent" || rl === "behind" || t.category_name || t.estimate_min || t.tracked_min > 0}
                  <div class="pmeta">
                    {#if rl === "urgent"}<span class="risk-tag urgent"><Icon name="flame" size={10} fill /> Urgent</span>
                    {:else if rl === "behind"}<span class="risk-tag behind"><Icon name="timer" size={10} /> Behind</span>{/if}
                    {@render catBadge(t)}
                    {@render timeBadge(t)}
                  </div>
                {/if}

                {#if hasBody && expanded[t.id]}
                  <div class="px-3 pb-3 pl-[42px] fade">
                    <div class="pt-2" style="border-top: 0.5px solid var(--line);">
                      <Markdown source={t.body_md} />
                    </div>
                  </div>
                {/if}
              </div>
            {:else}
              <div class="empty flex flex-col items-center gap-3">
                <span class="empty-orb"><Icon name="target" size={22} /></span>
                <span>{active ? "Nothing else queued." : "All clear. Add the first thing to get done."}</span>
                <button class="btn btn-soft no-drag" onclick={() => openTaskEditor(null)}><Icon name="plus" size={15} /> Add a task</button>
              </div>
            {/each}

          {:else if tab === "upcoming"}
            {#each store.upcoming as t (t.id)}
              <div class="list-item group">
                <div class="list-row px-3 py-3 flex items-center gap-3">
                  <button class="flex-1 min-w-0 text-left" onclick={() => openTaskEditor(t)}>
                    <div class="text-[13.5px] font-medium text-ink truncate flex items-center gap-1.5">
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
                  </div>
                  <button class="btn btn-soft no-drag shrink-0" style="padding:7px 12px;" onclick={() => bringToToday(t.id)}>
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
                <div class="list-row px-3 py-2.5 flex items-center gap-3 text-[12.5px]">
                  <Icon name="check-circle" size={16} class="shrink-0" style="color: var(--color-positive);" />
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

        {#if !active && tab === "today" && planned.length > 0}
          <div class="track-veil" title="Pick a task to start tracking">
            <div class="track-veil-inner">
              <span class="tv-orb"><span class="tv-ico"><Icon name="target" size={20} /></span></span>
              <div class="tv-title">You're not tracking anything</div>
              <div class="tv-sub">Hover to pick a task and start the clock</div>
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>

  {#if reschedFor}
    <Overlay variant="popover" onClose={() => (reschedFor = null)}>
      <DatePicker current={reschedFor.plan_date} onPick={pickDate} />
    </Overlay>
  {/if}

  {#if switchTo}
    {@const from = active}
    <Overlay variant="popover" maxWidth={320} pad={16} onClose={() => (switchTo = null)}>
      <div class="flex items-center gap-2.5 mb-1.5">
        <span class="grid place-items-center shrink-0" style="width:30px;height:30px;border-radius:10px;background:color-mix(in oklab, var(--color-warn) 15%, white);">
          <Icon name="pause" size={15} style="color: var(--color-warn);" />
        </span>
        <span class="text-[14px] font-semibold text-ink">Switch task?</span>
      </div>
      <p class="text-[12.5px] text-ink-soft leading-snug">
        {#if from}<span class="font-medium text-ink">{from.title}</span> is tracking.{/if}
        Pause it and start <span class="font-medium text-ink">{switchTo.title}</span>?
      </p>
      <input class="field no-drag mt-3" placeholder="Pause reason (optional): switching focus, blocked…"
        bind:value={switchReason} onkeydown={(e) => e.key === "Enter" && confirmSwitch()} />
      <div class="flex gap-2 mt-3">
        <button class="btn btn-soft flex-1" onclick={() => (switchTo = null)}>Cancel</button>
        <button class="btn btn-primary flex-1" disabled={switching} onclick={confirmSwitch}>
          <Icon name="play" size={13} fill /> Pause & start
        </button>
      </div>
    </Overlay>
  {/if}
</div>

{#snippet stat(value: string, label: string, color: string)}
  <div class="flex-1 text-center">
    <div class="text-[20px] font-bold tabular-nums leading-none" style="color: {color};">{value}</div>
    <div class="text-[9.5px] font-semibold tracking-wide uppercase text-ink-ghost mt-1.5">{label}</div>
  </div>
{/snippet}

<!-- Category chip: the dot carries the only color; the chip stays neutral. -->
{#snippet catBadge(t: Task)}
  {#if t.category_name}
    <span class="badge">
      <span class="dot" style="background: {catColor(t.category_color)};"></span>
      {t.category_name}
    </span>
  {/if}
{/snippet}

<!-- Time chip: tracked vs estimate; fills like a progress bar when work exists. -->
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
    height: 28px;
    background: var(--line);
  }
  /* Tasks card: the tab bar + list read as one panel. */
  .list-panel {
    position: relative;
    padding: 0;
    overflow: hidden;
  }
  /* Not-tracking veil: a frosted prompt over the list; hovering the card fades
     it so you can pick a task and start the clock. */
  .track-veil {
    position: absolute;
    inset: 0;
    z-index: 5;
    display: grid;
    place-items: center;
    text-align: center;
    padding: 16px;
    cursor: pointer;
    background: color-mix(in oklab, #ffffff 74%, transparent);
    backdrop-filter: blur(6px) saturate(120%);
    -webkit-backdrop-filter: blur(6px) saturate(120%);
    transition: opacity 0.3s ease;
    border-radius: var(--radius-lg);
  }
  .list-panel:hover .track-veil {
    opacity: 0;
    pointer-events: none;
  }
  .track-veil-inner {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 9px;
    animation: tvfloat 3.2s ease-in-out infinite;
  }
  .tv-orb {
    position: relative;
    display: grid;
    place-items: center;
    width: 50px;
    height: 50px;
    border-radius: 999px;
    color: var(--color-accent);
    background: var(--violet-50);
  }
  .tv-orb::before {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 999px;
    background: var(--color-accent);
    opacity: 0.28;
    animation: tvping 2.4s cubic-bezier(0, 0, 0.2, 1) infinite;
  }
  .tv-ico { position: relative; z-index: 1; }
  .tv-title {
    font-size: 14px;
    font-weight: 700;
    color: var(--color-ink);
    letter-spacing: -0.1px;
  }
  .tv-sub { font-size: 11.5px; color: var(--color-ink-faint); }
  @keyframes tvping {
    0% { transform: scale(1); opacity: 0.28; }
    70%, 100% { transform: scale(1.85); opacity: 0; }
  }
  @keyframes tvfloat {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(-3px); }
  }
  @media (prefers-reduced-motion: reduce) {
    .track-veil-inner, .tv-orb::before { animation: none; }
  }
  /* Tab bar = the card header. */
  .tabs {
    position: relative;
    display: flex;
    padding: 0 8px;
    border-bottom: 0.5px solid var(--line);
  }
  .tab-ind {
    position: absolute;
    bottom: -0.5px;
    left: 8px;
    height: 2.5px;
    width: calc((100% - 16px) / 3);
    border-radius: 3px 3px 0 0;
    background: var(--color-accent);
    transition: transform 0.26s cubic-bezier(0.22, 1, 0.36, 1);
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
    padding: 13px 4px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-ink-faint);
    transition: color 0.18s ease;
  }
  .tab:hover { color: var(--color-ink-soft); }
  .tab.on { color: var(--color-accent-strong); }
  .tab-count {
    font-size: 10px;
    font-weight: 700;
    line-height: 1;
    padding: 2px 6px;
    border-radius: 999px;
    color: var(--color-ink-faint);
    background: color-mix(in oklab, var(--color-ink) 8%, transparent);
    font-variant-numeric: tabular-nums;
    transition: all 0.18s ease;
  }
  .tab.on .tab-count {
    color: var(--color-accent);
    background: var(--violet-50);
  }
  .list-body { display: flex; flex-direction: column; }
  .list-item + .list-item { border-top: 0.5px solid var(--line); }
  .list-row { transition: background 0.12s ease; }
  .list-item:hover { background: color-mix(in oklab, var(--color-accent) 3.5%, transparent); }
  .empty {
    padding: 34px 16px;
    text-align: center;
    font-size: 12.5px;
    color: var(--color-ink-faint);
  }
  .empty-orb {
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    border-radius: 999px;
    color: var(--color-accent);
    background: var(--violet-50);
  }
  /* Collapsible KPI chart body (instant collapse; no per-frame resize). */
  .card-body { overflow: hidden; max-height: 340px; }
  .card-body.closed { max-height: 0; opacity: 0; }
  /* Bottom clock bar. */
  .clock-bar {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    margin-top: 10px;
    padding-top: 10px;
    border-top: 0.5px solid var(--line);
    cursor: pointer;
    transition: opacity 0.12s ease;
  }
  .clock-bar.frosted { margin-top: 5px; border-top: none; }
  .clock-bar:hover { opacity: 0.7; }
  .clock-txt {
    font-size: 19px;
    font-weight: 700;
    letter-spacing: -0.3px;
    color: var(--color-ink);
    font-variant-numeric: tabular-nums;
  }
  .stop-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--color-accent-strong);
    background: var(--violet-50);
    border: 0.5px solid color-mix(in oklab, var(--color-accent) 18%, transparent);
    border-radius: 999px;
    padding: 5px 11px;
    transition: background 0.12s ease;
  }
  .stop-chip:hover { background: var(--violet-100); }
  /* Done checkbox. */
  .check {
    width: 22px;
    height: 22px;
    border-radius: 8px;
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
    font-weight: 600;
    line-height: 1;
    white-space: nowrap;
    padding: 4px 9px;
    border-radius: 999px;
    color: var(--color-ink-soft);
    background: color-mix(in oklab, var(--color-ink) 5%, transparent);
  }
  .badge .dot { width: 7px; height: 7px; border-radius: 999px; flex-shrink: 0; }
  .badge.date { color: var(--color-ink-soft); }
  .badge.prog {
    color: var(--color-ink);
    font-variant-numeric: tabular-nums;
    background: linear-gradient(
      to right,
      color-mix(in oklab, var(--color-accent) 24%, #fff) var(--p),
      color-mix(in oklab, var(--color-ink) 6%, #fff) var(--p)
    );
  }
  .pill-muted {
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-ink-ghost);
    background: color-mix(in oklab, var(--color-ink) 7%, transparent);
    padding: 2px 6px;
    border-radius: 999px;
  }
  /* Planned row: two tiers so meta badges get a full line. */
  .ptop {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 11px;
  }
  .ptitle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    text-align: left;
  }
  .ptitle-text {
    font-size: 13.5px;
    font-weight: 500;
    color: var(--color-ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pactions { display: flex; align-items: center; gap: 3px; }
  .pmeta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 0 11px 11px 42px;
    margin-top: -4px;
  }
  .risk-tag {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex-shrink: 0;
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 3px 7px;
    border-radius: 999px;
  }
  .risk-tag.urgent {
    color: var(--color-danger);
    background: color-mix(in oklab, var(--color-danger) 13%, #fff);
  }
  .risk-tag.behind {
    color: color-mix(in oklab, var(--color-warn) 72%, #000);
    background: color-mix(in oklab, var(--color-warn) 16%, #fff);
  }
  /* Secondary tools: present but dim, brightening on hover. */
  .row-tools { opacity: 0.55; transition: opacity 0.12s ease; }
  .group:hover .row-tools, .row-tools:focus-within { opacity: 1; }
  .reopen-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10.5px;
    font-weight: 600;
    color: var(--color-ink-faint);
    padding: 3px 8px;
    border-radius: 999px;
    transition: all 0.12s ease;
  }
  .reopen-btn:hover {
    color: var(--color-accent);
    background: var(--violet-50);
  }
</style>
