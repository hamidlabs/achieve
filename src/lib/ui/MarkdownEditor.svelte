<script lang="ts">
  import Icon from "../icons/Icon.svelte";
  import Markdown from "./Markdown.svelte";

  interface Props {
    value: string;
    placeholder?: string;
    rows?: number;
  }
  let {
    value = $bindable(),
    placeholder = "Add details… markdown supported",
    rows = 5,
  }: Props = $props();

  let mode = $state<"write" | "preview">("write");
  let ta: HTMLTextAreaElement | null = $state(null);

  type Tool = { icon: string; title: string; run: () => void };

  function surround(before: string, after = before) {
    if (!ta) return;
    const s = ta.selectionStart;
    const e = ta.selectionEnd;
    const sel = value.slice(s, e) || "";
    value = value.slice(0, s) + before + sel + after + value.slice(e);
    queueMicrotask(() => {
      ta!.focus();
      ta!.selectionStart = s + before.length;
      ta!.selectionEnd = e + before.length;
    });
  }

  function linePrefix(prefix: string) {
    if (!ta) return;
    const s = ta.selectionStart;
    const lineStart = value.lastIndexOf("\n", s - 1) + 1;
    value = value.slice(0, lineStart) + prefix + value.slice(lineStart);
    queueMicrotask(() => {
      ta!.focus();
      ta!.selectionStart = ta!.selectionEnd = s + prefix.length;
    });
  }

  const tools: Tool[] = [
    { icon: "heading", title: "Heading", run: () => linePrefix("## ") },
    { icon: "bold", title: "Bold", run: () => surround("**") },
    { icon: "italic", title: "Italic", run: () => surround("*") },
    { icon: "list", title: "List", run: () => linePrefix("- ") },
    { icon: "square-check", title: "Checklist", run: () => linePrefix("- [ ] ") },
    { icon: "code", title: "Code", run: () => surround("`") },
    { icon: "link", title: "Link", run: () => surround("[", "](url)") },
  ];
</script>

<div class="rounded-[var(--radius-md)] panel overflow-hidden">
  <div
    class="flex items-center gap-0.5 px-1.5 py-1 border-b"
    style="border-color: color-mix(in oklab, var(--color-ink) 8%, transparent);"
  >
    {#each tools as t (t.icon)}
      <button
        class="icon-btn no-drag"
        style="width:28px;height:28px;"
        title={t.title}
        disabled={mode === "preview"}
        onclick={t.run}
      >
        <Icon name={t.icon} size={15} />
      </button>
    {/each}
    <div class="flex-1"></div>
    <button
      class="px-2.5 py-1 rounded-lg text-[11px] font-medium no-drag transition"
      style={mode === "preview"
        ? "background: color-mix(in oklab, var(--color-accent) 16%, white); color: var(--color-accent);"
        : "color: var(--color-ink-faint);"}
      onclick={() => (mode = mode === "write" ? "preview" : "write")}
    >
      <span class="inline-flex items-center gap-1.5">
        <Icon name={mode === "preview" ? "pencil" : "eye"} size={13} />
        {mode === "preview" ? "Edit" : "Preview"}
      </span>
    </button>
  </div>

  {#if mode === "write"}
    <textarea
      bind:this={ta}
      bind:value
      {rows}
      {placeholder}
      class="w-full resize-none bg-transparent outline-none px-3 py-2.5 text-[12.5px] leading-relaxed text-ink no-drag"
      style="user-select:text;"
    ></textarea>
  {:else}
    <div class="px-3 py-2.5 min-h-[100px]">
      <Markdown source={value} />
    </div>
  {/if}
</div>
