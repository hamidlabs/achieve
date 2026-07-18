<script lang="ts">
  // One note: rendered markdown with inline edit + a two-step delete. Shared by
  // the per-task journal (TaskNotes) and the global history (NotesView).
  import Icon from "../icons/Icon.svelte";
  import Markdown from "../ui/Markdown.svelte";
  import MarkdownEditor from "../ui/MarkdownEditor.svelte";
  import { api } from "../api";
  import { catColor } from "../format";
  import { fmtDay, fmtTime } from "../reminders";
  import type { Note } from "../types";

  interface Props {
    note: Note;
    /** Show the owning task (for the cross-task history view). */
    showTask?: boolean;
    onChanged: () => void;
  }
  let { note, showTask = false, onChanged }: Props = $props();

  let editing = $state(false);
  let draft = $state(note.body_md);
  let confirming = $state(false);
  let busy = $state(false);
  let err = $state<string | null>(null);

  const edited = $derived(note.updated_local !== note.created_local);
  function when(local: string): string {
    const [d, t] = local.split(" ");
    return `${fmtDay(d)} · ${fmtTime(t)}`;
  }

  async function save() {
    if (busy) return;
    busy = true; err = null;
    try {
      await api.updateNote(note.id, draft);
      editing = false;
      onChanged();
    } catch (e) {
      console.error("[achieve] update note failed:", e);
      err = "Couldn't save. Try again.";
    } finally { busy = false; }
  }
  async function del() {
    if (busy) return;
    busy = true; err = null;
    try {
      await api.deleteNote(note.id);
      onChanged();
    } catch (e) {
      console.error("[achieve] delete note failed:", e);
      err = "Couldn't delete. Try again.";
      confirming = false;
    } finally { busy = false; }
  }
  function startEdit() {
    draft = note.body_md;
    editing = true;
    confirming = false;
  }
</script>

<div class="note">
  <div class="note-top">
    {#if showTask}
      <span class="task-chip" style="--c: {catColor(note.category_color)}">
        <span class="task-dot"></span>{note.task_title}
      </span>
    {/if}
    <span class="note-when">{when(note.created_local)}{#if edited} · edited{/if}</span>
    {#if !editing}
      <div class="note-tools">
        <button class="ntool no-drag" title="Edit" onclick={startEdit}><Icon name="pencil" size={13} /></button>
        {#if confirming}
          <button class="ntool danger no-drag" onclick={del} disabled={busy}>Delete</button>
          <button class="ntool no-drag" onclick={() => (confirming = false)}>Cancel</button>
        {:else}
          <button class="ntool no-drag" title="Delete" onclick={() => (confirming = true)}><Icon name="trash" size={13} /></button>
        {/if}
      </div>
    {/if}
  </div>

  {#if editing}
    <MarkdownEditor bind:value={draft} rows={4} placeholder="Write your note… markdown supported" />
    {#if err}<div class="note-err">{err}</div>{/if}
    <div class="flex gap-2 mt-2">
      <button class="btn btn-soft flex-1" onclick={() => (editing = false)}>Cancel</button>
      <button class="btn btn-primary flex-1" onclick={save} disabled={busy}><Icon name="check" size={14} /> Save</button>
    </div>
  {:else}
    <div class="note-body"><Markdown source={note.body_md} /></div>
    {#if err}<div class="note-err">{err}</div>{/if}
  {/if}
</div>

<style>
  .note {
    padding: 11px 12px;
    border: 0.5px solid var(--line);
    border-radius: var(--radius-md);
    background: #fff;
  }
  .note-top {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 7px;
  }
  .task-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 10.5px;
    font-weight: 700;
    line-height: 1;
    padding: 3px 8px;
    border-radius: 999px;
    color: color-mix(in oklab, var(--c) 66%, var(--color-ink));
    background: color-mix(in oklab, var(--c) 13%, white);
    border: 0.5px solid color-mix(in oklab, var(--c) 30%, transparent);
    max-width: 190px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .task-dot { width: 6px; height: 6px; border-radius: 999px; background: var(--c); flex-shrink: 0; }
  .note-when {
    font-size: 10.5px;
    font-weight: 600;
    color: var(--color-ink-ghost);
    white-space: nowrap;
  }
  .note-tools {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .ntool {
    display: inline-grid;
    place-items: center;
    height: 26px;
    min-width: 26px;
    padding: 0 7px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    font-weight: 600;
    color: var(--color-ink-faint);
    transition: all 0.12s ease;
  }
  .ntool:hover { color: var(--color-accent); background: var(--violet-50); }
  .ntool.danger { color: var(--color-danger); }
  .ntool.danger:hover { background: color-mix(in oklab, var(--color-danger) 12%, white); }
  .note-body { font-size: 12.5px; }
  .note-err {
    margin-top: 6px;
    font-size: 11px;
    font-weight: 500;
    color: var(--color-danger);
  }
</style>
