<script lang="ts">
  type Conflict = {
    id: number;
    field_name: string;
    current_value: string;
    candidate_value: string;
    candidate_source: string;
  };

  export let conflicts: Conflict[] = [];
  export let isLoading = false;
  export let statusMessage = "";
  export let resolvingConflictId: number | null = null;
  export let onResolve: (conflictId: number, resolution: "keep_current" | "use_candidate") => void =
    () => {};
</script>

<section class="space-y-4">
  {#if statusMessage}
    <p class="text-sm font-semibold text-amber-300">{statusMessage}</p>
  {/if}

  {#if isLoading}
    <p class="text-sm text-slate-400">Chargement des conflits...</p>
  {:else if conflicts.length === 0}
    <p class="text-sm text-slate-400">Aucun conflit en attente.</p>
  {:else}
    <ul class="space-y-2 text-sm">
      {#each conflicts as conflict}
        <li class="rounded border border-slate-700 bg-slate-900/70 p-3">
          <p>
            <span class="font-medium">Field:</span> {conflict.field_name} ·
            <span class="font-medium">Source:</span> {conflict.candidate_source}
          </p>
          <p><span class="font-medium">Current:</span> {conflict.current_value}</p>
          <p><span class="font-medium">Candidate:</span> {conflict.candidate_value}</p>
          <div class="mt-2 flex flex-wrap gap-2">
            <button
              type="button"
              class="rounded border border-slate-600 px-3 py-1 hover:bg-slate-800"
              onclick={() => onResolve(conflict.id, "keep_current")}
              disabled={resolvingConflictId === conflict.id}
            >
              Garder actuel
            </button>
            <button
              type="button"
              class="rounded border border-slate-600 px-3 py-1 hover:bg-slate-800"
              onclick={() => onResolve(conflict.id, "use_candidate")}
              disabled={resolvingConflictId === conflict.id}
            >
              Appliquer candidat
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>
