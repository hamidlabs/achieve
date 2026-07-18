<script lang="ts">
  import { fly } from "svelte/transition";
  import Icon from "../icons/Icon.svelte";
  import MarkdownEditor from "../ui/MarkdownEditor.svelte";
  import Overlay from "../ui/Overlay.svelte";
  import DatePicker from "../ui/DatePicker.svelte";
  import ReminderEditor from "./ReminderEditor.svelte";
  import { api } from "../api";
  import CategoryPicker from "./CategoryPicker.svelte";
  import type { Category, Reminder, ReminderSpec, Task } from "../types";
  import { fmtReminder, channelLabel } from "../reminders";

  interface Props {
    task: Task | null;
    categories: Category[];
    onSaved: () => void;
    onClose: () => void;
  }
  let { task, categories, onSaved, onClose }: Props = $props();

  // Editing shows every field at once (quick changes); creating is a short
  // guided wizard (Name -> Category -> When -> Time) so it never feels like a
  // wall of fields.
  const isEdit = !!task;

  // Local YYYY-MM-DD, matching the backend's localtime dates.
  function ymd(d: Date): string {
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    return `${d.getFullYear()}-${m}-${day}`;
  }
  const todayYmd = ymd(new Date());

  let title = $state(task?.title ?? "");
  let body = $state(task?.body_md ?? "");
  let categoryId = $state<number | null>(task?.category_id ?? null);
  let estimate = $state<number | null>(task?.estimate_min ?? 30);
  let daily = $state(task?.recurrence === "daily");
  let planDate = $state<string | null>(task?.plan_date ?? todayYmd);
  let saving = $state(false);
  let showDetails = $state(!!task?.body_md?.trim());
  let showDate = $state(false); // edit-mode: reveal the calendar inline

  // Reminders. For an existing task they load from the backend and persist
  // immediately; for a new task they're kept as drafts and created after save.
  let reminders = $state<Reminder[]>([]);
  let drafts = $state<ReminderSpec[]>([]);
  let formOpen = $state(false);
  let formInitial = $state<Reminder | null>(null);
  let formEditId = $state<number | null>(null); // existing reminder being edited
  let formDraftIdx = $state<number | null>(null); // draft being edited

  $effect(() => {
    if (isEdit && task) loadReminders(task.id);
  });
  async function loadReminders(id: number) {
    try {
      reminders = await api.reminders(id);
    } catch (e) {
      console.error("[achieve] load reminders failed:", e);
    }
  }

  // Render both saved reminders and drafts through the same row formatter.
  function specToReminder(s: ReminderSpec, i: number): Reminder {
    return {
      id: -1 - i,
      task_id: 0,
      remind_at_local: s.remind_at,
      remind_at: "",
      rrule: s.rrule,
      rrule_until: s.until,
      rrule_count: s.count,
      channel: s.channel,
      note: s.note,
      status: "pending",
    };
  }
  const shownReminders = $derived(
    isEdit ? reminders : drafts.map((d, i) => specToReminder(d, i)),
  );

  function addReminder() {
    formInitial = null;
    formEditId = null;
    formDraftIdx = null;
    formOpen = true;
  }
  function editReminder(r: Reminder) {
    formInitial = r;
    formOpen = true;
    if (isEdit) {
      formEditId = r.id;
      formDraftIdx = null;
    } else {
      formDraftIdx = -1 - r.id; // recover the draft index from the synthetic id
      formEditId = null;
    }
  }
  async function saveReminder(spec: ReminderSpec) {
    if (isEdit && task) {
      try {
        if (formEditId != null) await api.updateReminder(formEditId, spec);
        else await api.createReminder(task.id, spec);
        await loadReminders(task.id);
      } catch (e) {
        console.error("[achieve] save reminder failed:", e);
        saveError = "Couldn't save that reminder. Try again.";
      }
    } else if (formDraftIdx != null && formDraftIdx >= 0) {
      drafts[formDraftIdx] = spec;
    } else {
      drafts = [...drafts, spec];
    }
    formOpen = false;
  }
  async function removeReminder(r: Reminder) {
    if (isEdit) {
      try {
        await api.deleteReminder(r.id);
        await loadReminders(r.task_id);
      } catch (e) {
        console.error("[achieve] delete reminder failed:", e);
      }
    } else {
      const idx = -1 - r.id;
      drafts = drafts.filter((_, i) => i !== idx);
    }
    if (formOpen) formOpen = false;
  }
  // Inline, non-blocking error (never a native alert): the dialog stays open
  // with the user's input intact, and the primary button is the retry.
  let saveError = $state<string | null>(null);

  const presets = [15, 30, 45, 60, 90, 120];

  // Wizard state (create only).
  const STEPS = ["Name", "Category", "When", "Time"];
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

  // Advance the create wizard once a category / date is chosen.
  function onCatPicked() {
    if (!isEdit && step === 1) next();
  }
  function onDatePicked(d: string | null) {
    planDate = d;
    if (!isEdit) next();
    else showDate = false;
  }

  async function save() {
    if (!title.trim() || saving) return;
    saving = true;
    saveError = null;
    const payload = {
      category_id: categoryId,
      title: title.trim(),
      body_md: body,
      estimate_min: estimate,
      recurrence: daily ? "daily" : null,
    };
    try {
      if (task) {
        await api.updateTask({ id: task.id, ...payload });
        if ((planDate ?? null) !== (task.plan_date ?? null)) {
          await api.setPlanDate(task.id, planDate);
        }
      } else {
        const id = await api.createTask(payload);
        // create_task defaults to today; only re-file when a different date was picked.
        if (planDate !== todayYmd) await api.setPlanDate(id, planDate);
        // Persist any reminders drafted while creating.
        for (const d of drafts) await api.createReminder(id, d);
      }
      onSaved();
    } catch (e) {
      console.error("[achieve] save task failed:", e);
      saveError = "Couldn't save that. Check and try again.";
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!task || saving) return;
    saving = true;
    saveError = null;
    try {
      await api.deleteTask(task.id);
      onSaved();
    } catch (e) {
      console.error("[achieve] delete task failed:", e);
      saveError = "Couldn't delete that. Try again.";
    } finally {
      saving = false;
    }
  }

  function autofocus(node: HTMLInputElement) {
    node.focus();
  }
  function fmtPreset(p: number): string {
    return p < 60 ? `${p}m` : `${p / 60}h`.replace(".5", "½");
  }
  function fmtDate(d: string | null): string {
    if (!d) return "Someday";
    if (d === todayYmd) return "Today";
    const dt = new Date(d + "T00:00:00");
    return dt.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
  }
</script>

<Overlay variant="dialog" {onClose}>
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
      {@render whenSection()}
      {@render timeSection()}
      {@render remindersSection()}
      {@render detailsSection()}
    </div>
    {#if saveError}<div class="err-line" role="alert">{saveError}</div>{/if}
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
          {:else if step === 2}
            <div class="q">When do you want to do this?</div>
            <DatePicker current={planDate} onPick={onDatePicked} allowNone={false} />
          {:else}
            <div class="q">How long will it take?</div>
            {@render timeSection()}
            {@render remindersSection()}
            {@render detailsSection()}
          {/if}
        </div>
      {/key}
    </div>

    {#if saveError}<div class="err-line" role="alert">{saveError}</div>{/if}
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
</Overlay>

<!-- ===================== shared sections ===================== -->

{#snippet categorySection()}
  <CategoryPicker bind:categoryId {categories} onPicked={onCatPicked} />
{/snippet}

{#snippet whenSection()}
  <div>
    <button class="when-row no-drag" onclick={() => (showDate = !showDate)}>
      <Icon name="calendar" size={14} style="color: var(--color-accent);" />
      <span class="flex-1 text-left text-[12.5px] text-ink">When</span>
      <span class="when-val">{fmtDate(planDate)}</span>
      <Icon name="chevron-down" size={14} class={showDate ? "rotate-180 text-ink-faint" : "text-ink-faint"} />
    </button>
    {#if showDate}
      <div class="when-cal">
        <DatePicker current={planDate} onPick={onDatePicked} />
      </div>
    {/if}
  </div>
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

{#snippet remindersSection()}
  <div class="rem-sec">
    <div class="rem-head">
      <Icon name="bell" size={13} style="color: var(--color-accent);" />
      <span class="rem-title">Reminders</span>
      {#if shownReminders.length}<span class="rem-count">{shownReminders.length}</span>{/if}
    </div>

    {#if shownReminders.length}
      <div class="flex flex-col gap-1.5">
        {#each shownReminders as r (r.id)}
          <div class="rem-row" class:sent={r.status === "sent"}>
            <button class="rem-main no-drag" onclick={() => editReminder(r)}>
              <Icon
                name={r.channel === "email" ? "mail" : r.rrule ? "repeat" : "bell"}
                size={13} class="text-ink-faint shrink-0" />
              <span class="rem-when">{fmtReminder(r)}</span>
              <span class="rem-meta">
                {channelLabel(r.channel)}{#if r.status === "scheduled"} · scheduled{:else if r.status === "sent"} · sent{:else if r.status === "failed"} · retrying{/if}
              </span>
            </button>
            <button class="rem-x no-drag" title="Remove reminder" onclick={() => removeReminder(r)}>
              <Icon name="x" size={13} />
            </button>
          </div>
        {/each}
      </div>
    {/if}

    {#if formOpen}
      <!-- Key on the target so switching add<->edit (or between reminders) while
           the form stays open remounts it with the right initial values. -->
      {#key formInitial}
        <ReminderEditor existing={formInitial} onSave={saveReminder} onCancel={() => (formOpen = false)} />
      {/key}
    {:else}
      <button class="rem-add no-drag" onclick={addReminder}>
        <Icon name="plus" size={13} /> Add reminder
      </button>
    {/if}
  </div>
{/snippet}

<style>
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
  /* Inline, non-blocking error strip above the footer buttons. */
  .err-line {
    flex-shrink: 0;
    padding: 7px 12px;
    font-size: 12px;
    font-weight: 500;
    color: var(--color-danger);
    background: color-mix(in oklab, var(--color-danger) 9%, transparent);
  }
  .when-row {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 8px 10px;
    border-radius: var(--radius-md);
    border: 0.5px solid var(--line);
    background: #fff;
    transition: border-color 0.12s ease;
  }
  .when-row:hover {
    border-color: var(--line-strong);
  }
  .when-val {
    font-size: 12px;
    font-weight: 600;
    color: var(--color-accent);
  }
  .when-cal {
    margin-top: 8px;
    padding: 8px;
    border: 0.5px solid var(--line);
    border-radius: var(--radius-md);
    background: color-mix(in oklab, var(--color-ink) 2%, white);
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
  /* Reminders */
  .rem-sec {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .rem-head {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .rem-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-ink-faint);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .rem-count {
    display: grid;
    place-items: center;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 700;
    color: var(--color-accent);
    background: color-mix(in oklab, var(--color-accent) 15%, white);
  }
  .rem-row {
    display: flex;
    align-items: center;
    gap: 4px;
    border: 0.5px solid var(--line);
    border-radius: var(--radius-md);
    background: #fff;
    transition: border-color 0.12s ease;
  }
  .rem-row:hover {
    border-color: var(--line-strong);
  }
  .rem-row.sent {
    opacity: 0.6;
  }
  .rem-main {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    padding: 8px 4px 8px 10px;
    text-align: left;
  }
  .rem-when {
    font-size: 12.5px;
    font-weight: 550;
    color: var(--color-ink);
    white-space: nowrap;
  }
  .rem-meta {
    flex: 1;
    min-width: 0;
    font-size: 11px;
    color: var(--color-ink-ghost);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: right;
  }
  .rem-x {
    display: grid;
    place-items: center;
    width: 28px;
    height: 32px;
    color: var(--color-ink-ghost);
    border-radius: var(--radius-sm);
    transition: color 0.12s ease;
    flex-shrink: 0;
  }
  .rem-x:hover {
    color: var(--color-danger);
  }
  .rem-add {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    align-self: flex-start;
    padding: 6px 11px;
    font-size: 12px;
    font-weight: 500;
    color: var(--color-accent);
    border: 0.5px dashed color-mix(in oklab, var(--color-accent) 40%, var(--line));
    border-radius: var(--radius-md);
    transition: background 0.12s ease;
  }
  .rem-add:hover {
    background: color-mix(in oklab, var(--color-accent) 8%, white);
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
