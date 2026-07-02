<script lang="ts">
  import { fly } from "svelte/transition";
  import Icon from "../icons/Icon.svelte";
  import MarkdownEditor from "../ui/MarkdownEditor.svelte";
  import { api } from "../api";
  import CategoryPicker from "./CategoryPicker.svelte";
  import type { Category, Task } from "../types";

  interface Props {
    task: Task | null;
    categories: Category[];
    onSaved: () => void;
    onClose: () => void;
  }
  let { task, categories, onSaved, onClose }: Props = $props();

  // Editing shows every field at once (quick changes); creating is a short
  // guided wizard (Name -> Category -> Time) so it never feels like a wall of
  // fields, and the category list can't grow the form unbounded.
  const isEdit = !!task;

  let title = $state(task?.title ?? "");
  let body = $state(task?.body_md ?? "");
  let categoryId = $state<number | null>(task?.category_id ?? null);
  let estimate = $state<number | null>(task?.estimate_min ?? 30);
  let daily = $state(task?.recurrence === "daily");
  let saving = $state(false);
  let showDetails = $state(!!task?.body_md?.trim());

  const presets = [15, 30, 45, 60, 90, 120];

  // Wizard state (create only).
  const STEPS = ["Name", "Category", "Time"];
  let step = $state(0);
  let dir = $state(1);
  const canNext = $derived(step !== 0 || title.trim().length > 0);
  function next() {
    if (!canNext) return;
    if (step < STEPS.length - 1) {
      dir = 1;
      step += 1;
    } else {
      save();
    }
  }
  function back() {
    if (step > 0) {
      dir = -1;
      step -= 1;
    }
  }

  // Advance the create wizard once a category is chosen on the category step.
  function onCatPicked() {
    if (!isEdit && step === 1) next();
  }

  async function save() {
    if (!title.trim() || saving) return;
    saving = true;
    const payload = {
      category_id: categoryId,
      title: title.trim(),
      body_md: body,
      estimate_min: estimate,
      recurrence: daily ? "daily" : null,
    };
    try {
      if (task) await api.updateTask({ id: task.id, ...payload });
      else await api.createTask(payload);
      onSaved();
    } catch (e) {
      console.error("[achieve] save task failed:", e);
      alert("Could not save the task: " + e);
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (task) {
      await api.deleteTask(task.id);
      onSaved();
    }
  }

  function autofocus(node: HTMLInputElement) {
    node.focus();
  }
  function fmtPreset(p: number): string {
    return p < 60 ? `${p}m` : `${p / 60}h`.replace(".5", "½");
  }
</script>

<!-- Compact dialog floating over the dimmed task list. -->
<div class="overlay">
  <button class="scrim no-drag" aria-label="Close" onclick={onClose}></button>

  <div class="card no-drag fade">
    {#if isEdit}
      <!-- ============ EDIT: everything at once ============ -->
      <div class="head">
        <span class="head-ico"><Icon name="pencil" size={14} style="color: var(--color-accent);" /></span>
        <span class="head-title">Edit task</span>
        <button class="icon-btn ml-auto" onclick={onClose}><Icon name="x" size={16} /></button>
      </div>
      <div class="body scroll flex flex-col gap-3">
        <input class="field text-[14px] font-medium" style="user-select:text;"
          placeholder="What needs doing?" bind:value={title}
          onkeydown={(e) => e.key === "Enter" && (e.metaKey || e.ctrlKey) && save()} />
        {@render categorySection()}
        {@render timeSection()}
        {@render detailsSection()}
      </div>
      <div class="foot">
        <button class="btn btn-danger-ghost" onclick={remove}><Icon name="trash" size={15} /> Delete</button>
        <div class="flex-1"></div>
        <button class="btn btn-soft" onclick={onClose}>Cancel</button>
        <button class="btn btn-primary" onclick={save} disabled={!title.trim()}>
          <Icon name="check" size={15} /> Save
        </button>
      </div>
    {:else}
      <!-- ============ CREATE: guided wizard ============ -->
      <div class="head">
        {#if step > 0}
          <button class="icon-btn" title="Back" onclick={back}><Icon name="chevron-right" size={16} class="rotate-180" /></button>
        {:else}
          <span class="head-ico"><Icon name="plus" size={14} style="color: var(--color-accent);" /></span>
        {/if}
        <span class="head-title">New task</span>
        <span class="step-count">{step + 1} of {STEPS.length}</span>
        <button class="icon-btn" onclick={onClose}><Icon name="x" size={16} /></button>
      </div>
      <div class="stepper">
        {#each STEPS as s, i (s)}
          <span class="seg" class:on={i <= step}></span>
        {/each}
      </div>

      <div class="body wizard">
        {#key step}
          <div class="step" in:fly={{ x: dir * 18, duration: 170 }}>
            {#if step === 0}
              <div class="q">What needs doing?</div>
              <input class="field field-lg" style="user-select:text;" placeholder="e.g. Draft the proposal"
                bind:value={title} use:autofocus
                onkeydown={(e) => e.key === "Enter" && next()} />
              <p class="hint">Give it a clear, single action. Press Enter to continue.</p>
            {:else if step === 1}
              <div class="q">Choose a category</div>
              {@render categorySection()}
            {:else}
              <div class="q">How long will it take?</div>
              {@render timeSection()}
              {@render detailsSection()}
            {/if}
          </div>
        {/key}
      </div>

      <div class="foot">
        {#if step === 1}
          <button class="btn btn-ghost" onclick={() => { categoryId = null; next(); }}>Skip</button>
        {/if}
        <div class="flex-1"></div>
        <button class="btn btn-primary" onclick={next} disabled={!canNext}>
          {#if step < STEPS.length - 1}
            Next <Icon name="chevron-right" size={15} />
          {:else}
            <Icon name="check" size={15} /> Create task
          {/if}
        </button>
      </div>
    {/if}
  </div>
</div>

<!-- ===================== shared sections ===================== -->

{#snippet categorySection()}
  <CategoryPicker bind:categoryId {categories} onPicked={onCatPicked} />
{/snippet}

{#snippet timeSection()}
  <div class="flex flex-col gap-3">
    <div>
      <div class="flex flex-wrap gap-1.5 items-center">
        {#each presets as p (p)}
          <button class="chip transition" style={estimate === p ? "background: var(--color-accent); color: white;" : ""} onclick={() => (estimate = p)}>
            {fmtPreset(p)}
          </button>
        {/each}
        <input type="number" class="field no-drag" style="width:74px; padding:4px 8px; user-select:text;" placeholder="min" bind:value={estimate} />
      </div>
    </div>
    <button class="flex items-center gap-2.5 text-left" onclick={() => (daily = !daily)}>
      <span class="w-9 h-5 rounded-full transition relative shrink-0"
        style={daily ? "background: var(--color-accent);" : "background: color-mix(in oklab, var(--color-ink) 18%, transparent);"}>
        <span class="absolute top-0.5 w-4 h-4 rounded-full bg-white transition-all" style={daily ? "left: 18px;" : "left: 2px;"}></span>
      </span>
      <span class="text-[12.5px] text-ink flex items-center gap-1.5"><Icon name="repeat" size={14} /> Repeat daily</span>
    </button>
  </div>
{/snippet}

{#snippet detailsSection()}
  <div>
    {#if showDetails}
      <div class="text-[11px] font-medium text-ink-faint mb-1.5">Details</div>
      <MarkdownEditor bind:value={body} rows={3} />
    {:else}
      <button class="link-btn" onclick={() => (showDetails = true)}>
        <Icon name="plus" size={12} /> Add details
      </button>
    {/if}
  </div>
{/snippet}

<style>
  .overlay {
    position: absolute;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    padding: 14px;
  }
  .scrim {
    position: absolute;
    inset: 0;
    border-radius: var(--radius-card);
    background: rgba(17, 18, 22, 0.22);
    backdrop-filter: blur(2px);
    -webkit-backdrop-filter: blur(2px);
  }
  .card {
    position: relative;
    z-index: 1;
    width: 100%;
    max-width: 422px;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    background: linear-gradient(170deg, var(--glass-top), var(--glass-bot));
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: 0 18px 50px -14px rgba(0, 0, 0, 0.42);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-bottom: 0.5px solid var(--line);
    flex-shrink: 0;
  }
  .head-ico {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 7px;
    background: color-mix(in oklab, var(--color-accent) 13%, white);
  }
  .head-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-ink);
  }
  .step-count {
    margin-left: auto;
    font-size: 10.5px;
    font-weight: 600;
    color: var(--color-ink-ghost);
    font-variant-numeric: tabular-nums;
  }
  .stepper {
    display: flex;
    gap: 4px;
    padding: 8px 12px 0;
    flex-shrink: 0;
  }
  .seg {
    flex: 1;
    height: 3px;
    border-radius: 999px;
    background: color-mix(in oklab, var(--color-ink) 10%, transparent);
    transition: background 0.25s ease;
  }
  .seg.on {
    background: var(--color-accent);
  }
  .body {
    padding: 12px;
    flex: 1;
    min-height: 0;
  }
  .body.scroll {
    overflow-y: auto;
  }
  .body.wizard {
    /* Clip the horizontal slide transition, but let content (e.g. the open
       category dropdown) scroll vertically instead of being cut off. */
    overflow-x: hidden;
    overflow-y: auto;
    position: relative;
  }
  .step {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .q {
    font-size: 15px;
    font-weight: 650;
    letter-spacing: -0.2px;
    color: var(--color-ink);
  }
  .field-lg {
    font-size: 15px;
    font-weight: 500;
    padding: 9px 12px;
  }
  .hint {
    font-size: 11.5px;
    color: var(--color-ink-faint);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-top: 0.5px solid var(--line);
    flex-shrink: 0;
  }
  .link-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--color-ink-faint);
    transition: color 0.12s ease;
  }
  .link-btn:hover {
    color: var(--color-ink);
  }
  .btn-ghost {
    background: transparent;
    color: var(--color-ink-faint);
  }
  .btn-ghost:hover {
    color: var(--color-ink);
  }
  @media (prefers-reduced-motion: reduce) {
    .step {
      transition: none;
    }
  }
</style>
