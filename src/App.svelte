<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { store, refreshAll } from "./lib/store.svelte";
  import { initSound } from "./lib/sound";
  import type { Snapshot, View } from "./lib/types";
  import TasksView from "./lib/views/TasksView.svelte";
  import DashboardView from "./lib/views/DashboardView.svelte";
  import BreakView from "./lib/views/BreakView.svelte";

  onMount(() => {
    refreshAll();
    initSound();
    const unNav = listen<View>("navigate", (e) => {
      store.view = e.payload;
      // The tasks hub auto-fits its height to content; nudge it to re-measure
      // whenever the engine surfaces it (it opens at the default "nudge" size).
      if (e.payload === "nudge") store.fitTick++;
    });
    const unSnap = listen<Snapshot>("snapshot", (e) => {
      store.snapshot = e.payload;
    });
    return () => {
      unNav.then((f) => f());
      unSnap.then((f) => f());
    };
  });
</script>

<main
  class="glass"
  style="width:100%; height:100%; border-radius: var(--radius-card);"
>
  {#if !store.ready}
    <div class="grid place-items-center h-full text-ink-faint text-[13px]">
      Waking up…
    </div>
  {:else}
    {#key store.view}
      <div class="h-full rise">
        {#if store.view === "dashboard"}
          <DashboardView />
        {:else if store.view === "break"}
          <BreakView />
        {:else}
          <TasksView />
        {/if}
      </div>
    {/key}
  {/if}
</main>
