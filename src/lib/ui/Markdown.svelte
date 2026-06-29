<script lang="ts">
  import { marked } from "marked";

  interface Props {
    source: string;
  }
  let { source }: Props = $props();

  marked.setOptions({ gfm: true, breaks: true });

  // GitHub-style task list checkboxes, rendered read-only.
  const html = $derived(
    source?.trim()
      ? (marked.parse(source, { async: false }) as string)
      : "",
  );
</script>

{#if html}
  <div class="md">
    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
    {@html html}
  </div>
{:else}
  <div class="md text-ink-ghost italic">No details.</div>
{/if}
