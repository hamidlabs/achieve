<script lang="ts">
  interface Option {
    value: string;
    label: string;
  }
  interface Props {
    options: Option[];
    value: string;
    onChange: (v: string) => void;
  }
  let { options, value, onChange }: Props = $props();
</script>

<div
  class="inline-flex p-0.5 rounded-[var(--radius-md)] gap-0.5"
  style="background: color-mix(in oklab, var(--color-ink) 8%, transparent);"
>
  {#each options as opt (opt.value)}
    <button
      class="px-3 py-1.5 rounded-[10px] text-[12px] font-medium transition no-drag"
      class:active={value === opt.value}
      style={value === opt.value
        ? "background: #ffffff; color: var(--color-ink); box-shadow: 0 1px 2px rgba(0,0,0,0.16); border: 0.5px solid rgba(0,0,0,0.06);"
        : "color: var(--color-ink-faint);"}
      onclick={() => onChange(opt.value)}
    >
      {opt.label}
    </button>
  {/each}
</div>
