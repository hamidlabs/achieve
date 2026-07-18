<script lang="ts">
  // The per-task note journal shown in the task editor: a composer plus the
  // task's dated notes, newest first. Each note edits/deletes independently.
  import Icon from "../icons/Icon.svelte";
  import MarkdownEditor from "../ui/MarkdownEditor.svelte";
  import NoteCard from "./NoteCard.svelte";
  import { api } from "../api";
  import type { Note } from "../types";

  interface Props {
    taskId: number;
  }
  let { taskId }: Props = $props();

  let notes = $state<Note[]>([]);
  let composing = $state(false);
  let draft = $state("");
  let busy = $state(false);
  let err = $state<string | null>(null);

  async function load() {
    try {
      notes = await api.notes(taskId);
    } catch (e) {
      console.error("[achieve] load notes failed:", e);
    }
  }
  $effect(() => {
    if (taskId) load();
  });

  async function add() {
    if (!draft.trim() || busy) return;
    busy = true; err = null;
    try {
      await api.createNote(taskId, draft.trim());
      draft = "";
      composing = false;
      await load();
    } catch (e) {
      console.error("[achieve] create note failed:", e);
      err = "Couldn't add that note. Try again.";
    } finally { busy = false; }
  }
</script>

<div class="fieldset">
  <div class="notes-head">
    <span class="fieldset-label">Notes</span>
    {#if notes.length}<span class="notes-count">{notes.length}</span>{/if}
  </div>

  {#if composing}
    <div class="composer">
      <MarkdownEditor bind:value={draft} rows={3} placeholder="Jot a note… saved with today's date" />
      {#if err}<div class="notes-err">{err}</div>{/if}
      <div class="flex gap-2 mt-2">
        <button class="btn btn-soft flex-1" onclick={() => { composing = false; draft = ""; }}>Cancel</button>
        <button class="btn btn-primary flex-1" onclick={add} disabled={busy || !draft.trim()}>
          <Icon name="check" size={14} /> Add note
        </button>
      </div>
    </div>
  {:else}
    <button class="add-note no-drag" onclick={() => (composing = true)}>
      <Icon name="plus" size={14} /> Add a note
    </button>
  {/if}

  {#if notes.length}
    <div class="flex flex-col gap-2 mt-1">
      {#each notes as n (n.id)}
        <NoteCard note={n} onChanged={load} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .notes-head {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .notes-count {
    display: grid;
    place-items: center;
    min-width: 17px;
    height: 17px;
    padding: 0 5px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 700;
    color: var(--color-accent);
    background: var(--violet-50);
  }
  .add-note {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    align-self: flex-start;
    padding: 8px 13px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-accent);
    border: 0.5px dashed color-mix(in oklab, var(--color-accent) 42%, var(--line));
    border-radius: var(--radius-md);
    background: var(--violet-50);
    transition: background 0.12s ease;
  }
  .add-note:hover { background: var(--violet-100); }
  .composer {
    padding: 2px 0;
  }
  .notes-err {
    margin-top: 6px;
    font-size: 11px;
    font-weight: 500;
    color: var(--color-danger);
  }
</style>
