<script lang="ts">
  import Icon from "../icons/Icon.svelte";
  import { api } from "../api";
  import { refreshCategories } from "../store.svelte";
  import { catColor } from "../format";
  import type { Category } from "../types";

  interface Props {
    categoryId: number | null;
    categories: Category[];
    // Called after a category is chosen (used by the create wizard to advance).
    onPicked?: () => void;
  }
  let { categoryId = $bindable(null), categories, onPicked }: Props = $props();

  const palette = [
    "#6b6bff", "#37b6a6", "#e0a23c", "#c06bd8",
    "#e36b6b", "#4f9be0", "#5bc46b", "#df7bb0",
  ];

  let open = $state(false);
  let mode = $state<"select" | "create">("select");
  let query = $state("");
  let newColor = $state(palette[0]);
  let editingId = $state<number | null>(null);
  let editName = $state("");
  let editColor = $state(palette[0]);
  let busy = $state(false);
  let cpError = $state<string | null>(null); // inline, non-blocking error

  const selected = $derived(categories.find((c) => c.id === categoryId) ?? null);
  const q = $derived(query.trim().toLowerCase());
  const filtered = $derived(
    q ? categories.filter((c) => c.name.toLowerCase().includes(q)) : categories,
  );
  const exactMatch = $derived(categories.some((c) => c.name.toLowerCase() === q));

  function nextColor(): string {
    return palette[categories.length % palette.length];
  }

  function openSelect() {
    mode = "select";
    query = "";
    editingId = null;
    open = !open;
  }
  function openCreate() {
    mode = "create";
    query = "";
    newColor = nextColor();
    editingId = null;
    open = true;
  }
  function close() {
    open = false;
    editingId = null;
    query = "";
  }

  function pick(id: number | null) {
    categoryId = id;
    close();
    onPicked?.();
  }

  async function create(name: string, color: string) {
    const trimmed = name.trim();
    if (!trimmed || busy) return;
    busy = true;
    cpError = null;
    try {
      const id = await api.createCategory(trimmed, color);
      await refreshCategories();
      pick(id);
    } catch (e) {
      console.error("[achieve] create category failed:", e);
      cpError = "Couldn't create that category. Try again.";
    } finally {
      busy = false;
    }
  }

  function startEdit(c: Category) {
    editingId = c.id;
    editName = c.name;
    editColor = c.color?.startsWith("#") ? c.color : palette[0];
  }
  async function saveEdit() {
    const name = editName.trim();
    if (!name || editingId == null || busy) return;
    busy = true;
    cpError = null;
    try {
      await api.updateCategory(editingId, name, editColor);
      await refreshCategories();
      editingId = null;
    } catch (e) {
      console.error("[achieve] rename category failed:", e);
      cpError = "Couldn't rename that category. Try again.";
    } finally {
      busy = false;
    }
  }
  async function del(c: Category) {
    if (!confirm(`Delete "${c.name}"? Tasks in it are kept and become uncategorized.`)) return;
    busy = true;
    cpError = null;
    try {
      await api.deleteCategory(c.id);
      await refreshCategories();
      if (categoryId === c.id) categoryId = null;
    } catch (e) {
      console.error("[achieve] delete category failed:", e);
      cpError = "Couldn't delete that category. Try again.";
    } finally {
      busy = false;
    }
  }

  function autofocus(node: HTMLInputElement) {
    node.focus();
  }
</script>

<div class="cp">
  <!-- Trigger row: combobox + create button (see the screenshot). -->
  <div class="row">
    <button class="trigger no-drag" class:on={open && mode === "select"} onclick={openSelect}>
      {#if selected}
        <span class="dot" style="background: {catColor(selected.color)};"></span>
        <span class="name">{selected.name}</span>
      {:else}
        <span class="placeholder">Select category</span>
      {/if}
      <Icon name="chevron-down" size={15} class={open ? "rotate-180 chev" : "chev"} />
    </button>
    <button class="plus no-drag" title="New category" onclick={openCreate}>
      <Icon name="plus" size={16} />
    </button>
  </div>

  {#if open}
    <div class="panel fade">
      {#if mode === "create"}
        <div class="create">
          <div class="create-top">
            <span class="dot" style="background: {newColor};"></span>
            <input
              class="input no-drag"
              placeholder="New category name…"
              bind:value={query}
              use:autofocus
              onkeydown={(e) => {
                if (e.key === "Enter") create(query, newColor);
                if (e.key === "Escape") close();
              }} />
            <button class="btn-add" disabled={!query.trim() || busy} onclick={() => create(query, newColor)}>
              Add
            </button>
          </div>
          <div class="swatches">
            {#each palette as col (col)}
              <button
                class="swatch"
                aria-label="color"
                style="background: {col}; outline: {newColor === col ? '2px solid var(--color-accent)' : '2px solid transparent'};"
                onclick={() => (newColor = col)}></button>
            {/each}
          </div>
        </div>
      {:else}
        <input
          class="input search no-drag"
          placeholder="Search categories…"
          bind:value={query}
          use:autofocus
          onkeydown={(e) => {
            if (e.key === "Escape") close();
            if (e.key === "Enter" && !exactMatch && query.trim()) create(query, nextColor());
          }} />
        <div class="list">
          {#if categoryId != null}
            <button class="opt none" onclick={() => pick(null)}>
              <span class="dot ring"></span><span class="name">No category</span>
            </button>
          {/if}
          {#each filtered as c (c.id)}
            {#if editingId === c.id}
              <div class="opt editing">
                <span class="dot" style="background: {editColor};"></span>
                <input
                  class="input inline no-drag"
                  bind:value={editName}
                  use:autofocus
                  onkeydown={(e) => {
                    if (e.key === "Enter") saveEdit();
                    if (e.key === "Escape") editingId = null;
                  }} />
                <div class="edit-colors">
                  {#each palette as col (col)}
                    <button
                      class="swatch sm"
                      aria-label="color"
                      style="background: {col}; outline: {editColor === col ? '2px solid var(--color-accent)' : '2px solid transparent'};"
                      onclick={() => (editColor = col)}></button>
                  {/each}
                </div>
                <button class="act save" title="Save" disabled={!editName.trim() || busy} onclick={saveEdit}>
                  <Icon name="check" size={14} />
                </button>
              </div>
            {:else}
              <div class="opt" class:sel={categoryId === c.id}>
                <button class="opt-main" onclick={() => pick(c.id)}>
                  <span class="dot" style="background: {catColor(c.color)};"></span>
                  <span class="name">{c.name}</span>
                  {#if categoryId === c.id}<Icon name="check" size={13} class="tick" />{/if}
                </button>
                <button class="act" title="Rename" onclick={() => startEdit(c)}><Icon name="pencil" size={13} /></button>
                <button class="act del" title="Delete" onclick={() => del(c)}><Icon name="trash" size={13} /></button>
              </div>
            {/if}
          {/each}

          {#if q && !exactMatch}
            <button class="opt create-opt" onclick={() => create(query, nextColor())}>
              <span class="dot" style="background: {nextColor()};"></span>
              <span class="name">Create “{query.trim()}”</span>
              <Icon name="plus" size={13} class="tick" />
            </button>
          {:else if filtered.length === 0 && categoryId == null}
            <div class="empty">No categories yet. Type a name to create one.</div>
          {/if}
        </div>
      {/if}
      {#if cpError}<div class="cp-err" role="alert">{cpError}</div>{/if}
    </div>
  {/if}
</div>

<style>
  .cp { position: relative; }
  .row { display: flex; gap: 8px; }
  .trigger {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    border-radius: var(--radius-md);
    border: 0.5px solid var(--line-strong);
    background: color-mix(in oklab, var(--color-ink) 3%, white);
    font-size: 13px;
    color: var(--color-ink);
    transition: border-color 0.12s ease, background 0.12s ease;
  }
  .trigger:hover { border-color: color-mix(in oklab, var(--color-accent) 40%, var(--line-strong)); }
  .trigger.on { border-color: var(--color-accent); }
  .trigger :global(.chev) { margin-left: auto; color: var(--color-ink-ghost); transition: transform 0.15s ease; }
  .placeholder { color: var(--color-ink-faint); }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .plus {
    display: grid;
    place-items: center;
    width: 40px;
    border-radius: var(--radius-md);
    border: 0.5px solid var(--line-strong);
    background: color-mix(in oklab, var(--color-ink) 3%, white);
    color: var(--color-ink-soft);
    transition: border-color 0.12s ease, color 0.12s ease;
  }
  .plus:hover { border-color: var(--color-accent); color: var(--color-accent); }

  .panel {
    margin-top: 6px;
    border: 0.5px solid var(--line-strong);
    border-radius: var(--radius-md);
    background: linear-gradient(170deg, var(--glass-top), var(--glass-bot));
    box-shadow: 0 12px 30px -12px rgba(0, 0, 0, 0.35);
    overflow: hidden;
  }
  .input {
    width: 100%;
    padding: 8px 10px;
    font-size: 13px;
    color: var(--color-ink);
    background: transparent;
    border: none;
    user-select: text;
  }
  .input:focus { outline: none; }
  .search { border-bottom: 0.5px solid var(--line); }
  .list { max-height: 208px; overflow-y: auto; padding: 4px; display: flex; flex-direction: column; gap: 1px; }
  .opt {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px 2px 8px;
    border-radius: 7px;
    transition: background 0.1s ease;
  }
  .opt:hover { background: color-mix(in oklab, var(--color-ink) 6%, transparent); }
  .opt.sel { background: color-mix(in oklab, var(--color-accent) 12%, transparent); }
  .opt-main { flex: 1; display: flex; align-items: center; gap: 8px; padding: 5px 0; font-size: 13px; color: var(--color-ink); min-width: 0; }
  .opt-main :global(.tick) { margin-left: auto; color: var(--color-accent); }
  .opt.none .name { color: var(--color-ink-faint); }
  .dot { width: 9px; height: 9px; border-radius: 999px; flex-shrink: 0; }
  .dot.ring { background: transparent; border: 1.5px solid var(--line-strong); }
  .act {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 6px;
    color: var(--color-ink-ghost);
    opacity: 0;
    transition: opacity 0.1s ease, color 0.1s ease, background 0.1s ease;
  }
  .opt:hover .act { opacity: 1; }
  .act:hover { background: color-mix(in oklab, var(--color-ink) 8%, transparent); color: var(--color-ink); }
  .act.del:hover { color: var(--color-danger); }
  .act.save { opacity: 1; color: var(--color-accent); }
  .create-opt { color: var(--color-accent); }
  .create-opt .name { color: var(--color-accent); font-weight: 500; }
  .create-opt :global(.tick) { margin-left: auto; }
  .empty { padding: 12px 10px; font-size: 12px; color: var(--color-ink-faint); text-align: center; }
  .cp-err {
    padding: 7px 10px;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--color-danger);
    background: color-mix(in oklab, var(--color-danger) 9%, transparent);
    border-top: 0.5px solid var(--line);
  }

  .create { padding: 8px; display: flex; flex-direction: column; gap: 8px; }
  .create-top { display: flex; align-items: center; gap: 8px; }
  .create-top .input { border: 0.5px solid var(--line-strong); border-radius: 7px; }
  .btn-add {
    flex-shrink: 0;
    padding: 7px 12px;
    border-radius: 7px;
    font-size: 12.5px;
    font-weight: 600;
    color: white;
    background: var(--color-accent);
  }
  .btn-add:disabled { opacity: 0.5; }
  .swatches, .edit-colors { display: flex; flex-wrap: wrap; gap: 6px; }
  .edit-colors { gap: 4px; }
  .swatch { width: 20px; height: 20px; border-radius: 999px; outline-offset: 1px; }
  .swatch.sm { width: 15px; height: 15px; }
  .editing { align-items: center; }
  .editing .input.inline { border: 0.5px solid var(--line-strong); border-radius: 6px; padding: 5px 8px; }
</style>
