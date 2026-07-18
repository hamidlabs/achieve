<script lang="ts">
  import { onMount } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { listen } from "@tauri-apps/api/event";
  import {
    store,
    refreshAll,
    refreshTasks,
    refreshSnapshot,
    closeTaskEditor,
  } from "./lib/store.svelte";
  import { initSound } from "./lib/sound";
  import type { Snapshot, View } from "./lib/types";
  import TasksView from "./lib/views/TasksView.svelte";
  import DashboardView from "./lib/views/DashboardView.svelte";
  import NotesView from "./lib/views/NotesView.svelte";
  import BreakView from "./lib/views/BreakView.svelte";
  import BottomNav from "./lib/ui/BottomNav.svelte";
  import TaskEditor from "./lib/components/TaskEditor.svelte";
  import BreakSettingsPopover from "./lib/components/BreakSettingsPopover.svelte";

  // Bumped on every navigate so the keyed view below remounts each time the
  // engine surfaces a popup, even when the target view is unchanged. That makes
  // each view's onMount fire on every popup, which is where the cue sounds live.
  let navTick = $state(0);

  const isBreak = $derived(store.view === "break");

  onMount(() => {
    refreshAll();
    initSound();
    const unNav = listen<View>("navigate", (e) => {
      store.view = e.payload;
      navTick++;
      if (e.payload === "nudge") store.fitTick++;
    });
    const unSnap = listen<Snapshot>("snapshot", (e) => {
      store.snapshot = e.payload;
    });
    // The backend changed the task list (e.g. day rollover) — refetch it.
    const unTasks = listen("tasks-changed", () => {
      refreshTasks();
    });
    return () => {
      unNav.then((f) => f());
      unSnap.then((f) => f());
      unTasks.then((f) => f());
    };
  });

  async function onEditorSaved() {
    closeTaskEditor();
    await Promise.all([refreshTasks(), refreshSnapshot()]);
  }

  // Respect reduced-motion for the view transition (collapse to an instant swap).
  const reduce =
    typeof window !== "undefined" &&
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  const inDur = reduce ? 0 : 240;
  const outDur = reduce ? 0 : 120;
</script>

<main
  class={isBreak ? "" : "glass"}
  style="width:100%; height:100%; {isBreak ? '' : 'border-radius: var(--radius-card);'}"
>
  {#if !store.ready}
    <div class="grid place-items-center h-full text-ink-faint text-[13px]">
      Waking up…
    </div>
  {:else if isBreak}
    {#key navTick}
      <div class="h-full fade"><BreakView /></div>
    {/key}
  {:else}
    <div class="flex flex-col h-full">
      <div class="view-stack">
        {#key `${store.view}:${navTick}`}
          <div
            class="view-layer"
            in:scale={{ start: 0.985, opacity: 0, duration: inDur, easing: cubicOut }}
            out:fade={{ duration: outDur }}
          >
            {#if store.view === "dashboard"}
              <DashboardView />
            {:else if store.view === "notes"}
              <NotesView />
            {:else}
              <TasksView />
            {/if}
          </div>
        {/key}
      </div>
      <BottomNav />
    </div>
  {/if}
</main>

<!-- App-level modals: opened by the bottom nav from any view, so navigation and
     primary actions live in one place instead of per-view toolbars. -->
{#if store.editor}
  <TaskEditor
    task={store.editor.task}
    categories={store.categories}
    onSaved={onEditorSaved}
    onClose={closeTaskEditor}
  />
{/if}

{#if store.settingsOpen}
  <BreakSettingsPopover onClose={() => (store.settingsOpen = false)} />
{/if}

<style>
  /* View crossfade: incoming and outgoing layers stack in the same box so the
     fade-through never shifts layout. Each view manages its own inner scroll. */
  .view-stack {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .view-layer {
    position: absolute;
    inset: 0;
    will-change: transform, opacity;
  }
</style>
