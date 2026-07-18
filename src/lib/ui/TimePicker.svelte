<script lang="ts">
  // On-brand time picker (replaces the native <input type=time>). Hour + minute
  // are custom selects; AM/PM is a segmented toggle. Emits 24h "HH:MM".
  import Select from "./Select.svelte";

  interface Props {
    value: string; // "HH:MM" 24-hour
    onChange: (v: string) => void;
  }
  let { value, onChange }: Props = $props();

  function parse(v: string) {
    const [h, m] = (v || "09:00").split(":").map((n) => parseInt(n, 10));
    const h24 = isNaN(h) ? 9 : h;
    const min = isNaN(m) ? 0 : m;
    return { h24, min };
  }
  const cur = $derived(parse(value));
  const ampm = $derived(cur.h24 < 12 ? "AM" : "PM");
  const h12 = $derived(cur.h24 % 12 === 0 ? 12 : cur.h24 % 12);
  const pad = (n: number) => String(n).padStart(2, "0");

  const hours = Array.from({ length: 12 }, (_, i) => {
    const v = String(i + 1);
    return { value: v, label: v };
  });
  // Every 5 minutes, plus the current minute if it's off-grid.
  const minutes = $derived.by(() => {
    const base = Array.from({ length: 12 }, (_, i) => i * 5);
    if (!base.includes(cur.min)) base.push(cur.min);
    base.sort((a, b) => a - b);
    return base.map((m) => ({ value: pad(m), label: pad(m) }));
  });

  function emit(nextH12: number, nextMin: number, nextAmpm: string) {
    let h24 = nextH12 % 12;
    if (nextAmpm === "PM") h24 += 12;
    onChange(`${pad(h24)}:${pad(nextMin)}`);
  }
</script>

<div class="tp">
  <div class="tp-field hour">
    <Select value={String(h12)} options={hours} ariaLabel="Hour" compact
      onChange={(v) => emit(parseInt(v, 10), cur.min, ampm)} />
  </div>
  <span class="tp-colon">:</span>
  <div class="tp-field min">
    <Select value={pad(cur.min)} options={minutes} ariaLabel="Minute" compact
      onChange={(v) => emit(h12, parseInt(v, 10), ampm)} />
  </div>
  <div class="tp-ampm" role="radiogroup" aria-label="AM or PM">
    <button class="tp-half" class:on={ampm === "AM"} role="radio" aria-checked={ampm === "AM"}
      onclick={() => emit(h12, cur.min, "AM")}>AM</button>
    <button class="tp-half" class:on={ampm === "PM"} role="radio" aria-checked={ampm === "PM"}
      onclick={() => emit(h12, cur.min, "PM")}>PM</button>
  </div>
</div>

<style>
  .tp {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .tp-field.hour { width: 62px; flex-shrink: 0; }
  .tp-field.min { width: 68px; flex-shrink: 0; }
  .tp-colon {
    font-size: 15px;
    font-weight: 700;
    color: var(--color-ink-faint);
    margin: 0 -2px;
  }
  .tp-ampm {
    display: inline-flex;
    margin-left: auto;
    padding: 2px;
    gap: 2px;
    background: color-mix(in oklab, var(--color-ink) 7%, transparent);
    border-radius: var(--radius-pill);
  }
  .tp-half {
    padding: 5px 13px;
    font-size: 12px;
    font-weight: 600;
    color: var(--color-ink-faint);
    border-radius: var(--radius-pill);
    transition: all 0.14s ease;
  }
  .tp-half.on {
    background: #fff;
    color: var(--color-accent-strong);
    box-shadow: 0 1px 3px rgba(28, 27, 42, 0.12);
  }
</style>
