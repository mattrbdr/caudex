<script lang="ts">
  export let author = "";
  export let language = "";
  export let tag = "";
  export let collection = "";
  export let sortBy: "id" | "title" | "author" | "language" | "published_at" = "id";
  export let sortDirection: "asc" | "desc" = "asc";
  export let onApply: () => void = () => {};
  export let onReset: () => void = () => {};
</script>

<div class="grid gap-2 rounded-xl border border-slate-700 bg-slate-900/70 p-4 md:grid-cols-6">
  <div class="md:col-span-2">
    <label for="filter-author" class="mb-1 block text-sm">Author</label>
    <input id="filter-author" class="w-full rounded border px-3 py-2 text-sm text-slate-900" bind:value={author} />
  </div>
  <div>
    <label for="filter-language" class="mb-1 block text-sm">Language</label>
    <input id="filter-language" class="w-full rounded border px-3 py-2 text-sm text-slate-900" bind:value={language} />
  </div>
  <div>
    <label for="filter-tag" class="mb-1 block text-sm">Tag</label>
    <input id="filter-tag" class="w-full rounded border px-3 py-2 text-sm text-slate-900" bind:value={tag} />
  </div>
  <div>
    <label for="filter-collection" class="mb-1 block text-sm">Collection</label>
    <input id="filter-collection" class="w-full rounded border px-3 py-2 text-sm text-slate-900" bind:value={collection} />
  </div>
  <div>
    <label for="sort-by" class="mb-1 block text-sm">Sort by</label>
    <select id="sort-by" class="w-full rounded border px-3 py-2 text-sm text-slate-900" bind:value={sortBy}>
      <option value="id">ID</option>
      <option value="title">Title</option>
      <option value="author">Author</option>
      <option value="language">Language</option>
      <option value="published_at">Published date</option>
    </select>
  </div>
  <div>
    <label for="sort-direction" class="mb-1 block text-sm">Direction</label>
    <select id="sort-direction" class="w-full rounded border px-3 py-2 text-sm text-slate-900" bind:value={sortDirection}>
      <option value="asc">Asc</option>
      <option value="desc">Desc</option>
    </select>
  </div>
  <div class="md:col-span-6 flex gap-2">
    <button type="button" class="rounded border border-slate-600 px-3 py-2 text-sm" onclick={onApply}>Appliquer filtres</button>
    <button type="button" class="rounded border border-slate-600 px-3 py-2 text-sm" onclick={onReset}>Réinitialiser</button>
  </div>
</div>
