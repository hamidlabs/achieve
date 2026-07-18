<script lang="ts">
  // Activity chart.
  //  - DAY: a line/area chart on a 12am->stop clock axis. Each tracked session
  //    rises at its exact start, holds for its duration, and falls at its exact
  //    end (focus = teal, untracked = grey). Mouse-wheel zooms the time axis
  //    (centered on the cursor) and drag pans, so short sessions can be zoomed
  //    in to read; double-click resets.
  //  - WEEK / MONTH: a stacked bar per day.
  import { fmtMin } from "../format";
  import type { Bar, TimelineSpan } from "../types";

  type Period = "day" | "week" | "month";
  let {
    bars = [],
    timeline = [],
    dayEndMin = 0,
    period = "day",
    tone = "card",
    height = 128,
    compact = false,
  }: {
    bars?: Bar[];
    timeline?: TimelineSpan[];
    dayEndMin?: number;
    period?: Period;
    tone?: "card" | "glass" | "bare";
    height?: number;
    compact?: boolean;
  } = $props();

  // Three presence buckets, each a clearly distinct hue:
  //   Focus = teal accent, Untracked (distraction) = amber, Away (idle) = grey.
  const FOCUS = "var(--color-accent)";
  const UNTR = "#e6a23c";
  const AWAY = "#9aa0aa";
  const spanFill = (kind: string) =>
    kind === "focus" ? "url(#gfocus)" : kind === "away" ? "url(#gaway)" : "url(#guntr)";
  const spanStroke = (kind: string) =>
    kind === "focus" ? "var(--color-accent)" : kind === "away" ? AWAY : "#d18e28";
  const spanLabelColor = (kind: string) =>
    kind === "focus" ? "var(--color-accent)" : kind === "away" ? AWAY : "#c2831f";

  const wrapClass = $derived(
    tone === "bare"
      ? ""
      : `rounded-[var(--radius-lg)] px-4 pt-3.5 pb-2.5 ${tone === "glass" ? "chart-glass" : "panel-raised"}`,
  );

  const H = $derived(height);
  let chartW = $state(0);
  const LPAD = 30;
  const RPAD = 12;
  const TPAD = $derived(compact ? 8 : 16);
  // Away has no per-instant "amount", so it draws as a slim grey rail sitting
  // right AT the baseline (RAIL_GAP = 0), marking "not at the machine here". The
  // humps rest on BASE and the rail hangs just below it, so at every away↔active
  // boundary the rail meets the hump base on the baseline with no floating gap,
  // yet the two never share area (rail below the line, humps above it).
  const RAIL_H = $derived(compact ? 4 : 7);
  const RAIL_GAP = $derived(0);
  const BASE = $derived(H - RAIL_H - RAIL_GAP);

  const useTimeline = $derived(period === "day" && timeline.length > 0 && dayEndMin > 0);

  function clk(min: number): string {
    const m = Math.round(min);
    const h = Math.floor(m / 60) % 24;
    const mm = ((m % 60) + 60) % 60;
    const ap = h < 12 ? "a" : "p";
    const h12 = h % 12 === 0 ? 12 : h % 12;
    return mm === 0 ? `${h12}${ap}` : `${h12}:${String(mm).padStart(2, "0")}${ap}`;
  }

  // ---- DAY zoom/pan state. view=null means "full day". ----
  const FULL = $derived(Math.max(60, dayEndMin));
  const MIN_SPAN = 10; // closest zoom: a 10-minute window
  let view = $state<{ min: number; max: number } | null>(null);
  // Reset the zoom whenever the day/extent changes.
  let lastFull = -1;
  $effect(() => {
    if (FULL !== lastFull) { lastFull = FULL; view = null; }
  });
  const vMin = $derived(view ? view.min : 0);
  const vMax = $derived(view ? view.max : FULL);
  const zoomed = $derived(view !== null);

  const tl = $derived.by(() => {
    const span = Math.max(1, vMax - vMin);
    const innerW = Math.max(0, chartW - LPAD - RPAD);
    const plotH = BASE - TPAD;
    const xAt = (t: number) => LPAD + ((t - vMin) / span) * innerW;
    const visible = timeline.filter((s) => s.end_min > vMin && s.start_min < vMax);
    // Away (idle / suspend / machine-off) has no "amount" per instant: it is
    // context, not a measured quantity. So the y-scale is driven only by Focus
    // and Untracked durations, and away renders as a full-height background band
    // that simply marks "you weren't at the machine here" — filling every gap.
    const measured = visible.filter((s) => s.kind !== "away");
    const rawMax = Math.max(1, ...measured.map((s) => s.end_min - s.start_min));
    const step = 15;
    const yMax = Math.max(step, Math.ceil(rawMax / step) * step);
    const yAt = (v: number) => TPAD + (1 - v / yMax) * plotH;
    const items = visible.map((s, i) => {
      const x0 = Math.max(LPAD, xAt(s.start_min));
      const x1 = Math.min(chartW - RPAD, xAt(s.end_min));
      const dur = s.end_min - s.start_min;
      const isAway = s.kind === "away";
      return { ...s, i, dur, isAway, x0, x1: Math.max(x0 + 1.5, x1), w: Math.max(1.5, x1 - x0), cx: (x0 + x1) / 2, y: isAway ? H - RAIL_H : yAt(dur) };
    });
    const grid = [0, 0.5, 1].map((f) => ({ y: yAt(yMax * f), val: Math.round(yMax * f) }));
    // session boundary clock labels within the view, thinned
    const rawPts = visible
      .flatMap((s) => [
        { x: xAt(s.start_min), min: s.start_min },
        { x: xAt(s.end_min), min: s.end_min },
      ])
      .filter((p) => p.x >= LPAD - 1 && p.x <= chartW - RPAD + 1)
      .sort((a, b) => a.x - b.x);
    const minGap = compact ? 1e9 : 34;
    const ticks: { x: number; min: number }[] = [];
    for (const p of rawPts) if (!ticks.length || p.x - ticks[ticks.length - 1].x >= minGap) ticks.push(p);
    return { span, innerW, yMax, xAt, yAt, items, grid, ticks };
  });

  // ---- WEEK / MONTH stacked bars ----
  const chart = $derived.by(() => {
    const n = bars.length;
    const rawMax = Math.max(1, ...bars.map((b) => b.focus_min + b.untracked_min + b.away_min));
    const step = period === "week" ? 60 : 15;
    const max = Math.max(step, Math.ceil(rawMax / step) * step);
    const innerW = Math.max(0, chartW - LPAD - RPAD);
    const plotH = H - TPAD;
    const bandW = n > 0 ? innerW / n : innerW;
    const barW = Math.max(3, Math.min(compact ? 13 : 26, bandW * (compact ? 0.72 : 0.62)));
    const cx = (i: number) => LPAD + i * bandW + bandW / 2;
    const hOf = (v: number) => (v / max) * plotH;
    const segs = bars.map((b, i) => {
      const fH = hOf(b.focus_min);
      const uH = hOf(b.untracked_min);
      const aH = hOf(b.away_min);
      const fy = H - fH;
      const uy = fy - uH;
      const ay = uy - aH; // away stacks on top
      const total = b.focus_min + b.untracked_min + b.away_min;
      // Layers bottom -> top; only non-zero ones. The topmost gets a rounded cap.
      const layers: { y: number; h: number; color: string; op: number }[] = [];
      if (b.focus_min > 0) layers.push({ y: fy, h: fH, color: FOCUS, op: 0.95 });
      if (b.untracked_min > 0) layers.push({ y: uy, h: uH, color: UNTR, op: 1 });
      if (b.away_min > 0) layers.push({ y: ay, h: aH, color: AWAY, op: 1 });
      return { i, f: b.focus_min, u: b.untracked_min, a: b.away_min, total, x: cx(i) - barW / 2, w: barW, cx: cx(i), layers, topY: total > 0 ? ay : H };
    });
    const grid = [0, 0.5, 1].map((f) => ({ y: TPAD + (1 - f) * plotH, val: Math.round(max * f) }));
    return { n, max, innerW, bandW, barW, cx, segs, grid };
  });

  const hasData = $derived(
    useTimeline
      ? timeline.length > 0
      : bars.some((b) => b.focus_min + b.untracked_min + b.away_min > 0),
  );
  const R = $derived(compact ? 1.5 : 2.5);

  // Rounded-top hump (square bottom) for a session's filled area.
  function hump(x: number, y: number, w: number, r: number): string {
    const h = BASE - y;
    if (h <= 0.4) return "";
    const rr = Math.min(r, w / 2, h);
    return `M${x.toFixed(1)} ${BASE.toFixed(1)} L${x.toFixed(1)} ${(y + rr).toFixed(1)} `
      + `Q${x.toFixed(1)} ${y.toFixed(1)} ${(x + rr).toFixed(1)} ${y.toFixed(1)} `
      + `L${(x + w - rr).toFixed(1)} ${y.toFixed(1)} `
      + `Q${(x + w).toFixed(1)} ${y.toFixed(1)} ${(x + w).toFixed(1)} ${(y + rr).toFixed(1)} `
      + `L${(x + w).toFixed(1)} ${BASE.toFixed(1)} Z`;
  }
  function humpLine(x0: number, x1: number, y: number, r: number): string {
    const rr = Math.min(r, (x1 - x0) / 2, BASE - y);
    return `M${x0.toFixed(1)} ${BASE.toFixed(1)} L${x0.toFixed(1)} ${(y + rr).toFixed(1)} `
      + `Q${x0.toFixed(1)} ${y.toFixed(1)} ${(x0 + rr).toFixed(1)} ${y.toFixed(1)} `
      + `L${(x1 - rr).toFixed(1)} ${y.toFixed(1)} `
      + `Q${x1.toFixed(1)} ${y.toFixed(1)} ${x1.toFixed(1)} ${(y + rr).toFixed(1)} L${x1.toFixed(1)} ${BASE.toFixed(1)}`;
  }
  function rTop(x: number, y: number, w: number, h: number, r: number): string {
    if (h <= 0.4) return "";
    const rr = Math.min(r, w / 2, h);
    const b = y + h;
    return `M${x.toFixed(1)} ${b.toFixed(1)} L${x.toFixed(1)} ${(y + rr).toFixed(1)} `
      + `Q${x.toFixed(1)} ${y.toFixed(1)} ${(x + rr).toFixed(1)} ${y.toFixed(1)} `
      + `L${(x + w - rr).toFixed(1)} ${y.toFixed(1)} `
      + `Q${(x + w).toFixed(1)} ${y.toFixed(1)} ${(x + w).toFixed(1)} ${(y + rr).toFixed(1)} `
      + `L${(x + w).toFixed(1)} ${b.toFixed(1)} Z`;
  }

  let hoverIdx = $state<number | null>(null);
  let dragging = $state(false);
  let dragStartX = 0;
  let dragStartView: { min: number; max: number } | null = null;

  function onWheel(e: WheelEvent) {
    if (compact || !useTimeline) return;
    e.preventDefault();
    const innerW = Math.max(1, chartW - LPAD - RPAD);
    const span = vMax - vMin;
    const frac = Math.min(1, Math.max(0, (e.offsetX - LPAD) / innerW));
    const cursorT = vMin + frac * span;
    const factor = e.deltaY > 0 ? 1.25 : 0.8;
    let newSpan = Math.min(FULL, Math.max(MIN_SPAN, span * factor));
    if (newSpan >= FULL) { view = null; return; }
    let newMin = cursorT - frac * newSpan;
    let newMax = newMin + newSpan;
    if (newMin < 0) { newMin = 0; newMax = newSpan; }
    if (newMax > FULL) { newMax = FULL; newMin = FULL - newSpan; }
    view = { min: newMin, max: newMax };
  }
  function onDown(e: MouseEvent) {
    if (compact || !useTimeline || !zoomed) return;
    dragging = true;
    dragStartX = e.offsetX;
    dragStartView = { min: vMin, max: vMax };
  }
  function onUp() { dragging = false; dragStartView = null; }

  function onMove(e: MouseEvent) {
    if (useTimeline) {
      if (dragging && dragStartView) {
        const innerW = Math.max(1, chartW - LPAD - RPAD);
        const span = dragStartView.max - dragStartView.min;
        const dt = -((e.offsetX - dragStartX) / innerW) * span;
        let nmin = dragStartView.min + dt;
        let nmax = dragStartView.max + dt;
        if (nmin < 0) { nmin = 0; nmax = span; }
        if (nmax > FULL) { nmax = FULL; nmin = FULL - span; }
        view = { min: nmin, max: nmax };
        return;
      }
      const m = vMin + ((e.offsetX - LPAD) / Math.max(1, tl.innerW)) * (vMax - vMin);
      const hit = tl.items.find((it) => m >= it.start_min - 1 && m <= it.end_min + 1);
      hoverIdx = hit ? hit.start_min : null; // key by start_min (stable across zoom)
      return;
    }
    const { n, bandW } = chart;
    if (n <= 0 || bandW <= 0) return;
    hoverIdx = Math.max(0, Math.min(n - 1, Math.floor((e.offsetX - LPAD) / bandW)));
  }
  function onLeave() { hoverIdx = null; dragging = false; }

  function showVal(i: number): boolean {
    if (compact) return false;
    const s = chart.segs[i];
    if (!s || s.total <= 0) return false;
    if (period === "month") return false;
    return true;
  }
  function showTick(i: number): boolean {
    const n = chart.n;
    if (period === "week" || n <= 8) return true;
    const step = Math.ceil(n / 8);
    return i % step === 0 || i === n - 1;
  }
</script>

<div class={wrapClass}>
  {#if !compact}
    <div class="flex items-center justify-between mb-2">
      <span class="text-[10.5px] font-semibold tracking-wide uppercase text-ink-faint">Activity</span>
      <div class="flex items-center gap-3.5 text-[10.5px] text-ink-faint">
        {#if useTimeline && zoomed}
          <button class="zoom-reset no-drag" onclick={() => (view = null)} title="Reset zoom">
            {clk(vMin)}–{clk(vMax)} · reset
          </button>
        {/if}
        <span class="inline-flex items-center gap-1.5">
          <span class="w-2.5 h-2.5 rounded-[3px]" style="background: var(--color-accent);"></span> Focus
        </span>
        <span class="inline-flex items-center gap-1.5">
          <span class="w-2.5 h-2.5 rounded-[3px]" style="background: {UNTR};"></span> Untracked
        </span>
        <span class="inline-flex items-center gap-1.5">
          <span class="w-2.5 h-2.5 rounded-[3px]" style="background: {AWAY};"></span> Away
        </span>
      </div>
    </div>
  {/if}

  {#if hasData}
    <div class="relative" bind:clientWidth={chartW}>
      <svg width="100%" height={H} class="block {useTimeline && !compact ? (dragging ? 'grabbing' : 'zoomable') : ''}" role="img"
        onmousemove={onMove} onmouseleave={onLeave} onmousedown={onDown} onmouseup={onUp}
        onwheel={onWheel} ondblclick={() => (view = null)}>
        <defs>
          <linearGradient id="gfocus" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="var(--color-accent)" stop-opacity="0.32" />
            <stop offset="100%" stop-color="var(--color-accent)" stop-opacity="0.03" />
          </linearGradient>
          <linearGradient id="guntr" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color={UNTR} stop-opacity="0.5" />
            <stop offset="100%" stop-color={UNTR} stop-opacity="0.05" />
          </linearGradient>
          <linearGradient id="gaway" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color={AWAY} stop-opacity="0.42" />
            <stop offset="100%" stop-color={AWAY} stop-opacity="0.04" />
          </linearGradient>
        </defs>

        {#if useTimeline}
          {#each tl.grid as g, gi (gi)}
            <line x1={LPAD} x2={chartW - RPAD} y1={g.y} y2={g.y}
              stroke="var(--color-ink)" stroke-opacity="0.07" stroke-width="1" />
            <text x={LPAD - 6} y={g.y + 3} text-anchor="end"
              font-size="8.5" fill="var(--color-ink-ghost)">{g.val ? fmtMin(g.val) : "0"}</text>
          {/each}
          {#each tl.items as it (it.i)}
            {#if it.isAway}
              {@const dim = hoverIdx !== null && hoverIdx !== it.start_min}
              <rect x={it.x0} y={it.y} width={it.w} height={RAIL_H}
                fill={AWAY} opacity={dim ? 0.3 : 0.6} />
            {/if}
          {/each}
          {#each tl.items as it (it.i)}
            {#if !it.isAway}
              {@const dim = hoverIdx !== null && hoverIdx !== it.start_min}
              <path d={hump(it.x0, it.y, it.w, R)} fill={spanFill(it.kind)} opacity={dim ? 0.5 : 1} />
              <path d={humpLine(it.x0, it.x1, it.y, R)} fill="none"
                stroke={spanStroke(it.kind)} stroke-width={it.kind === "focus" ? 2 : 1.5}
                stroke-linejoin="round" opacity={dim ? 0.55 : 1} />
              {#if !compact && it.w >= 26}
                <text x={it.cx} y={it.y - 5} text-anchor="middle" font-size="9" font-weight="600"
                  fill={spanLabelColor(it.kind)}>{fmtMin(it.dur)}</text>
              {/if}
            {/if}
          {/each}
        {:else}
          {#each chart.grid as g (g.val)}
            <line x1={LPAD} x2={chartW - RPAD} y1={g.y} y2={g.y}
              stroke="var(--color-ink)" stroke-opacity="0.07" stroke-width="1" />
            <text x={LPAD - 6} y={g.y + 3} text-anchor="end"
              font-size="8.5" fill="var(--color-ink-ghost)">{g.val ? fmtMin(g.val) : "0"}</text>
          {/each}
          {#if hoverIdx !== null}
            <rect x={LPAD + hoverIdx * chart.bandW} y={TPAD} width={chart.bandW} height={H - TPAD}
              fill="var(--color-ink)" fill-opacity="0.045" />
          {/if}
          {#each chart.segs as s (s.i)}
            {@const dim = hoverIdx !== null && hoverIdx !== s.i}
            {#each s.layers as L, li (li)}
              {#if li === s.layers.length - 1}
                <path d={rTop(s.x, L.y, s.w, L.h, R)} fill={L.color} opacity={dim ? 0.5 : L.op} />
              {:else}
                <rect x={s.x} y={L.y} width={s.w} height={L.h} fill={L.color} opacity={dim ? 0.5 : L.op} />
              {/if}
            {/each}
          {/each}
          {#each chart.segs as s (s.i)}
            {#if showVal(s.i)}
              <text x={s.cx} y={s.topY - 5} text-anchor="middle" font-size="9" font-weight="600"
                fill="var(--color-ink-faint)">{fmtMin(s.total)}</text>
            {/if}
          {/each}
        {/if}
      </svg>

      <div class="relative {compact ? 'h-3' : 'h-3.5'} mt-0.5">
        {#if useTimeline}
          {#each tl.ticks as t, ti (ti)}
            <span class="absolute text-[8.5px] text-ink-ghost -translate-x-1/2 whitespace-nowrap tabular-nums"
              style="left: {t.x}px;">{clk(t.min)}</span>
          {/each}
        {:else}
          {#each bars as b, i (b.label)}
            {#if showTick(i)}
              <span class="absolute {compact ? 'text-[8px]' : 'text-[9px]'} text-ink-ghost -translate-x-1/2 whitespace-nowrap"
                style="left: {chart.cx(i)}px;">{b.label}</span>
            {/if}
          {/each}
        {/if}
      </div>

      {#if useTimeline && !dragging && hoverIdx !== null && tl.items.some((x) => x.start_min === hoverIdx)}
        {@const it = tl.items.find((x) => x.start_min === hoverIdx)!}
        <div class="pointer-events-none absolute z-30 rounded-[var(--radius-md)] px-3 py-2.5 text-[11px] min-w-[150px] chart-tip"
          style="left: {Math.min(Math.max(it.cx, 84), chartW - 84)}px; top: {it.y - 6}px; transform: translate(-50%, -100%);">
          <div class="flex items-center gap-1.5 pb-1.5 mb-1.5" style="border-bottom: 1px solid rgba(255,255,255,0.16);">
            <span class="w-2 h-2 rounded-full shrink-0" style="background: {it.kind === 'focus' ? it.color : it.kind === 'away' ? AWAY : UNTR};"></span>
            <span class="font-semibold truncate">{it.label}</span>
            <span class="ml-auto pl-2 tabular-nums font-medium shrink-0">{fmtMin(it.dur)}</span>
          </div>
          <div class="tabular-nums opacity-90">{clk(it.start_min)} – {clk(it.end_min)}</div>
        </div>
      {:else if !useTimeline && hoverIdx !== null && bars[hoverIdx] && chart.segs[hoverIdx]}
        {@const b = bars[hoverIdx]}
        {@const s = chart.segs[hoverIdx]}
        <div class="pointer-events-none absolute z-30 rounded-[var(--radius-md)] px-3 py-2.5 text-[11px] min-w-[150px] chart-tip"
          style="left: {Math.min(Math.max(s.cx, 78), chartW - 78)}px; top: {s.topY - 6}px; transform: translate(-50%, -100%);">
          <div class="flex items-center gap-1.5 pb-1.5 mb-1.5" style="border-bottom: 1px solid rgba(255,255,255,0.16);">
            {#if b.top}
              <span class="w-2 h-2 rounded-full shrink-0" style="background: {b.top_color};"></span>
              <span class="font-semibold truncate">{b.top}</span>
            {:else}
              <span class="font-semibold opacity-70">No activity</span>
            {/if}
            <span class="ml-auto pl-2 opacity-60 shrink-0">{b.label}</span>
          </div>
          <div class="flex items-center gap-1.5 whitespace-nowrap">
            <span class="w-2 h-2 rounded-[2px]" style="background: var(--color-accent);"></span>
            Focus <span class="ml-auto pl-3 tabular-nums font-medium">{fmtMin(b.focus_min)}</span>
          </div>
          <div class="flex items-center gap-1.5 whitespace-nowrap mt-0.5">
            <span class="w-2 h-2 rounded-[2px]" style="background: {UNTR};"></span>
            Untracked <span class="ml-auto pl-3 tabular-nums font-medium">{fmtMin(b.untracked_min)}</span>
          </div>
          <div class="flex items-center gap-1.5 whitespace-nowrap mt-0.5">
            <span class="w-2 h-2 rounded-[2px]" style="background: {AWAY};"></span>
            Away <span class="ml-auto pl-3 tabular-nums font-medium">{fmtMin(b.away_min)}</span>
          </div>
        </div>
      {/if}
    </div>
    {#if useTimeline && !compact}
      <div class="text-[9px] text-ink-ghost text-center mt-1 select-none">scroll to zoom · drag to pan · double-click to reset</div>
    {/if}
  {:else}
    <div class="grid place-items-center text-[12px] text-ink-faint" style="height: {H}px;">
      No activity tracked yet
    </div>
  {/if}
</div>

<style>
  .zoomable { cursor: zoom-in; }
  .grabbing { cursor: grabbing; }
  .chart-tip {
    background: color-mix(in oklab, var(--color-ink) 55%, transparent);
    color: white;
    backdrop-filter: blur(18px) saturate(1.3);
    -webkit-backdrop-filter: blur(18px) saturate(1.3);
    border: 0.5px solid rgba(255, 255, 255, 0.16);
    box-shadow: 0 8px 24px -8px rgba(0, 0, 0, 0.4);
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  }
  .chart-glass {
    background: rgba(255, 255, 255, 0.46);
    backdrop-filter: blur(8px) saturate(1.15);
    -webkit-backdrop-filter: blur(8px) saturate(1.15);
    border: 0.5px solid rgba(255, 255, 255, 0.55);
    box-shadow:
      inset 0 0.5px 0 rgba(255, 255, 255, 0.6),
      0 1px 3px rgba(0, 0, 0, 0.06);
  }
</style>
