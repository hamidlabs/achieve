<script lang="ts">
  import { fly } from "svelte/transition";
  import Icon from "../icons/Icon.svelte";
  import MarkdownEditor from "../ui/MarkdownEditor.svelte";
  import Overlay from "../ui/Overlay.svelte";
  import DatePicker from "../ui/DatePicker.svelte";
  import ReminderEditor from "./ReminderEditor.svelte";
  import TaskNotes from "./TaskNotes.svelte";
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

<Overlay variant="sheet" maxWidth={404} {onClose}>
  {#if isEdit}
    <!-- ============ EDIT: everything at once ============ -->
    <div class="head">
      <span class="head-ico"><Icon name="pencil" size={15} /></span>
      <span class="head-title">Edit task</span>
      <button class="hbtn ml-auto" onclick={onClose} aria-label="Close"><Icon name="x" size={17} /></button>
    </div>
    <div class="body scroll flex flex-col gap-3.5">
      <input class="field field-lg" style="user-select:text;"
        placeholder="What needs doing?" bind:value={title}
        onkeydown={(e) => e.key === "Enter" && (e.metaKey || e.ctrlKey) && save()} />
      {@render labeled("Category", categorySection)}
      {@render whenSection()}
      {@render labeled("Estimate & repeat", timeSection)}
      {@render remindersSection()}
      {@render detailsSection()}
      {#if task}<TaskNotes taskId={task.id} />{/if}
    </div>
    {#if saveError}<div class="err-line" role="alert">{saveError}</div>{/if}
    <div class="foot">
      <button class="btn btn-danger-ghost" onclick={remove} aria-label="Delete task"><Icon name="trash" size={15} /></button>
      <button class="btn btn-soft flex-1" onclick={onClose}>Cancel</button>
      <button class="btn btn-primary flex-[2]" onclick={save} disabled={!title.trim()}>
        <Icon name="check" size={15} /> Save changes
      </button>
    </div>
  {:else}
    <!-- ============ CREATE: guided wizard ============ -->
    <div class="head">
      {#if step > 0}
        <button class="hbtn" title="Back" onclick={back} aria-label="Back"><Icon name="chevron-right" size={18} class="rotate-180" /></button>
      {:else}
        <span class="head-ico"><Icon name="sparkles" size={15} /></span>
      {/if}
      <div class="head-titles">
        <span class="head-title">New task</span>
        <span class="head-sub">{STEPS[step]} · step {step + 1} of {STEPS.length}</span>
      </div>
      <button class="hbtn" onclick={onClose} aria-label="Close"><Icon name="x" size={17} /></button>
    </div>
    <div class="stepper">
      {#each STEPS as s, i (s)}
        <span class="seg" class:on={i <= step}></span>
      {/each}
    </div>

    <div class="body wizard">
      {#key step}
        <div class="step" in:fly={{ x: dir * 20, duration: 190, opacity: 0 }}>
          {#if step === 0}
            <div class="q">What needs doing?</div>
            <input class="field field-lg" style="user-select:text;" placeholder="e.g. Draft the proposal"
              bind:value={title} use:autofocus
              onkeydown={(e) => e.key === "Enter" && next()} />
            <p class="hint">Give it a clear, single action. Press Enter to continue.</p>
          {:else if step === 1}
            <div class="q">Choose a category</div>
            <p class="hint -mt-1">Group it so time adds up the way you think.</p>
            {@render categorySection()}
          {:else if step === 2}
            <div class="q">When will you do it?</div>
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
        <button class="btn btn-soft" onclick={() => { categoryId = null; next(); }}>Skip</button>
      {/if}
      <button class="btn btn-primary flex-1" onclick={next} disabled={!canNext}>
        {#if step < STEPS.length - 1}
          Continue <Icon name="chevron-right" size={15} />
        {:else}
          <Icon name="check" size={15} /> Create task
        {/if}
      </button>
    </div>
  {/if}
</Overlay>

<!-- ===================== shared sections ===================== -->

{#snippet labeled(label: string, section: import("svelte").Snippet)}
  <div class="fieldset">
    <div class="fieldset-label">{label}</div>
    {@render section()}
  </div>
{/snippet}

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
    <div class="presets">
      {#each presets as p (p)}
        <button class="preset" class:on={estimate === p} onclick={() => (estimate = p)}>
          {fmtPreset(p)}
        </button>
      {/each}
      <input type="number" class="preset-custom no-drag" style="user-select:text;" placeholder="min" bind:value={estimate} aria-label="Custom minutes" />
    </div>
    <button class="toggle-row no-drag" onclick={() => (daily = !daily)} aria-pressed={daily}>
      <span class="toggle-ico"><Icon name="repeat" size={14} /></span>
      <div class="text-left flex-1">
        <div class="text-[12.5px] font-medium text-ink">Repeat daily</div>
        <div class="text-[10.5px] text-ink-faint">A fresh copy shows up each day</div>
      </div>
      <span class="switch" class:on={daily}><span class="knob"></span></span>
    </button>
  </div>
{/snippet}

{#snippet detailsSection()}
  <div class="fieldset">
    <div class="fieldset-label">Details</div>
    <MarkdownEditor bind:value={body} rows={4} />
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
    gap: 10px;
    padding: 13px 14px;
    border-bottom: 0.5px solid var(--line);
    flex-shrink: 0;
  }
  .head-ico {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 10px;
    color: #fff;
    background: linear-gradient(150deg, var(--color-accent-bright), var(--color-accent-strong));
    box-shadow: 0 3px 10px -4px rgba(92, 79, 214, 0.55);
    flex-shrink: 0;
  }
  .head-titles {
    display: flex;
    flex-direction: column;
    line-height: 1.15;
  }
  .head-title {
    font-size: 14.5px;
    font-weight: 700;
    color: var(--color-ink);
  }
  .head-sub {
    font-size: 11px;
    font-weight: 500;
    color: var(--color-ink-faint);
    font-variant-numeric: tabular-nums;
  }
  /* icon button tuned for the sheet header */
  .hbtn {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-md);
    color: var(--color-ink-faint);
    transition: color 0.12s ease, background 0.12s ease;
    flex-shrink: 0;
  }
  .hbtn:hover { color: var(--color-accent); background: var(--violet-50); }
  .stepper {
    display: flex;
    gap: 5px;
    padding: 12px 14px 2px;
    flex-shrink: 0;
  }
  .seg {
    flex: 1;
    height: 3.5px;
    border-radius: 999px;
    background: color-mix(in oklab, var(--color-ink) 9%, transparent);
    transition: background 0.25s ease;
  }
  .seg.on {
    background: var(--color-accent);
  }
  /* labelled field group (edit view) */
  .fieldset {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .fieldset-label {
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--color-ink-faint);
  }
  .body {
    padding: 14px;
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
  /* estimate presets */
  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
    align-items: center;
  }
  .preset {
    min-width: 44px;
    padding: 7px 12px;
    border-radius: var(--radius-pill);
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-ink-soft);
    background: #fff;
    border: 0.5px solid var(--line-strong);
    transition: all 0.12s ease;
  }
  .preset:hover { border-color: color-mix(in oklab, var(--color-accent) 40%, var(--line-strong)); color: var(--color-ink); }
  .preset.on {
    color: #fff;
    background: var(--color-accent);
    border-color: transparent;
    box-shadow: 0 3px 10px -5px rgba(92, 79, 214, 0.6);
  }
  .preset-custom {
    width: 72px;
    padding: 7px 10px;
    font-size: 12.5px;
    color: var(--color-ink);
    background: #fff;
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-pill);
    outline: none;
    text-align: center;
  }
  .preset-custom:focus { border-color: var(--color-accent); box-shadow: 0 0 0 3px color-mix(in oklab, var(--color-accent) 20%, transparent); }
  /* repeat toggle row */
  .toggle-row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 9px 11px;
    border-radius: var(--radius-md);
    border: 0.5px solid var(--line);
    background: var(--card-2);
    transition: border-color 0.12s ease;
  }
  .toggle-row:hover { border-color: var(--line-strong); }
  .toggle-ico {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    color: var(--color-accent);
    background: var(--violet-50);
    flex-shrink: 0;
  }
  .switch {
    width: 38px; height: 22px; border-radius: 999px; flex-shrink: 0;
    background: rgba(28, 27, 42, 0.16); position: relative; transition: background 0.16s ease;
  }
  .switch.on { background: var(--color-accent); }
  .knob {
    position: absolute; top: 2px; left: 2px; width: 18px; height: 18px; border-radius: 999px;
    background: #fff; box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25); transition: transform 0.16s ease;
  }
  .switch.on .knob { transform: translateX(16px); }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
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
