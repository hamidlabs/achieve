<script lang="ts">
  // The persistent bottom navigation for the whole hub: icon tabs + one raised
  // violet "Add" FAB. It drives navigation and the shared modals from any view,
  // so the top of each screen stays a calm greeting/title bar with no toolbar.
  import Icon from "../icons/Icon.svelte";
  import { store, go, openTaskEditor } from "../store.svelte";
  import type { View } from "../types";

  // "nudge" and "dashboard" are the two real destinations; the FAB and the gear
  // open modals rather than navigating.
  const current = $derived(store.view);
  function nav(v: View) {
    if (v !== current) go(v);
  }
</script>

<nav class="bottom-nav no-drag" aria-label="Primary">
  <button class="nav-item" class:on={current === "nudge"} onclick={() => nav("nudge")} aria-current={current === "nudge"}>
    <span class="nav-ico"><Icon name="home" size={19} /></span>
    Today
  </button>

  <button class="nav-item" class:on={current === "dashboard"} onclick={() => nav("dashboard")} aria-current={current === "dashboard"}>
    <span class="nav-ico"><Icon name="chart-column" size={19} /></span>
    Insights
  </button>

  <button class="nav-fab" title="Add a task" aria-label="Add a task" onclick={() => openTaskEditor(null)}>
    <Icon name="plus" size={24} />
  </button>

  <button class="nav-item" class:on={current === "notes"} onclick={() => nav("notes")} aria-current={current === "notes"}>
    <span class="nav-ico"><Icon name="book-open" size={18} /></span>
    Journal
  </button>

  <button class="nav-item" class:on={store.settingsOpen} onclick={() => (store.settingsOpen = true)}>
    <span class="nav-ico"><Icon name="settings" size={18} /></span>
    Breaks
  </button>
</nav>
