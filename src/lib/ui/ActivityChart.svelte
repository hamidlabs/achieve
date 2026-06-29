<script lang="ts">
  // Overlaid focus-vs-untracked area chart. Each series plots its OWN value
  // (not a stacked total), so an all-tracked day shows the untracked line at
  // the floor. Shared by the History dashboard and the Tasks hub.
  import { fmtMin } from "../format";
  import type { Bar } from "../types";

  type Period = "day" | "week" | "month";
  let {
    bars = [],
    period = "day",
    tone = "card",
    height = 128,
    compact = false,
  }: {
    bars?: Bar[];
    period?: Period;
    tone?: "card" | "glass" | "bare";
    height?: number;
    compact?: boolean;
  } = $props();

  // "bare" = no surface of its own (for embedding inside another card, flush);
  // "card" = white raised panel; "glass" = standalone frosted panel.
  const wrapClass = $derived(
    tone === "bare"
      ? ""
      : `rounded-[var(--radius-lg)] px-4 pt-3.5 pb-2.5 ${tone === "glass" ? "chart-glass" : "panel-raised"}`,
  );

  const H = $derived(height); // plot height in px
  let chartW = $state(0);
  const LPAD = 30; // left gutter for y-axis labels
  const RPAD = 12;
  const TPAD = $derived(compact ? 8 : 14); // headroom (less when compact)

  const chart = $derived.by(() => {
    const n = bars.length;
    // Scale to the larger of the two series in any bucket (overlaid, not stacked).
    const rawMax = Math.max(1, ...bars.map((b) => Math.max(b.focus_min, b.untracked_min)));
    const step = period === "week" ? 60 : 15;
    const max = Math.max(step, Math.ceil(rawMax / step) * step);
    const innerW = Math.max(0, chartW - LPAD - RPAD);
    const xAt = (i: number) => LPAD + (n <= 1 ? innerW / 2 : (i / (n - 1)) * innerW);
    const yAt = (v: number) => TPAD + (1 - v / max) * (H - TPAD);
    const focus = bars.map((b, i) => ({ x: xAt(i), y: yAt(b.focus_min) }));
    const untr = bars.map((b, i) => ({ x: xAt(i), y: yAt(b.untracked_min) }));
    const grid = [0, 0.5, 1].map((f) => ({ y: yAt(max * f), val: Math.round(max * f) }));
    return { n, max, innerW, xAt, yAt, focus, untr, grid };
  });
  const hasData = $derived(bars.some((b) => b.focus_min + b.untracked_min > 0));

  function line(pts: { x: number; y: number }[]): string {
    return pts.map((p, i) => `${i ? "L" : "M"}${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(" ");
  }
  function area(pts: { x: number; y: number }[]): string {
    if (!pts.length) return "";
    const first = pts[0], last = pts[pts.length - 1];
    return `M${first.x.toFixed(1)} ${H} ${line(pts)} L${last.x.toFixed(1)} ${H} Z`;
  }

  let hoverIdx = $state<number | null>(null);
  function onMove(e: MouseEvent) {
    const { n, innerW } = chart;
    if (n <= 0) return;
    const x = e.offsetX - LPAD;
    hoverIdx = n <= 1 ? 0 : Math.max(0, Math.min(n - 1, Math.round((x / innerW) * (n - 1))));
  }
  // Show a value label above a point if it carries enough time to be worth it.
  function showVal(i: number): boolean {
    if (compact) return false; // sparkline: rely on hover, keep it clean
    const b = bars[i];
    if (!b) return false;
    const peak = Math.max(b.focus_min, b.untracked_min);
    if (peak <= 0) return false;
    if (period === "month") return false; // too many days; gridlines + hover instead
    if (period === "week") return true;
    return peak >= chart.max * 0.18;
  }
  function showTick(i: number): boolean {
    const n = chart.n;
    if (period === "week" || n <= 8) return true;
    const step = Math.ceil(n / 8);
    return i % step === 0 || i === n - 1;
  }
  function barLabel(raw: string): string {
    if (period !== "day") return raw;
    const h = parseInt(raw, 10);
    const ampm = h < 12 ? "a" : "p";
    const h12 = h % 12 === 0 ? 12 : h % 12;
    return `${h12}${ampm}`;
  }
</script>

<div class={wrapClass}>
  {#if !compact}
    <div class="flex items-center justify-between mb-2">
      <span class="text-[10.5px] font-semibold tracking-wide uppercase text-ink-faint">Activity</span>
      <div class="flex items-center gap-3.5 text-[10.5px] text-ink-faint">
        <span class="inline-flex items-center gap-1.5">
          <span class="w-2.5 h-2.5 rounded-[3px]" style="background: var(--color-accent);"></span> Focus
        </span>
        <span class="inline-flex items-center gap-1.5">
          <span class="w-2.5 h-2.5 rounded-[3px]" style="background: #c2c6cf;"></span> Untracked
        </span>
      </div>
    </div>
  {/if}

  {#if hasData}
    <div class="relative" bind:clientWidth={chartW}>
      <svg width="100%" height={H} class="block" role="img"
        onmousemove={onMove} onmouseleave={() => (hoverIdx = null)}>
        <defs>
          <linearGradient id="gfocus" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="var(--color-accent)" stop-opacity="0.30" />
            <stop offset="100%" stop-color="var(--color-accent)" stop-opacity="0.02" />
          </linearGradient>
          <linearGradient id="guntr" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="#c2c6cf" stop-opacity="0.45" />
            <stop offset="100%" stop-color="#c2c6cf" stop-opacity="0.04" />
          </linearGradient>
        </defs>

        <!-- y gridlines + labels -->
        {#each chart.grid as g (g.val)}
          <line x1={LPAD} x2={chartW - RPAD} y1={g.y} y2={g.y}
            stroke="var(--color-ink)" stroke-opacity="0.07" stroke-width="1" />
          <text x={LPAD - 6} y={g.y + 3} text-anchor="end"
            font-size="8.5" fill="var(--color-ink-ghost)">{g.val ? fmtMin(g.val) : "0"}</text>
        {/each}

        <!-- overlaid: untracked behind, focus in front; each plots its own value -->
        <path d={area(chart.untr)} fill="url(#guntr)" />
        <path d={area(chart.focus)} fill="url(#gfocus)" />
        <path d={line(chart.untr)} fill="none" stroke="#aab0ba" stroke-width="1.5" />
        <path d={line(chart.focus)} fill="none" stroke="var(--color-accent)" stroke-width="2" />

        <!-- always-on value label at each notable bucket's peak line -->
        {#each bars as b, i (b.label)}
          {#if showVal(i)}
            {@const pk = b.focus_min >= b.untracked_min ? chart.focus[i] : chart.untr[i]}
            <text x={pk.x} y={pk.y - 6} text-anchor="middle"
              font-size="9" font-weight="600" fill="var(--color-ink-soft)">{fmtMin(Math.max(b.focus_min, b.untracked_min))}</text>
          {/if}
        {/each}

        {#if hoverIdx !== null && chart.focus[hoverIdx]}
          <line x1={chart.focus[hoverIdx].x} x2={chart.focus[hoverIdx].x} y1="0" y2={H}
            stroke="var(--color-ink)" stroke-opacity="0.12" stroke-width="1" />
          <circle cx={chart.untr[hoverIdx].x} cy={chart.untr[hoverIdx].y} r="3" fill="#aab0ba" />
          <circle cx={chart.focus[hoverIdx].x} cy={chart.focus[hoverIdx].y} r="3.5"
            fill="white" stroke="var(--color-accent)" stroke-width="2" />
        {/if}
      </svg>

      <!-- tick labels -->
      <div class="relative {compact ? 'h-3' : 'h-3.5'} mt-0.5">
        {#each bars as b, i (b.label)}
          {#if showTick(i)}
            <span class="absolute {compact ? 'text-[8px]' : 'text-[9px]'} text-ink-ghost -translate-x-1/2 whitespace-nowrap"
              style="left: {chart.xAt(i)}px;">{barLabel(b.label)}</span>
          {/if}
        {/each}
      </div>

      {#if hoverIdx !== null && bars[hoverIdx] && chart.focus[hoverIdx]}
        {@const b = bars[hoverIdx]}
        {@const anchorY = Math.min(chart.focus[hoverIdx].y, chart.untr[hoverIdx].y)}
        <!-- tooltip floats exactly 1px above the hovered point and follows it -->
        <div class="pointer-events-none absolute z-30 rounded-[var(--radius-md)] px-3 py-2.5 text-[11px] min-w-[150px] chart-tip"
          style="left: {Math.min(Math.max(chart.xAt(hoverIdx), 78), chartW - 78)}px; top: {anchorY - 1}px; transform: translate(-50%, -100%);">
          <div class="flex items-center gap-1.5 pb-1.5 mb-1.5" style="border-bottom: 1px solid rgba(255,255,255,0.16);">
            {#if b.top}
              <span class="w-2 h-2 rounded-full shrink-0" style="background: {b.top_color};"></span>
              <span class="font-semibold truncate">{b.top}</span>
            {:else}
              <span class="font-semibold opacity-70">No activity</span>
            {/if}
            <span class="ml-auto pl-2 opacity-60 shrink-0">{period === "week" ? b.label : barLabel(b.label)}</span>
          </div>
          <div class="flex items-center gap-1.5 whitespace-nowrap">
            <span class="w-2 h-2 rounded-[2px]" style="background: var(--color-accent);"></span>
            Focus <span class="ml-auto pl-3 tabular-nums font-medium">{fmtMin(b.focus_min)}</span>
          </div>
          <div class="flex items-center gap-1.5 whitespace-nowrap mt-0.5">
            <span class="w-2 h-2 rounded-[2px]" style="background: #c2c6cf;"></span>
            Untracked <span class="ml-auto pl-3 tabular-nums font-medium">{fmtMin(b.untracked_min)}</span>
          </div>
        </div>
      {/if}
    </div>
  {:else}
    <div class="grid place-items-center text-[12px] text-ink-faint" style="height: {H}px;">
      No activity tracked yet
    </div>
  {/if}
</div>

<style>
  /* Semi-transparent, frosted tooltip (asked for: see the chart faintly behind). */
  .chart-tip {
    background: color-mix(in oklab, var(--color-ink) 55%, transparent);
    color: white;
    backdrop-filter: blur(18px) saturate(1.3);
    -webkit-backdrop-filter: blur(18px) saturate(1.3);
    border: 0.5px solid rgba(255, 255, 255, 0.16);
    box-shadow: 0 8px 24px -8px rgba(0, 0, 0, 0.4);
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  }
  /* Translucent frosted card for the Tasks hub: the window's blurred backdrop
     reads through, so the chart floats on glass. */
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
