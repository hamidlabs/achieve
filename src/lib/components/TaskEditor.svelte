<script lang="ts">
  import Icon from "../icons/Icon.svelte";
  import MarkdownEditor from "../ui/MarkdownEditor.svelte";
  import { api } from "../api";
  import { refreshCategories } from "../store.svelte";
  import { catColor } from "../format";
  import type { Category, Task } from "../types";

  interface Props {
    task: Task | null;
    categories: Category[];
    onSaved: () => void;
    onClose: () => void;
  }
  let { task, categories, onSaved, onClose }: Props = $props();

  let title = $state(task?.title ?? "");
  let body = $state(task?.body_md ?? "");
  let categoryId = $state<number | null>(
    task?.category_id ?? categories[0]?.id ?? null,
  );
  let estimate = $state<number | null>(task?.estimate_min ?? 30);
  let daily = $state(task?.recurrence === "daily");
  let saving = $state(false);

  const presets = [15, 30, 45, 60, 90, 120];

  // inline new-category creation
  const palette = [
    "#6b6bff", "#37b6a6", "#e0a23c", "#c06bd8",
    "#e36b6b", "#4f9be0", "#5bc46b", "#df7bb0",
  ];
  let addingCat = $state(false);
  let managingCats = $state(false);
  let newCatName = $state("");
  let newCatColor = $state(palette[0]);

  async function deleteCategory(c: Category) {
    if (
      !confirm(
        `Delete "${c.name}"? Tasks in it are kept and become uncategorized.`,
      )
    )
      return;
    try {
      await api.deleteCategory(c.id);
      await refreshCategories();
      if (categoryId === c.id) categoryId = categories[0]?.id ?? null;
    } catch (e) {
      console.error("[achieve] delete category failed:", e);
      alert("Could not delete the category: " + e);
    }
  }

  async function createCategory() {
    const name = newCatName.trim();
    if (!name) return;
    try {
      const id = await api.createCategory(name, newCatColor);
      await refreshCategories();
      categoryId = id;
    } catch (e) {
      console.error("[achieve] create category failed:", e);
      alert("Could not create the category: " + e);
    }
    addingCat = false;
    newCatName = "";
    newCatColor = palette[0];
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
</script>

<!-- Full-bleed opaque panel (no dark scrim) so it reads as a clean form,
     not a floating modal with a dark frame around it. -->
<div
  class="absolute inset-0 z-20 flex flex-col fade"
  style="background: linear-gradient(160deg, var(--glass-top), var(--glass-bot));"
>
  <div class="flex items-center gap-2 px-4 py-3 border-b shrink-0"
    style="border-color: var(--line);">
    <Icon name={task ? "pencil" : "plus"} size={16} class="text-accent" />
    <span class="text-[13px] font-semibold">{task ? "Edit task" : "New task"}</span>
    <button class="icon-btn ml-auto" onclick={onClose}><Icon name="x" size={16} /></button>
  </div>

  <div class="flex-1 overflow-y-auto">
    <div class="max-w-[480px] mx-auto px-4 py-3.5 flex flex-col gap-3.5">
      <input
        class="field text-[14px] font-medium"
        style="user-select:text;"
        placeholder="What needs doing?"
        bind:value={title}
        onkeydown={(e) => e.key === "Enter" && (e.metaKey || e.ctrlKey) && save()}
      />

      <!-- Category -->
      <div>
        <div class="flex items-center justify-between mb-1.5">
          <span class="text-[11px] font-medium text-ink-faint">Category</span>
          {#if categories.length > 0}
            <button
              class="text-[11px] text-ink-faint hover:text-ink transition flex items-center gap-1"
              onclick={() => {
                managingCats = !managingCats;
                addingCat = false;
              }}
            >
              <Icon name={managingCats ? "check" : "settings"} size={12} />
              {managingCats ? "Done" : "Manage"}
            </button>
          {/if}
        </div>
        <div class="flex flex-wrap gap-1.5 items-center">
          {#each categories as c (c.id)}
            <div
              class="chip transition"
              style={categoryId === c.id && !managingCats
                ? `background: ${catColor(c.color)}; color: white;`
                : ""}
            >
              <button
                class="flex items-center gap-1.5"
                disabled={managingCats}
                onclick={() => (categoryId = c.id)}
              >
                <span class="w-2 h-2 rounded-full" style="background: {categoryId === c.id && !managingCats ? 'white' : catColor(c.color)};"></span>
                {c.name}
              </button>
              {#if managingCats}
                <button
                  class="ml-1 -mr-1 grid place-items-center w-4 h-4 rounded-full hover:bg-black/5 transition"
                  style="color: var(--color-danger);"
                  title="Delete category"
                  onclick={() => deleteCategory(c)}
                >
                  <Icon name="x" size={11} />
                </button>
              {/if}
            </div>
          {/each}
          {#if !managingCats}
            <button
              class="chip transition"
              style="border: 1px dashed var(--line-strong);"
              onclick={() => (addingCat = !addingCat)}
            >
              <Icon name={addingCat ? "x" : "plus"} size={12} /> New
            </button>
          {/if}
        </div>

        {#if addingCat}
          <div class="mt-2 panel rounded-[var(--radius-md)] p-2.5 flex flex-col gap-2 fade">
            <div class="flex items-center gap-2">
              <span class="w-3.5 h-3.5 rounded-full shrink-0" style="background: {newCatColor};"></span>
              <input
                class="field no-drag"
                style="padding:6px 9px; user-select:text;"
                placeholder="New category name…"
                bind:value={newCatName}
                onkeydown={(e) => e.key === "Enter" && createCategory()}
              />
              <button
                class="btn btn-primary"
                style="padding:6px 12px;"
                disabled={!newCatName.trim()}
                onclick={createCategory}
              >
                Add
              </button>
            </div>
            <div class="flex flex-wrap gap-1.5">
              {#each palette as col (col)}
                <button
                  class="w-5 h-5 rounded-full transition"
                  style="background: {col}; outline: {newCatColor === col
                    ? '2px solid white'
                    : '2px solid transparent'}; outline-offset: 1px;"
                  aria-label="color"
                  onclick={() => (newCatColor = col)}
                ></button>
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <!-- Estimate -->
      <div>
        <div class="text-[11px] font-medium text-ink-faint mb-1.5 flex items-center gap-1.5">
          <Icon name="hourglass" size={12} /> Estimate
        </div>
        <div class="flex flex-wrap gap-1.5 items-center">
          {#each presets as p (p)}
            <button
              class="chip transition"
              style={estimate === p ? "background: var(--color-accent); color: white;" : ""}
              onclick={() => (estimate = p)}
            >
              {p < 60 ? `${p}m` : `${p / 60}h`.replace(".5", "½")}
            </button>
          {/each}
          <input
            type="number"
            class="field no-drag"
            style="width:74px; padding:4px 8px; user-select:text;"
            placeholder="min"
            bind:value={estimate}
          />
        </div>
      </div>

      <!-- Recurrence -->
      <button
        class="flex items-center gap-2.5 text-left"
        onclick={() => (daily = !daily)}
      >
        <span
          class="w-9 h-5 rounded-full transition relative shrink-0"
          style={daily
            ? "background: var(--color-accent);"
            : "background: color-mix(in oklab, var(--color-ink) 18%, transparent);"}
        >
          <span
            class="absolute top-0.5 w-4 h-4 rounded-full bg-white transition-all"
            style={daily ? "left: 18px;" : "left: 2px;"}
          ></span>
        </span>
        <span class="text-[12.5px] text-ink flex items-center gap-1.5">
          <Icon name="repeat" size={14} /> Repeat daily
        </span>
      </button>

      <!-- Body markdown -->
      <div>
        <div class="text-[11px] font-medium text-ink-faint mb-1.5">Details</div>
        <MarkdownEditor bind:value={body} rows={5} />
      </div>
    </div>
  </div>

  <div class="px-4 py-3 flex items-center gap-2 border-t shrink-0"
    style="border-color: var(--line);">
      {#if task}
        <button class="btn btn-danger-ghost" onclick={remove}>
          <Icon name="trash" size={15} /> Delete
        </button>
      {/if}
      <div class="flex-1"></div>
      <button class="btn btn-soft" onclick={onClose}>Cancel</button>
      <button class="btn btn-primary" onclick={save} disabled={!title.trim()}>
        <Icon name="check" size={15} /> {task ? "Save" : "Add task"}
      </button>
  </div>
</div>
