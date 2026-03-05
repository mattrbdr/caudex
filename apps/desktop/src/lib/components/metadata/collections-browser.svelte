<script lang="ts">
  export let tags: string[] = [];
  export let collections: string[] = [];
</script>

<section class="grid gap-3 sm:grid-cols-2">
  <div class="rounded border border-slate-700 bg-slate-900/70 p-3">
    <h3 class="mb-2 text-sm font-semibold">Tags</h3>
    {#if tags.length === 0}
      <p class="text-sm text-slate-400">Aucun tag.</p>
    {:else}
      <ul class="space-y-1 text-sm">
        {#each tags as tag}
          <li>{tag}</li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="rounded border border-slate-700 bg-slate-900/70 p-3">
    <h3 class="mb-2 text-sm font-semibold">Collections</h3>
    {#if collections.length === 0}
      <p class="text-sm text-slate-400">Aucune collection.</p>
    {:else}
      <ul class="space-y-1 text-sm">
        {#each collections as collection}
          <li>{collection}</li>
        {/each}
      </ul>
    {/if}
  </div>
</section>
