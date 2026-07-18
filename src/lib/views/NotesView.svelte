<script lang="ts">
  // The notes journal: search + browse every note across all tasks, grouped by
  // day, each independently editable/deletable. Also composes a new note against
  // any current task. This is the "task/note history" search surface.
  import { onMount } from "svelte";
  import Icon from "../icons/Icon.svelte";
  import MarkdownEditor from "../ui/MarkdownEditor.svelte";
  import Select from "../ui/Select.svelte";
  import NoteCard from "../components/NoteCard.svelte";
  import { api } from "../api";
  import { store } from "../store.svelte";
  import { fmtDay } from "../reminders";
  import type { Note } from "../types";

  let query = $state("");
  let notes = $state<Note[]>([]);
  let loading = $state(true);
  let timer: ReturnType<typeof setTimeout> | undefined;

  // Compose against any current task.
  let composing = $state(false);
  let composeTask = $state<string>("");
  let draft = $state("");
  let busy = $state(false);
  let err = $state<string | null>(null);

  const taskOptions = $derived(
    [...store.tasks, ...store.upcoming].map((t) => ({ value: String(t.id), label: t.title })),
  );

  async function run() {
    loading = true;
    try {
      notes = await api.searchNotes(query.trim());
    } catch (e) {
      console.error("[achieve] search notes failed:", e);
      notes = [];
    } finally { loading = false; }
  }
  function onInput() {
    clearTimeout(timer);
    timer = setTimeout(run, 180);
  }
  onMount(run);

  // Group by calendar day (created_local is "YYYY-MM-DD HH:MM").
  const groups = $derived.by(() => {
    const map = new Map<string, Note[]>();
    for (const n of notes) {
      const day = n.created_local.split(" ")[0];
      (map.get(day) ?? map.set(day, []).get(day)!).push(n);
    }
    return [...map.entries()].map(([day, items]) => ({ day, label: fmtDay(day), items }));
  });

  async function add() {
    if (!composeTask || !draft.trim() || busy) return;
    busy = true; err = null;
    try {
      await api.createNote(Number(composeTask), draft.trim());
      draft = ""; composing = false; composeTask = "";
      await run();
    } catch (e) {
      console.error("[achieve] create note failed:", e);
      err = "Couldn't add that note. Try again.";
    } finally { busy = false; }
  }
</script>

<div class="h-full flex flex-col">
  <header class="app-bar" data-tauri-drag-region>
    <span class="app-bar-badge no-drag"><Icon name="book-open" size={18} /></span>
    <div class="flex-1 min-w-0">
      <div class="text-[16px] font-bold text-ink leading-tight">Journal</div>
      <div class="text-[11.5px] text-ink-faint truncate">Every note, searchable by task or text</div>
    </div>
    <button class="icon-btn no-drag shrink-0" title="Hide" aria-label="Hide" onclick={() => api.dismiss()}>
      <Icon name="chevron-down" size={18} />
    </button>
  </header>

  <div class="px-4 pb-2 pt-0.5 shrink-0">
    <div class="search no-drag">
      <Icon name="target" size={15} class="text-ink-faint shrink-0" />
      <input class="search-input" style="user-select:text;" placeholder="Search notes…"
        bind:value={query} oninput={onInput} />
      {#if query}
        <button class="search-clear" aria-label="Clear" onclick={() => { query = ""; run(); }}><Icon name="x" size={13} /></button>
      {/if}
    </div>
  </div>

  <div class="flex-1 min-h-0 overflow-y-auto px-4 pb-4">
    <!-- Composer -->
    {#if composing}
      <div class="card p-3.5 mb-3.5">
        <div class="text-[10.5px] font-bold tracking-wide uppercase text-ink-faint mb-2">New note</div>
        {#if taskOptions.length}
          <Select value={composeTask} options={taskOptions} placeholder="Choose a task…"
            ariaLabel="Task" onChange={(v) => (composeTask = v)} />
          <div class="mt-2.5">
            <MarkdownEditor bind:value={draft} rows={3} placeholder="Write your note…" />
          </div>
          {#if err}<div class="text-[11px] font-medium text-danger mt-1.5" style="color: var(--color-danger);">{err}</div>{/if}
          <div class="flex gap-2 mt-2.5">
            <button class="btn btn-soft flex-1" onclick={() => { composing = false; draft = ""; composeTask = ""; }}>Cancel</button>
            <button class="btn btn-primary flex-1" onclick={add} disabled={busy || !composeTask || !draft.trim()}>
              <Icon name="check" size={14} /> Add note
            </button>
          </div>
        {:else}
          <p class="text-[12.5px] text-ink-faint">Add a task first, then you can note against it here.</p>
          <button class="btn btn-soft mt-2.5" onclick={() => (composing = false)}>Close</button>
        {/if}
      </div>
    {:else}
      <button class="add-note no-drag mb-3.5" onclick={() => (composing = true)}>
        <Icon name="plus" size={14} /> New note
      </button>
    {/if}

    {#if loading}
      <div class="py-10 text-center text-[12.5px] text-ink-faint">Searching…</div>
    {:else if notes.length === 0}
      <div class="empty">
        <span class="empty-orb"><Icon name="book-open" size={22} /></span>
        <div class="text-[13px] font-semibold text-ink mt-1">{query ? "No matching notes" : "No notes yet"}</div>
        <div class="text-[12px] text-ink-faint">{query ? "Try a different search." : "Notes you add to tasks show up here, newest first."}</div>
      </div>
    {:else}
      <div class="flex flex-col gap-4">
        {#each groups as g (g.day)}
          <div>
            <div class="day-label">{g.label}</div>
            <div class="flex flex-col gap-2">
              {#each g.items as n (n.id)}
                <NoteCard note={n} showTask onChanged={run} />
              {/each}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    border-radius: var(--radius-md);
    background: #fff;
    border: 0.5px solid var(--line-strong);
    box-shadow: inset 0 1px 1px rgba(28, 27, 42, 0.03);
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .search:focus-within {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px color-mix(in oklab, var(--color-accent) 18%, transparent);
  }
  .search-input {
    flex: 1;
    min-width: 0;
    font-size: 13.5px;
    color: var(--color-ink);
    background: transparent;
    border: none;
    outline: none;
  }
  .search-clear {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 999px;
    color: var(--color-ink-faint);
    transition: all 0.12s ease;
  }
  .search-clear:hover { color: var(--color-ink); background: color-mix(in oklab, var(--color-ink) 8%, transparent); }
  .day-label {
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--color-ink-faint);
    margin-bottom: 8px;
  }
  .add-note {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-accent);
    border: 0.5px dashed color-mix(in oklab, var(--color-accent) 42%, var(--line));
    border-radius: var(--radius-md);
    background: var(--violet-50);
    transition: background 0.12s ease;
  }
  .add-note:hover { background: var(--violet-100); }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    text-align: center;
    padding: 40px 16px;
  }
  .empty-orb {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 999px;
    color: var(--color-accent);
    background: var(--violet-50);
    margin-bottom: 6px;
  }
</style>
