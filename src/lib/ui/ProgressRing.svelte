<script lang="ts">
  interface Props {
    value: number; // 0..1
    size?: number;
    stroke?: number;
    color?: string;
    track?: string;
    children?: import("svelte").Snippet;
  }
  let {
    value,
    size = 56,
    stroke = 6,
    color = "var(--color-accent)",
    track = "color-mix(in oklab, var(--color-ink) 12%, transparent)",
    children,
  }: Props = $props();

  const r = $derived((size - stroke) / 2);
  const c = $derived(2 * Math.PI * r);
  const clamped = $derived(Math.max(0, Math.min(1, value)));
  const offset = $derived(c * (1 - clamped));
</script>

<div class="relative inline-grid place-items-center" style="width:{size}px;height:{size}px;">
  <svg width={size} height={size} class="-rotate-90">
    <circle cx={size / 2} cy={size / 2} {r} fill="none" stroke={track} stroke-width={stroke} />
    <circle
      cx={size / 2}
      cy={size / 2}
      {r}
      fill="none"
      stroke={color}
      stroke-width={stroke}
      stroke-linecap="round"
      stroke-dasharray={c}
      stroke-dashoffset={offset}
      style="transition: stroke-dashoffset 0.6s cubic-bezier(0.22,1,0.36,1);"
    />
  </svg>
  {#if children}
    <div class="absolute inset-0 grid place-items-center">{@render children()}</div>
  {/if}
</div>
