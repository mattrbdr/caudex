<script lang="ts">
  import { Button } from "$lib/components/ui/button/index.js";
  import {
    Card,
    CardContent,
    CardDescription,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Label } from "$lib/components/ui/label/index.js";
  import { invoke } from "@tauri-apps/api/core";
  import { documentDir, homeDir, join } from "@tauri-apps/api/path";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  type Library = {
    id: number;
    name: string;
    path: string;
    created_at: string;
  };

  type ImportFileStatus = "queued" | "running" | "success" | "failed" | "skipped";

  type ImportFileResult = {
    source_path: string;
    status: ImportFileStatus;
    format: "epub" | "mobi" | "pdf" | null;
    title: string | null;
    error_message?: string | null;
    dedupe_decision?: "skip_duplicate" | "merge_metadata" | "force_import" | null;
  };

  type ImportJobResult = {
    job_id: number;
    status: "success" | "partial_success" | "failed";
    scanned_count: number;
    processed_count: number;
    success_count: number;
    failed_count: number;
    skipped_count: number;
    items: ImportFileResult[];
  };

  type LibraryItemSummary = {
    id: number;
    title: string;
    authors: string[];
    language: string | null;
    published_at: string | null;
    format: string;
    source_path: string;
  };

  type ListLibraryItemsResult = {
    page: number;
    page_size: number;
    total: number;
    items: LibraryItemSummary[];
  };

  type LibraryItemMetadata = {
    id: number;
    title: string;
    authors: string[];
    language: string | null;
    published_at: string | null;
    format: string;
    source_path: string;
  };

  type MetadataEnrichmentProposal = {
    id: number;
    provider: string;
    confidence: number;
    title: string | null;
    authors: string[];
    language: string | null;
    published_at: string | null;
    diagnostic: string | null;
    applied_at: string | null;
  };

  type ListMetadataEnrichmentProposalsResult = {
    item_id: number;
    proposals: MetadataEnrichmentProposal[];
  };

  type EnrichmentRunResult = {
    run_id: number;
    status: string;
    diagnostic: string | null;
    proposals: MetadataEnrichmentProposal[];
  };

  type ApplyMetadataEnrichmentProposalResult = {
    proposal_id: number;
    item: LibraryItemMetadata;
  };

  let isLoading = $state(true);
  let isSubmitting = $state(false);
  let isPickingLocation = $state(false);
  let errorMessage = $state("");
  let importErrorMessage = $state("");
  let library = $state<Library | null>(null);
  let libraryName = $state("My Library");
  let libraryPath = $state("");
  let isImporting = $state(false);
  let isBulkImporting = $state(false);
  let isRetrying = $state(false);
  let latestImport = $state<ImportJobResult | null>(null);
  let selectedRetryPaths = $state<string[]>([]);
  let metadataItems = $state<LibraryItemSummary[]>([]);
  let selectedMetadataItemId = $state<number | null>(null);
  let metadataDetail = $state<LibraryItemMetadata | null>(null);
  let metadataTitle = $state("");
  let metadataAuthors = $state("");
  let metadataLanguage = $state("");
  let metadataPublishedAt = $state("");
  let metadataErrorMessage = $state("");
  let metadataSuccessMessage = $state("");
  let isMetadataListLoading = $state(false);
  let isMetadataDetailLoading = $state(false);
  let isMetadataSaving = $state(false);
  let isMetadataEnriching = $state(false);
  let isMetadataProposalsLoading = $state(false);
  let applyingProposalId = $state<number | null>(null);
  let metadataEnrichmentProposals = $state<MetadataEnrichmentProposal[]>([]);
  let metadataEnrichmentStatus = $state("");
  let bulkDuplicateMode = $state<"skip_duplicate" | "merge_metadata" | "force_import">(
    "skip_duplicate",
  );
  let bulkDryRun = $state(false);
  const failedItems = $derived(
    latestImport ? latestImport.items.filter((item) => item.status === "failed") : [],
  );

  async function setSuggestedLibraryPath() {
    if (libraryPath.trim() !== "" || library !== null) {
      return;
    }

    try {
      const docsDir = await documentDir();
      libraryPath = await join(docsDir, "Caudex");
    } catch {
      try {
        const home = await homeDir();
        libraryPath = await join(home, "Documents", "Caudex");
      } catch {
        libraryPath =
          navigator.userAgent.includes("Windows") ? "C:\\Caudex" : "/tmp/Caudex";
      }
    }
  }

  async function loadLibraryState() {
    isLoading = true;
    errorMessage = "";

    try {
      library = await invoke<Library | null>("get_library");
    } catch (error) {
      errorMessage =
        error instanceof Error
          ? error.message
          : "Unable to load library state. Please try again.";
    } finally {
      isLoading = false;
    }
  }

  async function chooseLibraryPath() {
    isPickingLocation = true;
    errorMessage = "";

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Choisir un emplacement",
        defaultPath: libraryPath,
      });

      if (selected === null) {
        return;
      }

      if (typeof selected !== "string") {
        errorMessage =
          "Le sélecteur a retourné une valeur invalide. Merci de réessayer.";
        return;
      }

      libraryPath = selected;
    } catch (error) {
      errorMessage =
        error instanceof Error
          ? `Impossible d'ouvrir le sélecteur de dossier: ${error.message}`
          : "Impossible d'ouvrir le sélecteur de dossier. Vérifiez les autorisations du système.";
    } finally {
      isPickingLocation = false;
    }
  }

  async function handleCreateLibrary(event: Event) {
    event.preventDefault();
    if (!libraryPath.trim()) {
      errorMessage = "Choisissez un emplacement de bibliothèque avant de continuer.";
      return;
    }

    isSubmitting = true;
    errorMessage = "";

    try {
      library = await invoke<Library>("create_library", {
        input: {
          name: libraryName,
          path: libraryPath,
        },
      });
    } catch (error) {
      errorMessage =
        error instanceof Error
          ? error.message
          : "Unable to create library. Check the input values and retry.";
    } finally {
      isSubmitting = false;
    }
  }

  async function importSelectedFiles() {
    isImporting = true;
    importErrorMessage = "";

    try {
      const selected = await open({
        directory: false,
        multiple: true,
        title: "Sélectionner des ebooks",
        filters: [
          {
            name: "Ebooks",
            extensions: ["epub", "mobi", "pdf"],
          },
        ],
      });

      if (selected === null) {
        return;
      }

      const paths =
        typeof selected === "string"
          ? [selected]
          : selected.filter((value): value is string => typeof value === "string");

      if (paths.length === 0) {
        importErrorMessage = "Aucun fichier valide n'a été sélectionné.";
        return;
      }

      latestImport = await invoke<ImportJobResult>("start_import", {
        input: {
          paths,
        },
      });
      selectedRetryPaths = [];
    } catch (error) {
      importErrorMessage =
        error instanceof Error
          ? error.message
          : "Impossible de lancer l'import. Vérifiez la sélection et réessayez.";
    } finally {
      isImporting = false;
    }
  }

  async function importFolderTree() {
    isBulkImporting = true;
    importErrorMessage = "";

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Sélectionner un dossier à importer",
      });

      if (selected === null) {
        return;
      }

      if (typeof selected !== "string") {
        importErrorMessage = "Le sélecteur de dossier a retourné une valeur invalide.";
        return;
      }

      latestImport = await invoke<ImportJobResult>("start_bulk_import", {
        input: {
          root_path: selected,
          duplicate_mode: bulkDuplicateMode,
          dry_run: bulkDryRun,
        },
      });
      selectedRetryPaths = [];
    } catch (error) {
      importErrorMessage =
        error instanceof Error
          ? error.message
          : "Impossible de lancer l'import de dossier. Vérifiez la sélection et réessayez.";
    } finally {
      isBulkImporting = false;
    }
  }

  function toggleRetryPath(path: string, checked: boolean) {
    if (checked) {
      if (!selectedRetryPaths.includes(path)) {
        selectedRetryPaths = [...selectedRetryPaths, path];
      }
      return;
    }
    selectedRetryPaths = selectedRetryPaths.filter((candidate) => candidate !== path);
  }

  async function retryFailedItems(retrySelectedOnly: boolean) {
    if (!latestImport) {
      return;
    }
    if (failedItems.length === 0) {
      importErrorMessage = "Aucun élément en échec à relancer.";
      return;
    }

    const sourcePaths: string[] | null = retrySelectedOnly ? selectedRetryPaths : null;
    if (retrySelectedOnly && selectedRetryPaths.length === 0) {
      importErrorMessage = "Sélectionnez au moins un élément en échec à relancer.";
      return;
    }

    isRetrying = true;
    importErrorMessage = "";
    try {
      latestImport = await invoke<ImportJobResult>("start_import_retry", {
        input: {
          job_id: latestImport.job_id,
          source_paths: sourcePaths,
        },
      });
      selectedRetryPaths = [];
    } catch (error) {
      importErrorMessage =
        error instanceof Error
          ? error.message
          : "Impossible de relancer les éléments en échec. Réessayez.";
    } finally {
      isRetrying = false;
    }
  }

  async function loadMetadataItems() {
    isMetadataListLoading = true;
    metadataErrorMessage = "";
    metadataSuccessMessage = "";

    try {
      const result = await invoke<ListLibraryItemsResult>("list_library_items", {
        input: {
          page: 1,
          page_size: 50,
        },
      });

      metadataItems = result.items;
      if (result.items.length === 0) {
        selectedMetadataItemId = null;
        metadataDetail = null;
        return;
      }

      selectedMetadataItemId = result.items[0].id;
      await loadMetadataItemDetails(result.items[0].id);
    } catch (error) {
      metadataErrorMessage =
        error instanceof Error
          ? error.message
          : "Impossible de charger la liste des métadonnées.";
    } finally {
      isMetadataListLoading = false;
    }
  }

  async function loadMetadataItemDetails(itemId: number) {
    isMetadataDetailLoading = true;
    metadataErrorMessage = "";
    metadataSuccessMessage = "";

    try {
      const item = await invoke<LibraryItemMetadata>("get_library_item_metadata", {
        input: {
          item_id: itemId,
        },
      });

      metadataDetail = item;
      metadataTitle = item.title;
      metadataAuthors = item.authors.join(", ");
      metadataLanguage = item.language ?? "";
      metadataPublishedAt = item.published_at ?? "";
      await loadMetadataEnrichmentProposals(item.id);
    } catch (error) {
      metadataErrorMessage =
        error instanceof Error
          ? error.message
          : "Impossible de charger les détails metadata.";
    } finally {
      isMetadataDetailLoading = false;
    }
  }

  async function loadMetadataEnrichmentProposals(itemId: number) {
    isMetadataProposalsLoading = true;
    metadataEnrichmentStatus = "";
    try {
      const result = await invoke<ListMetadataEnrichmentProposalsResult>(
        "list_metadata_enrichment_proposals",
        {
          input: {
            item_id: itemId,
          },
        },
      );
      metadataEnrichmentProposals = result.proposals;
    } catch (error) {
      metadataEnrichmentStatus =
        error instanceof Error
          ? error.message
          : "Impossible de charger les propositions d'enrichissement.";
    } finally {
      isMetadataProposalsLoading = false;
    }
  }

  async function onMetadataItemChange(event: Event) {
    const selectedValue = Number((event.currentTarget as HTMLSelectElement).value);
    if (!Number.isFinite(selectedValue)) {
      return;
    }

    selectedMetadataItemId = selectedValue;
    await loadMetadataItemDetails(selectedValue);
  }

  function parseMetadataAuthors(value: string): string[] {
    return value
      .split(/[\n,]/g)
      .map((entry) => entry.trim())
      .filter((entry) => entry.length > 0);
  }

  function resetMetadataEdits() {
    if (!metadataDetail) {
      return;
    }

    metadataTitle = metadataDetail.title;
    metadataAuthors = metadataDetail.authors.join(", ");
    metadataLanguage = metadataDetail.language ?? "";
    metadataPublishedAt = metadataDetail.published_at ?? "";
    metadataErrorMessage = "";
    metadataEnrichmentStatus = "";
    metadataSuccessMessage = "Modifications annulées.";
  }

  async function saveMetadataEdits() {
    if (!selectedMetadataItemId) {
      metadataErrorMessage = "Sélectionnez un item avant d'enregistrer.";
      return;
    }

    isMetadataSaving = true;
    metadataErrorMessage = "";
    metadataSuccessMessage = "";

    try {
      const updated = await invoke<LibraryItemMetadata>("update_library_item_metadata", {
        input: {
          item_id: selectedMetadataItemId,
          title: metadataTitle,
          authors: parseMetadataAuthors(metadataAuthors),
          language: metadataLanguage.trim() === "" ? null : metadataLanguage.trim(),
          published_at: metadataPublishedAt.trim() === "" ? null : metadataPublishedAt.trim(),
        },
      });

      metadataDetail = updated;
      metadataTitle = updated.title;
      metadataAuthors = updated.authors.join(", ");
      metadataLanguage = updated.language ?? "";
      metadataPublishedAt = updated.published_at ?? "";
      metadataSuccessMessage = "Metadata enregistrée.";
      metadataItems = metadataItems.map((item) =>
        item.id === updated.id
          ? {
              ...item,
              title: updated.title,
              authors: updated.authors,
              language: updated.language,
              published_at: updated.published_at,
            }
          : item,
      );
    } catch (error) {
      metadataErrorMessage =
        error instanceof Error
          ? error.message
          : "Impossible d'enregistrer les métadonnées.";
    } finally {
      isMetadataSaving = false;
    }
  }

  async function enrichMetadataForSelectedItem() {
    if (!selectedMetadataItemId) {
      metadataEnrichmentStatus = "Sélectionnez un item avant de lancer l'enrichissement.";
      return;
    }

    isMetadataEnriching = true;
    metadataEnrichmentStatus = "";
    metadataErrorMessage = "";
    metadataSuccessMessage = "";
    try {
      const result = await invoke<EnrichmentRunResult>("enrich_library_item_metadata", {
        input: {
          item_id: selectedMetadataItemId,
        },
      });
      metadataEnrichmentProposals = result.proposals;
      if (result.status === "failed") {
        metadataEnrichmentStatus =
          result.diagnostic ?? "Aucune proposition d'enrichissement disponible.";
      } else {
        metadataEnrichmentStatus =
          result.diagnostic ??
          `Enrichissement terminé (${result.status}) avec ${result.proposals.length} proposition(s).`;
      }
    } catch (error) {
      metadataEnrichmentStatus =
        error instanceof Error
          ? error.message
          : "Impossible de lancer l'enrichissement metadata.";
    } finally {
      isMetadataEnriching = false;
    }
  }

  async function applyMetadataEnrichmentProposal(proposalId: number) {
    if (!selectedMetadataItemId) {
      metadataEnrichmentStatus = "Sélectionnez un item avant d'appliquer une proposition.";
      return;
    }

    applyingProposalId = proposalId;
    metadataEnrichmentStatus = "";
    metadataErrorMessage = "";
    metadataSuccessMessage = "";
    try {
      const result = await invoke<ApplyMetadataEnrichmentProposalResult>(
        "apply_metadata_enrichment_proposal",
        {
          input: {
            proposal_id: proposalId,
          },
        },
      );

      metadataDetail = result.item;
      metadataTitle = result.item.title;
      metadataAuthors = result.item.authors.join(", ");
      metadataLanguage = result.item.language ?? "";
      metadataPublishedAt = result.item.published_at ?? "";
      metadataSuccessMessage = "Proposition appliquée avec succès.";
      await loadMetadataEnrichmentProposals(selectedMetadataItemId);
      metadataItems = metadataItems.map((item) =>
        item.id === result.item.id
          ? {
              ...item,
              title: result.item.title,
              authors: result.item.authors,
              language: result.item.language,
              published_at: result.item.published_at,
            }
          : item,
      );
    } catch (error) {
      metadataEnrichmentStatus =
        error instanceof Error
          ? error.message
          : "Impossible d'appliquer la proposition.";
    } finally {
      applyingProposalId = null;
    }
  }

  onMount(() => {
    void setSuggestedLibraryPath();
    void loadLibraryState();
  });
</script>

<main class="min-h-screen bg-linear-to-br from-slate-100 via-slate-50 to-amber-50 px-4 py-8">
  <div class="mx-auto flex min-h-[75vh] w-full max-w-3xl items-center">
    <Card class="w-full shadow-xl">
      <CardHeader>
        <CardTitle class="text-2xl">Caudex First Run</CardTitle>
        <CardDescription>
          Configure your first local library before starting imports.
        </CardDescription>
      </CardHeader>
      <CardContent class="space-y-5">
        {#if isLoading}
          <p role="status" class="text-muted-foreground">Loading library configuration...</p>
        {:else if library}
          <section class="space-y-3">
            <h2 class="text-lg font-semibold">Library ready</h2>
            <p><span class="font-medium">Name:</span> {library.name}</p>
            <p><span class="font-medium">Path:</span> {library.path}</p>
            <p><span class="font-medium">Created:</span> {library.created_at}</p>

            <div class="pt-2">
              <div class="flex flex-wrap items-center gap-2">
                <Button
                  type="button"
                  onclick={importSelectedFiles}
                  disabled={isImporting || isBulkImporting}
                  aria-busy={isImporting}
                >
                  {#if isImporting}
                    Import en cours...
                  {:else}
                    Importer des fichiers
                  {/if}
                </Button>

                <Button
                  type="button"
                  variant="secondary"
                  onclick={importFolderTree}
                  disabled={isBulkImporting || isImporting}
                  aria-busy={isBulkImporting}
                >
                  {#if isBulkImporting}
                    Scan du dossier...
                  {:else}
                    Importer un dossier
                  {/if}
                </Button>
              </div>
            </div>

            <div class="grid gap-2 sm:grid-cols-2">
              <div class="space-y-1">
                <Label for="bulk-mode">Bulk duplicate mode</Label>
                <select
                  id="bulk-mode"
                  class="border-input bg-background ring-offset-background flex h-10 w-full rounded-md border px-3 py-2 text-sm"
                  bind:value={bulkDuplicateMode}
                >
                  <option value="skip_duplicate">Skip duplicate</option>
                  <option value="merge_metadata">Merge metadata</option>
                  <option value="force_import">Force import</option>
                </select>
              </div>
              <label class="flex items-center gap-2 pt-6 text-sm">
                <input type="checkbox" bind:checked={bulkDryRun} />
                Dry run
              </label>
            </div>

            <section class="space-y-3 rounded-lg border p-3">
              <h3 class="font-semibold">Single-item metadata editing</h3>
              <div class="flex flex-wrap items-center gap-2">
                <Button
                  type="button"
                  variant="secondary"
                  onclick={loadMetadataItems}
                  disabled={isMetadataListLoading}
                >
                  {#if isMetadataListLoading}
                    Chargement...
                  {:else}
                    Charger les métadonnées
                  {/if}
                </Button>
              </div>

              <div class="space-y-2">
                <Label for="metadata-item-select">Metadata item</Label>
                <select
                  id="metadata-item-select"
                  class="border-input bg-background ring-offset-background flex h-10 w-full rounded-md border px-3 py-2 text-sm"
                  disabled={metadataItems.length === 0 || isMetadataDetailLoading}
                  onchange={onMetadataItemChange}
                  value={selectedMetadataItemId ?? ""}
                >
                  {#if metadataItems.length === 0}
                    <option value="">No metadata item loaded</option>
                  {:else}
                    {#each metadataItems as item}
                      <option value={item.id}>{item.title} (#{item.id})</option>
                    {/each}
                  {/if}
                </select>
              </div>

              <div class="grid gap-3 sm:grid-cols-2">
                <div class="space-y-1 sm:col-span-2">
                  <Label for="metadata-title">Metadata title</Label>
                  <Input id="metadata-title" bind:value={metadataTitle} disabled={!metadataDetail} />
                </div>
                <div class="space-y-1 sm:col-span-2">
                  <Label for="metadata-authors">Metadata authors</Label>
                  <Input
                    id="metadata-authors"
                    bind:value={metadataAuthors}
                    disabled={!metadataDetail}
                    placeholder="Alice, Bob"
                  />
                </div>
                <div class="space-y-1">
                  <Label for="metadata-language">Metadata language</Label>
                  <Input id="metadata-language" bind:value={metadataLanguage} disabled={!metadataDetail} />
                </div>
                <div class="space-y-1">
                  <Label for="metadata-published-at">Metadata published date</Label>
                  <Input
                    id="metadata-published-at"
                    bind:value={metadataPublishedAt}
                    disabled={!metadataDetail}
                    placeholder="YYYY-MM-DD"
                  />
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-2">
                <Button
                  type="button"
                  onclick={saveMetadataEdits}
                  disabled={!metadataDetail || isMetadataSaving}
                >
                  {#if isMetadataSaving}
                    Enregistrement...
                  {:else}
                    Enregistrer metadata
                  {/if}
                </Button>
                <Button
                  type="button"
                  variant="secondary"
                  onclick={resetMetadataEdits}
                  disabled={!metadataDetail || isMetadataSaving}
                >
                  Annuler modifications
                </Button>
                <Button
                  type="button"
                  variant="secondary"
                  onclick={enrichMetadataForSelectedItem}
                  disabled={!metadataDetail || isMetadataEnriching}
                >
                  {#if isMetadataEnriching}
                    Enrichissement...
                  {:else}
                    Enrichir metadata
                  {/if}
                </Button>
                <Button
                  type="button"
                  variant="secondary"
                  onclick={() =>
                    selectedMetadataItemId
                      ? loadMetadataEnrichmentProposals(selectedMetadataItemId)
                      : Promise.resolve()}
                  disabled={!metadataDetail || isMetadataProposalsLoading}
                >
                  {#if isMetadataProposalsLoading}
                    Rechargement...
                  {:else}
                    Recharger propositions
                  {/if}
                </Button>
              </div>

              {#if metadataErrorMessage}
                <p class="text-sm font-semibold text-red-700" role="alert">{metadataErrorMessage}</p>
              {/if}
              {#if metadataSuccessMessage}
                <p class="text-sm font-semibold text-emerald-700">{metadataSuccessMessage}</p>
              {/if}
              {#if metadataEnrichmentStatus}
                <p class="text-sm font-semibold text-amber-700">{metadataEnrichmentStatus}</p>
              {/if}

              <div class="space-y-2 rounded-md border p-3">
                <h4 class="font-semibold">Metadata enrichment proposals</h4>
                {#if metadataEnrichmentProposals.length === 0}
                  <p class="text-sm text-muted-foreground">No proposal yet.</p>
                {:else}
                  <ul class="space-y-2">
                    {#each metadataEnrichmentProposals as proposal}
                      <li class="rounded border p-2 text-sm">
                        <p><span class="font-medium">Provider:</span> {proposal.provider}</p>
                        <p><span class="font-medium">Confidence:</span> {proposal.confidence.toFixed(2)}</p>
                        {#if proposal.title}
                          <p><span class="font-medium">Title:</span> {proposal.title}</p>
                        {/if}
                        {#if proposal.authors.length > 0}
                          <p><span class="font-medium">Authors:</span> {proposal.authors.join(", ")}</p>
                        {/if}
                        {#if proposal.language}
                          <p><span class="font-medium">Language:</span> {proposal.language}</p>
                        {/if}
                        {#if proposal.published_at}
                          <p><span class="font-medium">Published:</span> {proposal.published_at}</p>
                        {/if}
                        {#if proposal.diagnostic}
                          <p class="text-amber-700">
                            <span class="font-medium">Diagnostic:</span> {proposal.diagnostic}
                          </p>
                        {/if}
                        {#if proposal.applied_at}
                          <p class="text-emerald-700">
                            <span class="font-medium">Applied at:</span> {proposal.applied_at}
                          </p>
                        {:else}
                          <Button
                            type="button"
                            variant="secondary"
                            onclick={() => applyMetadataEnrichmentProposal(proposal.id)}
                            disabled={applyingProposalId === proposal.id}
                          >
                            {#if applyingProposalId === proposal.id}
                              Application...
                            {:else}
                              Appliquer proposition
                            {/if}
                          </Button>
                        {/if}
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            </section>

            {#if importErrorMessage}
              <p class="text-sm font-semibold text-red-700" role="alert">{importErrorMessage}</p>
            {/if}

            {#if latestImport}
              <section class="space-y-2 rounded-lg border p-3" aria-live="polite">
                <h3 class="font-semibold">Import #{latestImport.job_id}</h3>
                <p class="text-sm text-muted-foreground">
                  Scanned: {latestImport.scanned_count} · {latestImport.success_count} successful, {latestImport.failed_count} failed,
                  {latestImport.skipped_count} skipped
                </p>
                <div class="flex flex-wrap items-center gap-2">
                  <Button
                    type="button"
                    variant="secondary"
                    onclick={() => retryFailedItems(false)}
                    disabled={isRetrying || failedItems.length === 0}
                  >
                    Retry Failed (All)
                  </Button>
                  <Button
                    type="button"
                    variant="secondary"
                    onclick={() => retryFailedItems(true)}
                    disabled={isRetrying || failedItems.length === 0}
                  >
                    Retry Selected Failed
                  </Button>
                </div>
                <ul class="space-y-2 text-sm">
                  {#each latestImport.items as item}
                    <li class="rounded border p-2">
                      {#if item.status === "failed"}
                        <label class="mb-1 flex items-center gap-2 text-xs">
                          <input
                            type="checkbox"
                            aria-label={`Retry ${item.source_path}`}
                            checked={selectedRetryPaths.includes(item.source_path)}
                            onchange={(event) =>
                              toggleRetryPath(
                                item.source_path,
                                (event.currentTarget as HTMLInputElement).checked,
                              )}
                          />
                          Select for retry
                        </label>
                      {/if}
                      <p><span class="font-medium">File:</span> {item.source_path}</p>
                      <p><span class="font-medium">Status:</span> {item.status}</p>
                      <p><span class="font-medium">Format:</span> {item.format ?? "unknown"}</p>
                      {#if item.error_message}
                        <p class="text-red-700">
                          <span class="font-medium">Error:</span> {item.error_message}
                        </p>
                      {/if}
                      {#if item.dedupe_decision}
                        <p>
                          <span class="font-medium">Dedupe:</span> {item.dedupe_decision}
                        </p>
                      {/if}
                    </li>
                  {/each}
                </ul>
              </section>
            {/if}
          </section>
        {:else}
          <section class="space-y-4">
            <h2 class="text-lg font-semibold">Set up your library</h2>
            <p class="text-muted-foreground">
              Create your first library to begin importing and managing books.
            </p>

            <form onsubmit={handleCreateLibrary} class="space-y-4">
              <div class="space-y-2">
                <Label for="library-name">Library name</Label>
                <Input
                  id="library-name"
                  name="library-name"
                  bind:value={libraryName}
                  placeholder="Main Library"
                  required
                />
              </div>

              <div class="space-y-2">
                <Label for="library-path">Library path</Label>
                <div class="flex flex-col gap-2 sm:flex-row">
                  <Input
                    id="library-path"
                    name="library-path"
                    value={libraryPath}
                    placeholder="No folder selected"
                    readonly
                    aria-describedby="library-path-help"
                    class="flex-1"
                  />
                  <Button
                    type="button"
                    variant="secondary"
                    onclick={chooseLibraryPath}
                    disabled={isSubmitting || isPickingLocation}
                  >
                    {#if isPickingLocation}
                      Ouverture...
                    {:else}
                      Choisir un emplacement
                    {/if}
                  </Button>
                </div>
                <p id="library-path-help" class="text-muted-foreground text-sm">
                  Le système ouvrira le sélecteur natif. Si l'accès est refusé, autorisez Caudex
                  dans les réglages de confidentialité puis réessayez.
                </p>
              </div>

              <Button type="submit" disabled={isSubmitting} class="w-full sm:w-auto">
                {#if isSubmitting}
                  Creating library...
                {:else}
                  Create library
                {/if}
              </Button>
            </form>
          </section>
        {/if}

        {#if errorMessage}
          <p class="text-sm font-semibold text-red-700" role="alert">{errorMessage}</p>
        {/if}
      </CardContent>
    </Card>
  </div>
</main>
