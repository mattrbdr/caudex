<script lang="ts">
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Label } from "$lib/components/ui/label/index.js";
  import { invoke } from "@tauri-apps/api/core";
  import { documentDir, homeDir, join } from "@tauri-apps/api/path";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  type WorkspaceStep =
    | "library"
    | "import"
    | "metadata"
    | "batch"
    | "conflicts"
    | "settings";

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
    tags: string[];
    collections: string[];
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
    tags: string[];
    collections: string[];
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

  type BatchMetadataOutcome = {
    item_id: number;
    status: "updated" | "skipped" | "failed";
    reason: string | null;
    retry_eligible: boolean;
    before: LibraryItemMetadata | null;
    after: LibraryItemMetadata | null;
  };

  type BatchMetadataRunResult = {
    run_id: string;
    mode: "preview" | "execute";
    status: string;
    total_targets: number;
    updated_count: number;
    skipped_count: number;
    failed_count: number;
    outcomes: BatchMetadataOutcome[];
  };

  type ListMetadataNamesResult = {
    names: string[];
  };

  type MetadataConflictRecord = {
    id: number;
    item_id: number;
    field_name: string;
    current_value: string;
    candidate_value: string;
    candidate_source: string;
    status: string;
    rationale: string | null;
    created_at: string;
    resolved_at: string | null;
  };

  type ListMetadataConflictsResult = {
    item_id: number;
    conflicts: MetadataConflictRecord[];
  };

  type ResolveMetadataConflictResult = {
    conflict: MetadataConflictRecord;
    item: LibraryItemMetadata;
  };

  const WORKSPACE_STATE_KEY = "caudex.workspace.v1";

  let isLoading = $state(true);
  let isSubmitting = $state(false);
  let isPickingLocation = $state(false);
  let errorMessage = $state("");
  let importErrorMessage = $state("");
  let library = $state<Library | null>(null);
  let libraryName = $state("My Library");
  let libraryPath = $state("");
  let workspaceStep = $state<WorkspaceStep>("library");

  let isImporting = $state(false);
  let isBulkImporting = $state(false);
  let isRetrying = $state(false);
  let latestImport = $state<ImportJobResult | null>(null);
  let selectedRetryPaths = $state<string[]>([]);

  let metadataItems = $state<LibraryItemSummary[]>([]);
  let selectedBatchItemIds = $state<number[]>([]);
  let selectedMetadataItemId = $state<number | null>(null);
  let metadataDetail = $state<LibraryItemMetadata | null>(null);
  let metadataTitle = $state("");
  let metadataAuthors = $state("");
  let metadataLanguage = $state("");
  let metadataPublishedAt = $state("");
  let metadataTags = $state("");
  let metadataCollections = $state("");
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

  let filterAuthor = $state("");
  let filterLanguage = $state("");
  let filterTag = $state("");
  let filterCollection = $state("");
  let sortBy = $state<"id" | "title" | "author" | "language" | "published_at">("id");
  let sortDirection = $state<"asc" | "desc">("asc");

  let batchTitle = $state("");
  let batchAuthors = $state("");
  let batchLanguage = $state("");
  let batchPublishedAt = $state("");
  let batchTags = $state("");
  let batchCollections = $state("");
  let batchStatusMessage = $state("");
  let isBatchPreviewing = $state(false);
  let isBatchExecuting = $state(false);
  let batchPreviewResult = $state<BatchMetadataRunResult | null>(null);
  let batchExecuteResult = $state<BatchMetadataRunResult | null>(null);

  let availableTags = $state<string[]>([]);
  let availableCollections = $state<string[]>([]);

  let conflictRecords = $state<MetadataConflictRecord[]>([]);
  let conflictStatusMessage = $state("");
  let isConflictsLoading = $state(false);
  let resolvingConflictId = $state<number | null>(null);

  let bulkDuplicateMode = $state<"skip_duplicate" | "merge_metadata" | "force_import">(
    "skip_duplicate",
  );
  let bulkDryRun = $state(false);

  const failedItems = $derived(
    latestImport ? latestImport.items.filter((item) => item.status === "failed") : [],
  );
  const stepTitleByStep: Record<WorkspaceStep, string> = {
    library: "Library",
    import: "Import",
    metadata: "Metadata",
    batch: "Batch",
    conflicts: "Conflicts",
    settings: "Settings",
  };
  const stepHelpByStep: Record<WorkspaceStep, string> = {
    library: "Parcours les livres, filtre, puis ouvre un livre.",
    import: "Importe des fichiers ou un dossier pour alimenter la bibliothèque.",
    metadata: "Édite les métadonnées du livre sélectionné.",
    batch: "Applique des changements sur plusieurs livres en une action.",
    conflicts: "Détecte et résous les conflits de métadonnées.",
    settings: "Gère la configuration et les propositions d'enrichissement.",
  };
  const currentStepLabel = $derived(stepTitleByStep[workspaceStep]);
  const currentStepHelp = $derived(stepHelpByStep[workspaceStep]);

  function parseCsvEntries(value: string): string[] {
    return value
      .split(/[\n,]/g)
      .map((entry) => entry.trim())
      .filter((entry) => entry.length > 0);
  }

  function persistWorkspaceState() {
    if (typeof localStorage === "undefined") {
      return;
    }

    const payload = {
      step: workspaceStep,
      selected_metadata_item_id: selectedMetadataItemId,
      filters: {
        author: filterAuthor,
        language: filterLanguage,
        tag: filterTag,
        collection: filterCollection,
        sort_by: sortBy,
        sort_direction: sortDirection,
      },
    };

    localStorage.setItem(WORKSPACE_STATE_KEY, JSON.stringify(payload));
  }

  function restoreWorkspaceState() {
    if (typeof localStorage === "undefined") {
      return;
    }

    const raw = localStorage.getItem(WORKSPACE_STATE_KEY);
    if (!raw) {
      return;
    }

    try {
      const parsed = JSON.parse(raw) as {
        step?: WorkspaceStep;
        selected_metadata_item_id?: number;
        filters?: {
          author?: string;
          language?: string;
          tag?: string;
          collection?: string;
          sort_by?: "id" | "title" | "author" | "language" | "published_at";
          sort_direction?: "asc" | "desc";
        };
      };

      if (
        parsed.step === "library" ||
        parsed.step === "import" ||
        parsed.step === "metadata" ||
        parsed.step === "batch" ||
        parsed.step === "conflicts" ||
        parsed.step === "settings"
      ) {
        workspaceStep = parsed.step;
      }

      if (parsed.filters) {
        filterAuthor = parsed.filters.author ?? "";
        filterLanguage = parsed.filters.language ?? "";
        filterTag = parsed.filters.tag ?? "";
        filterCollection = parsed.filters.collection ?? "";
        sortBy = parsed.filters.sort_by ?? "id";
        sortDirection = parsed.filters.sort_direction ?? "asc";
      }

      if (Number.isFinite(parsed.selected_metadata_item_id)) {
        selectedMetadataItemId = parsed.selected_metadata_item_id ?? null;
      }
    } catch {
      // Ignore invalid persisted payload and keep defaults.
    }
  }

  function setWorkspaceStep(step: WorkspaceStep) {
    workspaceStep = step;
    persistWorkspaceState();
  }

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
      if (library) {
        restoreWorkspaceState();
        await loadMetadataItems();
        await loadMetadataTaxonomyOptions();
      }
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
      setWorkspaceStep("import");
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

  function clearDetailState() {
    metadataDetail = null;
    metadataTitle = "";
    metadataAuthors = "";
    metadataLanguage = "";
    metadataPublishedAt = "";
    metadataTags = "";
    metadataCollections = "";
    metadataEnrichmentProposals = [];
    conflictRecords = [];
  }

  async function loadMetadataTaxonomyOptions() {
    try {
      const [tagsResult, collectionsResult] = await Promise.all([
        invoke<ListMetadataNamesResult>("list_metadata_tags"),
        invoke<ListMetadataNamesResult>("list_metadata_collections"),
      ]);
      availableTags = tagsResult.names;
      availableCollections = collectionsResult.names;
    } catch {
      // Keep UX non-blocking if taxonomy cannot be loaded.
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
          author: filterAuthor.trim() === "" ? null : filterAuthor.trim(),
          language: filterLanguage.trim() === "" ? null : filterLanguage.trim(),
          published_from: null,
          published_to: null,
          tag: filterTag.trim() === "" ? null : filterTag.trim(),
          collection: filterCollection.trim() === "" ? null : filterCollection.trim(),
          sort_by: sortBy,
          sort_direction: sortDirection,
        },
      });

      metadataItems = result.items;
      selectedBatchItemIds = selectedBatchItemIds.filter((id) =>
        result.items.some((item) => item.id === id),
      );

      if (result.items.length === 0) {
        selectedMetadataItemId = null;
        clearDetailState();
        persistWorkspaceState();
        return;
      }

      if (
        selectedMetadataItemId === null ||
        !result.items.some((item) => item.id === selectedMetadataItemId)
      ) {
        selectedMetadataItemId = result.items[0].id;
      }

      await loadMetadataItemDetails(selectedMetadataItemId);
      persistWorkspaceState();
    } catch (error) {
      metadataErrorMessage =
        error instanceof Error
          ? error.message
          : "Impossible de charger la liste des métadonnées.";
    } finally {
      isMetadataListLoading = false;
    }
  }

  async function loadMetadataConflicts(itemId: number) {
    isConflictsLoading = true;
    try {
      const result = await invoke<ListMetadataConflictsResult>("list_metadata_conflicts", {
        input: {
          item_id: itemId,
          status: "pending",
        },
      });
      conflictRecords = result.conflicts;
    } catch {
      conflictRecords = [];
    } finally {
      isConflictsLoading = false;
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
      metadataTags = item.tags.join(", ");
      metadataCollections = item.collections.join(", ");
      await Promise.all([loadMetadataEnrichmentProposals(item.id), loadMetadataConflicts(item.id)]);
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
    persistWorkspaceState();
    await loadMetadataItemDetails(selectedValue);
  }

  function resetMetadataEdits() {
    if (!metadataDetail) {
      return;
    }

    metadataTitle = metadataDetail.title;
    metadataAuthors = metadataDetail.authors.join(", ");
    metadataLanguage = metadataDetail.language ?? "";
    metadataPublishedAt = metadataDetail.published_at ?? "";
    metadataTags = metadataDetail.tags.join(", ");
    metadataCollections = metadataDetail.collections.join(", ");
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
          authors: parseCsvEntries(metadataAuthors),
          language: metadataLanguage.trim() === "" ? null : metadataLanguage.trim(),
          published_at: metadataPublishedAt.trim() === "" ? null : metadataPublishedAt.trim(),
          tags: parseCsvEntries(metadataTags),
          collections: parseCsvEntries(metadataCollections),
        },
      });

      metadataDetail = updated;
      metadataTitle = updated.title;
      metadataAuthors = updated.authors.join(", ");
      metadataLanguage = updated.language ?? "";
      metadataPublishedAt = updated.published_at ?? "";
      metadataTags = updated.tags.join(", ");
      metadataCollections = updated.collections.join(", ");
      metadataSuccessMessage = "Metadata enregistrée.";
      metadataItems = metadataItems.map((item) =>
        item.id === updated.id
          ? {
              ...item,
              title: updated.title,
              authors: updated.authors,
              language: updated.language,
              published_at: updated.published_at,
              tags: updated.tags,
              collections: updated.collections,
            }
          : item,
      );
      await loadMetadataTaxonomyOptions();
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
      metadataTags = result.item.tags.join(", ");
      metadataCollections = result.item.collections.join(", ");
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
              tags: result.item.tags,
              collections: result.item.collections,
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

  function toggleBatchItem(itemId: number, checked: boolean) {
    if (checked) {
      if (!selectedBatchItemIds.includes(itemId)) {
        selectedBatchItemIds = [...selectedBatchItemIds, itemId];
      }
      return;
    }

    selectedBatchItemIds = selectedBatchItemIds.filter((id) => id !== itemId);
  }

  function selectAllVisibleMetadataItems() {
    selectedBatchItemIds = metadataItems.map((item) => item.id);
  }

  function clearBatchSelection() {
    selectedBatchItemIds = [];
  }

  async function openBookFromTable(itemId: number) {
    selectedMetadataItemId = itemId;
    persistWorkspaceState();
    await loadMetadataItemDetails(itemId);
    setWorkspaceStep("metadata");
  }

  function buildBatchPatchInput() {
    const patch: {
      title?: string;
      authors?: string[];
      language?: string;
      published_at?: string;
      tags?: string[];
      collections?: string[];
    } = {};

    if (batchTitle.trim() !== "") {
      patch.title = batchTitle.trim();
    }
    if (batchAuthors.trim() !== "") {
      patch.authors = parseCsvEntries(batchAuthors);
    }
    if (batchLanguage.trim() !== "") {
      patch.language = batchLanguage.trim();
    }
    if (batchPublishedAt.trim() !== "") {
      patch.published_at = batchPublishedAt.trim();
    }
    if (batchTags.trim() !== "") {
      patch.tags = parseCsvEntries(batchTags);
    }
    if (batchCollections.trim() !== "") {
      patch.collections = parseCsvEntries(batchCollections);
    }

    return patch;
  }

  async function previewBatchUpdate() {
    if (selectedBatchItemIds.length === 0) {
      batchStatusMessage = "Sélectionnez au moins un item pour la mise à jour batch.";
      return;
    }

    const patch = buildBatchPatchInput();
    if (Object.keys(patch).length === 0) {
      batchStatusMessage = "Renseignez au moins un champ batch avant aperçu.";
      return;
    }

    isBatchPreviewing = true;
    batchStatusMessage = "";

    try {
      batchPreviewResult = await invoke<BatchMetadataRunResult>("preview_batch_metadata_update", {
        input: {
          item_ids: selectedBatchItemIds,
          patch,
        },
      });
      batchExecuteResult = null;
      batchStatusMessage = "Aperçu batch prêt. Vérifiez les changements avant exécution.";
    } catch (error) {
      batchStatusMessage =
        error instanceof Error ? error.message : "Impossible de préparer l'aperçu batch.";
    } finally {
      isBatchPreviewing = false;
    }
  }

  async function executeBatchUpdate() {
    if (selectedBatchItemIds.length === 0) {
      batchStatusMessage = "Sélectionnez au moins un item pour la mise à jour batch.";
      return;
    }

    const patch = buildBatchPatchInput();
    if (Object.keys(patch).length === 0) {
      batchStatusMessage = "Renseignez au moins un champ batch avant exécution.";
      return;
    }

    isBatchExecuting = true;
    batchStatusMessage = "";

    try {
      batchExecuteResult = await invoke<BatchMetadataRunResult>("execute_batch_metadata_update", {
        input: {
          item_ids: selectedBatchItemIds,
          patch,
        },
      });
      batchStatusMessage = `Exécution batch terminée (${batchExecuteResult.status}).`;
      await loadMetadataItems();
      await loadMetadataTaxonomyOptions();
      if (selectedMetadataItemId) {
        await loadMetadataConflicts(selectedMetadataItemId);
      }
    } catch (error) {
      batchStatusMessage =
        error instanceof Error ? error.message : "Impossible d'exécuter la mise à jour batch.";
    } finally {
      isBatchExecuting = false;
    }
  }

  async function detectConflictsFromCurrentForm() {
    if (!selectedMetadataItemId) {
      conflictStatusMessage = "Sélectionnez un item avant de détecter les conflits.";
      return;
    }

    conflictStatusMessage = "";
    try {
      const result = await invoke<{ item_id: number; conflicts: MetadataConflictRecord[] }>(
        "detect_metadata_conflicts",
        {
          input: {
            item_id: selectedMetadataItemId,
            source: "manual_edit",
            candidate: {
              title: metadataTitle,
              authors: parseCsvEntries(metadataAuthors),
              language: metadataLanguage.trim() === "" ? null : metadataLanguage.trim(),
              published_at:
                metadataPublishedAt.trim() === "" ? null : metadataPublishedAt.trim(),
            },
          },
        },
      );

      if (result.conflicts.length === 0) {
        conflictStatusMessage = "Aucun conflit détecté pour les valeurs proposées.";
      } else {
        conflictStatusMessage = `${result.conflicts.length} conflit(s) détecté(s).`;
      }
      await loadMetadataConflicts(selectedMetadataItemId);
    } catch (error) {
      conflictStatusMessage =
        error instanceof Error ? error.message : "Impossible de détecter les conflits.";
    }
  }

  async function resolveConflict(conflictId: number, resolution: "keep_current" | "use_candidate") {
    if (!selectedMetadataItemId) {
      return;
    }

    resolvingConflictId = conflictId;
    conflictStatusMessage = "";
    try {
      const result = await invoke<ResolveMetadataConflictResult>("resolve_metadata_conflict", {
        input: {
          conflict_id: conflictId,
          resolution,
          rationale:
            resolution === "use_candidate"
              ? "Applied from explicit curator decision"
              : "Current value kept by explicit curator decision",
        },
      });

      metadataDetail = result.item;
      metadataTitle = result.item.title;
      metadataAuthors = result.item.authors.join(", ");
      metadataLanguage = result.item.language ?? "";
      metadataPublishedAt = result.item.published_at ?? "";
      metadataTags = result.item.tags.join(", ");
      metadataCollections = result.item.collections.join(", ");

      metadataItems = metadataItems.map((item) =>
        item.id === result.item.id
          ? {
              ...item,
              title: result.item.title,
              authors: result.item.authors,
              language: result.item.language,
              published_at: result.item.published_at,
              tags: result.item.tags,
              collections: result.item.collections,
            }
          : item,
      );

      await loadMetadataConflicts(selectedMetadataItemId);
      conflictStatusMessage = "Conflit résolu.";
    } catch (error) {
      conflictStatusMessage =
        error instanceof Error ? error.message : "Impossible de résoudre le conflit.";
    } finally {
      resolvingConflictId = null;
    }
  }

  function clearMetadataFilters() {
    filterAuthor = "";
    filterLanguage = "";
    filterTag = "";
    filterCollection = "";
    sortBy = "id";
    sortDirection = "asc";
    persistWorkspaceState();
  }

  onMount(() => {
    void setSuggestedLibraryPath();
    void loadLibraryState();
  });
</script>

<main class="min-h-screen bg-slate-50 text-slate-900">
  <header class="border-b border-slate-200 bg-white px-6 py-4">
    <div class="mx-auto flex w-full max-w-[1440px] items-center justify-between">
      <div>
        <h1 class="text-xl font-semibold tracking-tight">Caudex</h1>
        <p class="text-sm text-slate-600">
          Hub opérationnel bibliothèque · import · metadata · conflits
        </p>
      </div>
      {#if library}
        <div class="rounded-md border border-slate-200 bg-white px-3 py-2 text-xs text-slate-700">
          <span class="font-medium text-emerald-700">Library active:</span> {library.name}
        </div>
      {/if}
    </div>
  </header>

  {#if isLoading}
    <section class="mx-auto flex min-h-[calc(100vh-76px)] w-full max-w-[1440px] items-center justify-center px-6">
      <p role="status" class="text-slate-700">Loading library configuration...</p>
    </section>
  {:else if !library}
    <section class="mx-auto flex min-h-[calc(100vh-76px)] w-full max-w-[1440px] items-center px-6 py-10">
      <div class="grid w-full gap-8 lg:grid-cols-[1.1fr_0.9fr]">
        <div class="space-y-6">
          <div class="inline-flex rounded-full border border-blue-500/40 bg-blue-500/10 px-3 py-1 text-xs text-blue-700">
            First run setup
          </div>
          <h2 class="text-3xl font-semibold leading-tight text-slate-900">
            Crée ta library au premier démarrage, puis gère tout depuis l’interface applicative.
          </h2>
          <p class="max-w-2xl text-slate-700">
            Ensuite, la configuration library reste disponible dans <span class="font-semibold">Settings</span>.
            Toutes les actions sont séparées par vue dédiée pour éviter l’effet “tout-en-un”.
          </p>
        </div>

        <div class="rounded-2xl border border-slate-200 bg-white p-6">
          <h3 class="mb-4 text-lg font-semibold">Set up your library</h3>
          <form onsubmit={handleCreateLibrary} class="space-y-4">
            <div class="space-y-2">
              <Label for="library-name">Library name</Label>
              <Input id="library-name" name="library-name" bind:value={libraryName} placeholder="Main Library" required />
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
              <p id="library-path-help" class="text-sm text-slate-600">
                Le système ouvrira le sélecteur natif.
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
        </div>
      </div>
    </section>
  {:else}
    <div class="mx-auto min-h-[calc(100vh-76px)] w-full max-w-[1440px] px-4 py-4 lg:px-6">
      <div class="grid gap-4 lg:grid-cols-[260px_minmax(0,1fr)]">
      <aside class="rounded-xl border border-slate-200 bg-white p-4 lg:sticky lg:top-20 lg:h-fit">
        <div class="mb-4 space-y-1">
          <p class="text-xs uppercase tracking-wide text-slate-500">Workspace</p>
          <p class="text-sm text-slate-700">{library.name}</p>
          <p class="truncate text-xs text-slate-500">{library.path}</p>
        </div>

        <div class="mb-4 rounded-lg border border-slate-200 bg-slate-50 p-3">
          <p class="text-xs uppercase tracking-wide text-slate-500">Étape actuelle</p>
          <p class="text-sm font-medium text-slate-900">{currentStepLabel}</p>
          <p class="mt-1 text-xs text-slate-600">{currentStepHelp}</p>
        </div>

        <nav class="space-y-2" aria-label="Application sections">
          <Button
            type="button"
            variant={workspaceStep === "library" ? "default" : "secondary"}
            class="w-full justify-start"
            onclick={() => setWorkspaceStep("library")}
          >
            Library
          </Button>
          <Button
            type="button"
            variant={workspaceStep === "import" ? "default" : "secondary"}
            class="w-full justify-start"
            onclick={() => setWorkspaceStep("import")}
          >
            Import
          </Button>
          <Button
            type="button"
            variant={workspaceStep === "metadata" ? "default" : "secondary"}
            class="w-full justify-start"
            onclick={() => setWorkspaceStep("metadata")}
          >
            Metadata
          </Button>
          <Button
            type="button"
            variant={workspaceStep === "batch" ? "default" : "secondary"}
            class="w-full justify-start"
            onclick={() => setWorkspaceStep("batch")}
          >
            Batch
          </Button>
          <Button
            type="button"
            variant={workspaceStep === "conflicts" ? "default" : "secondary"}
            class="w-full justify-start"
            onclick={() => setWorkspaceStep("conflicts")}
          >
            Conflicts
          </Button>
          <Button
            type="button"
            variant={workspaceStep === "settings" ? "default" : "secondary"}
            class="w-full justify-start"
            onclick={() => setWorkspaceStep("settings")}
          >
            Settings
          </Button>
        </nav>
      </aside>

      <section class="overflow-auto rounded-xl border border-slate-200 bg-white p-4 md:p-5">
        <div class="mb-4 rounded-lg border border-slate-200 bg-slate-50 p-4">
          <div class="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p class="text-sm font-semibold text-slate-900">{currentStepLabel}</p>
              <p class="text-sm text-slate-600">{currentStepHelp}</p>
            </div>
            <div class="flex flex-wrap gap-2">
              {#if workspaceStep !== "library"}
                <Button type="button" variant="secondary" onclick={() => setWorkspaceStep("library")}>
                  Voir Library
                </Button>
              {/if}
              {#if workspaceStep !== "import"}
                <Button type="button" variant="secondary" onclick={() => setWorkspaceStep("import")}>
                  Aller à Import
                </Button>
              {/if}
              {#if workspaceStep !== "metadata"}
                <Button type="button" variant="secondary" onclick={() => setWorkspaceStep("metadata")}>
                  Aller à Metadata
                </Button>
              {/if}
            </div>
          </div>
        </div>

        {#if workspaceStep === "library"}
          <div class="space-y-4">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <h2 class="text-2xl font-semibold">Library</h2>
              <Button type="button" variant="secondary" onclick={loadMetadataItems} disabled={isMetadataListLoading}>
                {#if isMetadataListLoading}Chargement...{:else}Recharger la liste{/if}
              </Button>
            </div>
            <p class="text-sm text-slate-600">{metadataItems.length} livre(s) visible(s).</p>

            <div class="grid gap-2 rounded-xl border border-slate-200 bg-slate-50 p-4 md:grid-cols-6">
              <div class="md:col-span-2">
                <Label for="filter-author">Author</Label>
                <Input id="filter-author" bind:value={filterAuthor} placeholder="Alice" />
              </div>
              <div>
                <Label for="filter-language">Language</Label>
                <Input id="filter-language" bind:value={filterLanguage} placeholder="en" />
              </div>
              <div>
                <Label for="filter-tag">Tag</Label>
                <Input id="filter-tag" bind:value={filterTag} placeholder="to-read" list="tag-options" />
              </div>
              <div>
                <Label for="filter-collection">Collection</Label>
                <Input id="filter-collection" bind:value={filterCollection} placeholder="Classics" list="collection-options" />
              </div>
              <div>
                <Label for="sort-by">Sort by</Label>
                <select
                  id="sort-by"
                  class="border-input bg-background ring-offset-background flex h-10 w-full rounded-md border px-3 py-2 text-sm text-slate-900"
                  bind:value={sortBy}
                >
                  <option value="id">ID</option>
                  <option value="title">Title</option>
                  <option value="author">Author</option>
                  <option value="language">Language</option>
                  <option value="published_at">Published date</option>
                </select>
              </div>
              <div>
                <Label for="sort-direction">Direction</Label>
                <select
                  id="sort-direction"
                  class="border-input bg-background ring-offset-background flex h-10 w-full rounded-md border px-3 py-2 text-sm text-slate-900"
                  bind:value={sortDirection}
                >
                  <option value="asc">Asc</option>
                  <option value="desc">Desc</option>
                </select>
              </div>
              <div class="md:col-span-6 flex flex-wrap gap-2">
                <Button type="button" variant="secondary" onclick={() => { persistWorkspaceState(); void loadMetadataItems(); }}>
                  Appliquer filtres
                </Button>
                <Button type="button" variant="secondary" onclick={() => { clearMetadataFilters(); void loadMetadataItems(); }}>
                  Réinitialiser
                </Button>
              </div>
              <datalist id="tag-options">
                {#each availableTags as option}
                  <option value={option}></option>
                {/each}
              </datalist>
              <datalist id="collection-options">
                {#each availableCollections as option}
                  <option value={option}></option>
                {/each}
              </datalist>
            </div>

            <div class="overflow-hidden rounded-xl border border-slate-200 bg-white">
              <table class="min-w-full divide-y divide-slate-200 text-sm">
                <thead class="bg-slate-100 text-left text-xs uppercase tracking-wide text-slate-600">
                  <tr>
                    <th class="px-3 py-2">Sel</th>
                    <th class="px-3 py-2">Title</th>
                    <th class="px-3 py-2">Authors</th>
                    <th class="px-3 py-2">Lang</th>
                    <th class="px-3 py-2">Published</th>
                    <th class="px-3 py-2">Tags</th>
                    <th class="px-3 py-2">Collections</th>
                    <th class="px-3 py-2">Action</th>
                  </tr>
                </thead>
                <tbody class="divide-y divide-slate-200">
                  {#if metadataItems.length === 0}
                    <tr>
                      <td colspan="8" class="px-3 py-6 text-center text-slate-600">
                        <p>Aucun livre importé.</p>
                        <Button type="button" variant="secondary" class="mt-2" onclick={() => setWorkspaceStep("import")}>
                          Importer des livres
                        </Button>
                      </td>
                    </tr>
                  {:else}
                    {#each metadataItems as item}
                      <tr
                        class={`cursor-pointer hover:bg-slate-100 ${selectedMetadataItemId === item.id ? "bg-slate-100" : ""}`}
                        onclick={() => openBookFromTable(item.id)}
                        onkeydown={(event) => {
                          if (event.key === "Enter" || event.key === " ") {
                            event.preventDefault();
                            void openBookFromTable(item.id);
                          }
                        }}
                        tabindex="0"
                      >
                        <td class="px-3 py-2">
                          <input
                            type="checkbox"
                            checked={selectedBatchItemIds.includes(item.id)}
                            onchange={(event) => toggleBatchItem(item.id, (event.currentTarget as HTMLInputElement).checked)}
                            onclick={(event) => event.stopPropagation()}
                            aria-label={`Select metadata item ${item.id}`}
                          />
                        </td>
                        <td class="px-3 py-2 font-medium text-slate-900">{item.title}</td>
                        <td class="px-3 py-2 text-slate-700">{item.authors.join(", ")}</td>
                        <td class="px-3 py-2 text-slate-700">{item.language ?? "-"}</td>
                        <td class="px-3 py-2 text-slate-700">{item.published_at ?? "-"}</td>
                        <td class="px-3 py-2 text-slate-700">{item.tags.join(", ") || "-"}</td>
                        <td class="px-3 py-2 text-slate-700">{item.collections.join(", ") || "-"}</td>
                        <td class="px-3 py-2">
                          <Button
                            type="button"
                            variant="secondary"
                            onclick={(event) => {
                              event.stopPropagation();
                              void openBookFromTable(item.id);
                            }}
                          >
                            Ouvrir
                          </Button>
                        </td>
                      </tr>
                    {/each}
                  {/if}
                </tbody>
              </table>
            </div>
            <p class="text-sm text-slate-600">Clique sur un livre pour ouvrir l’interface metadata dédiée.</p>
          </div>
        {/if}

        {#if workspaceStep === "import"}
          <section class="space-y-4">
            <h2 class="text-2xl font-semibold">Import</h2>
            <p class="text-sm text-slate-600">
              Commence par importer, puis ouvre la vue Metadata pour éditer les livres.
            </p>
            <div class="flex flex-wrap items-center gap-2">
              <Button type="button" onclick={importSelectedFiles} disabled={isImporting || isBulkImporting} aria-busy={isImporting}>
                {#if isImporting}Import en cours...{:else}Importer des fichiers{/if}
              </Button>
              <Button
                type="button"
                variant="secondary"
                onclick={importFolderTree}
                disabled={isBulkImporting || isImporting}
                aria-busy={isBulkImporting}
              >
                {#if isBulkImporting}Scan du dossier...{:else}Importer un dossier{/if}
              </Button>
              <Button type="button" variant="secondary" onclick={() => setWorkspaceStep("metadata")}>
                Ouvrir Metadata
              </Button>
            </div>

            <div class="grid gap-2 sm:grid-cols-2">
              <div class="space-y-1">
                <Label for="bulk-mode">Bulk duplicate mode</Label>
                <select
                  id="bulk-mode"
                  class="border-input bg-background ring-offset-background flex h-10 w-full rounded-md border px-3 py-2 text-sm text-slate-900"
                  bind:value={bulkDuplicateMode}
                >
                  <option value="skip_duplicate">Skip duplicate</option>
                  <option value="merge_metadata">Merge metadata</option>
                  <option value="force_import">Force import</option>
                </select>
              </div>
              <label class="flex items-center gap-2 pt-6 text-sm text-slate-700">
                <input type="checkbox" bind:checked={bulkDryRun} />
                Dry run
              </label>
            </div>

            {#if importErrorMessage}
              <p class="text-sm font-semibold text-red-600" role="alert">{importErrorMessage}</p>
            {/if}

            {#if latestImport}
              <section class="space-y-2 rounded-xl border border-slate-200 bg-slate-50 p-4" aria-live="polite">
                <h3 class="font-semibold">Import #{latestImport.job_id}</h3>
                <p class="text-sm text-slate-600">
                  Scanned: {latestImport.scanned_count} · {latestImport.success_count} successful, {latestImport.failed_count} failed,
                  {latestImport.skipped_count} skipped
                </p>
                <div class="flex flex-wrap items-center gap-2">
                  <Button type="button" variant="secondary" onclick={() => retryFailedItems(false)} disabled={isRetrying || failedItems.length === 0}>
                    Retry Failed (All)
                  </Button>
                  <Button type="button" variant="secondary" onclick={() => retryFailedItems(true)} disabled={isRetrying || failedItems.length === 0}>
                    Retry Selected Failed
                  </Button>
                </div>
                <ul class="space-y-2 text-sm">
                  {#each latestImport.items as item}
                    <li class="rounded border border-slate-200 p-2">
                      {#if item.status === "failed"}
                        <label class="mb-1 flex items-center gap-2 text-xs">
                          <input
                            type="checkbox"
                            aria-label={`Retry ${item.source_path}`}
                            checked={selectedRetryPaths.includes(item.source_path)}
                            onchange={(event) => toggleRetryPath(item.source_path, (event.currentTarget as HTMLInputElement).checked)}
                          />
                          Select for retry
                        </label>
                      {/if}
                      <p><span class="font-medium">File:</span> {item.source_path}</p>
                      <p><span class="font-medium">Status:</span> {item.status}</p>
                      <p><span class="font-medium">Format:</span> {item.format ?? "unknown"}</p>
                      {#if item.error_message}
                        <p class="text-red-600"><span class="font-medium">Error:</span> {item.error_message}</p>
                      {/if}
                      {#if item.dedupe_decision}
                        <p><span class="font-medium">Dedupe:</span> {item.dedupe_decision}</p>
                      {/if}
                    </li>
                  {/each}
                </ul>
              </section>
            {/if}
          </section>
        {/if}

        {#if workspaceStep === "metadata"}
          <section class="space-y-4">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <h2 class="text-2xl font-semibold">Metadata Editor</h2>
              <div class="flex flex-wrap gap-2">
                <Button type="button" variant="secondary" onclick={loadMetadataItems} disabled={isMetadataListLoading}>
                  {#if isMetadataListLoading}Chargement...{:else}Charger les métadonnées{/if}
                </Button>
                <Button type="button" variant="secondary" onclick={() => setWorkspaceStep("library")}>
                  Retour à la liste
                </Button>
              </div>
            </div>
            <p class="text-sm text-slate-600">
              Sélectionne un livre à gauche, modifie les champs, puis enregistre.
            </p>

            <div class="grid gap-4 lg:grid-cols-[320px_minmax(0,1fr)]">
              <aside class="space-y-2 rounded-xl border border-slate-200 bg-slate-50 p-3">
                <Label for="metadata-item-select">Metadata item</Label>
                <select
                  id="metadata-item-select"
                  class="border-input bg-background ring-offset-background flex h-10 w-full rounded-md border px-3 py-2 text-sm text-slate-900"
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

                <ul class="max-h-[420px] space-y-1 overflow-auto rounded border border-slate-200 p-2 text-sm">
                  {#each metadataItems as item}
                    <li>
                      <button
                        type="button"
                        class={`w-full rounded px-2 py-1 text-left ${
                          selectedMetadataItemId === item.id ? "bg-slate-100 font-medium" : "hover:bg-slate-100"
                        }`}
                        onclick={() => openBookFromTable(item.id)}
                      >
                        <span class="font-medium">{item.title}</span>
                        <span class="block text-xs text-slate-600">{item.authors.join(", ")}</span>
                      </button>
                    </li>
                  {/each}
                </ul>
              </aside>

              <div class="space-y-3 rounded-xl border border-slate-200 bg-slate-50 p-4">
                <div class="grid gap-3 sm:grid-cols-2">
                  <div class="space-y-1 sm:col-span-2">
                    <Label for="metadata-title">Metadata title</Label>
                    <Input id="metadata-title" bind:value={metadataTitle} disabled={!metadataDetail} />
                  </div>
                  <div class="space-y-1 sm:col-span-2">
                    <Label for="metadata-authors">Metadata authors</Label>
                    <Input id="metadata-authors" bind:value={metadataAuthors} disabled={!metadataDetail} placeholder="Alice, Bob" />
                  </div>
                  <div class="space-y-1">
                    <Label for="metadata-language">Metadata language</Label>
                    <Input id="metadata-language" bind:value={metadataLanguage} disabled={!metadataDetail} />
                  </div>
                  <div class="space-y-1">
                    <Label for="metadata-published-at">Metadata published date</Label>
                    <Input id="metadata-published-at" bind:value={metadataPublishedAt} disabled={!metadataDetail} placeholder="YYYY-MM-DD" />
                  </div>
                  <div class="space-y-1">
                    <Label for="metadata-tags">Metadata tags</Label>
                    <Input id="metadata-tags" bind:value={metadataTags} disabled={!metadataDetail} />
                  </div>
                  <div class="space-y-1">
                    <Label for="metadata-collections">Metadata collections</Label>
                    <Input id="metadata-collections" bind:value={metadataCollections} disabled={!metadataDetail} />
                  </div>
                </div>

                <div class="flex flex-wrap items-center gap-2">
                  <Button type="button" onclick={saveMetadataEdits} disabled={!metadataDetail || isMetadataSaving}>
                    {#if isMetadataSaving}Enregistrement...{:else}Enregistrer metadata{/if}
                  </Button>
                  <Button type="button" variant="secondary" onclick={resetMetadataEdits} disabled={!metadataDetail || isMetadataSaving}>
                    Annuler modifications
                  </Button>
                  <Button type="button" variant="secondary" onclick={enrichMetadataForSelectedItem} disabled={!metadataDetail || isMetadataEnriching}>
                    {#if isMetadataEnriching}Enrichissement...{:else}Enrichir metadata{/if}
                  </Button>
                  <Button
                    type="button"
                    variant="secondary"
                    onclick={() => selectedMetadataItemId ? loadMetadataEnrichmentProposals(selectedMetadataItemId) : Promise.resolve()}
                    disabled={!metadataDetail || isMetadataProposalsLoading}
                  >
                    {#if isMetadataProposalsLoading}Rechargement...{:else}Recharger propositions{/if}
                  </Button>
                </div>

                {#if metadataErrorMessage}
                  <p class="text-sm font-semibold text-red-600" role="alert">{metadataErrorMessage}</p>
                {/if}
                {#if metadataSuccessMessage}
                  <p class="text-sm font-semibold text-emerald-700">{metadataSuccessMessage}</p>
                {/if}
                {#if metadataEnrichmentStatus}
                  <p class="text-sm font-semibold text-amber-700">{metadataEnrichmentStatus}</p>
                {/if}
              </div>
            </div>
          </section>
        {/if}

        {#if workspaceStep === "batch"}
          <section class="space-y-4">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <h2 class="text-2xl font-semibold">Batch actions</h2>
              <div class="flex gap-2">
                <Button type="button" variant="secondary" onclick={selectAllVisibleMetadataItems}>Sélectionner visibles</Button>
                <Button type="button" variant="secondary" onclick={clearBatchSelection}>Effacer sélection</Button>
              </div>
            </div>

            <p class="text-sm text-slate-600">{selectedBatchItemIds.length} item(s) sélectionné(s).</p>

            <div class="grid gap-2 rounded-xl border border-slate-200 bg-slate-50 p-4 sm:grid-cols-2 lg:grid-cols-3">
              <div class="space-y-1"><Label for="batch-title">Batch title</Label><Input id="batch-title" bind:value={batchTitle} placeholder="Nouveau titre" /></div>
              <div class="space-y-1"><Label for="batch-authors">Batch authors</Label><Input id="batch-authors" bind:value={batchAuthors} placeholder="Alice, Bob" /></div>
              <div class="space-y-1"><Label for="batch-language">Batch language</Label><Input id="batch-language" bind:value={batchLanguage} placeholder="fr" /></div>
              <div class="space-y-1"><Label for="batch-published">Batch published date</Label><Input id="batch-published" bind:value={batchPublishedAt} placeholder="YYYY-MM-DD" /></div>
              <div class="space-y-1"><Label for="batch-tags">Batch tags</Label><Input id="batch-tags" bind:value={batchTags} placeholder="tag1, tag2" /></div>
              <div class="space-y-1"><Label for="batch-collections">Batch collections</Label><Input id="batch-collections" bind:value={batchCollections} placeholder="collection1, collection2" /></div>
            </div>

            <div class="flex flex-wrap gap-2">
              <Button type="button" variant="secondary" onclick={previewBatchUpdate} disabled={isBatchPreviewing || isBatchExecuting}>
                {#if isBatchPreviewing}Aperçu...{:else}Aperçu batch{/if}
              </Button>
              <Button type="button" onclick={executeBatchUpdate} disabled={isBatchExecuting || isBatchPreviewing}>
                {#if isBatchExecuting}Exécution...{:else}Exécuter batch{/if}
              </Button>
            </div>

            {#if batchStatusMessage}
              <p class="text-sm font-semibold text-amber-700">{batchStatusMessage}</p>
            {/if}
            {#if batchPreviewResult}
              <div class="rounded border border-slate-200 bg-slate-50 p-3 text-sm">
                Preview run {batchPreviewResult.run_id} · {batchPreviewResult.updated_count} updated, {batchPreviewResult.skipped_count} skipped,
                {batchPreviewResult.failed_count} failed
              </div>
            {/if}
            {#if batchExecuteResult}
              <div class="rounded border border-slate-200 bg-slate-50 p-3 text-sm">
                <p>
                  Execute run {batchExecuteResult.run_id} · {batchExecuteResult.updated_count} updated, {batchExecuteResult.skipped_count}
                  skipped, {batchExecuteResult.failed_count} failed
                </p>
                <ul class="mt-2 space-y-1">
                  {#each batchExecuteResult.outcomes as outcome}
                    <li>
                      #{outcome.item_id} · {outcome.status}
                      {#if outcome.reason} · {outcome.reason}{/if}
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
          </section>
        {/if}

        {#if workspaceStep === "conflicts"}
          <section class="space-y-4">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <h2 class="text-2xl font-semibold">Conflict resolution</h2>
              <Button type="button" variant="secondary" onclick={detectConflictsFromCurrentForm}>Détecter conflits</Button>
            </div>

            {#if conflictStatusMessage}
              <p class="text-sm font-semibold text-amber-700">{conflictStatusMessage}</p>
            {/if}

            {#if isConflictsLoading}
              <p class="text-sm text-slate-600">Chargement des conflits...</p>
            {:else if conflictRecords.length === 0}
              <p class="text-sm text-slate-600">Aucun conflit en attente.</p>
            {:else}
              <ul class="space-y-2 text-sm">
                {#each conflictRecords as conflict}
                  <li class="rounded border border-slate-200 bg-slate-50 p-3">
                    <p><span class="font-medium">Field:</span> {conflict.field_name} · <span class="font-medium">Source:</span> {conflict.candidate_source}</p>
                    <p><span class="font-medium">Current:</span> {conflict.current_value}</p>
                    <p><span class="font-medium">Candidate:</span> {conflict.candidate_value}</p>
                    <div class="mt-2 flex flex-wrap gap-2">
                      <Button
                        type="button"
                        variant="secondary"
                        onclick={() => resolveConflict(conflict.id, "keep_current")}
                        disabled={resolvingConflictId === conflict.id}
                      >
                        Garder actuel
                      </Button>
                      <Button
                        type="button"
                        onclick={() => resolveConflict(conflict.id, "use_candidate")}
                        disabled={resolvingConflictId === conflict.id}
                      >
                        Appliquer candidat
                      </Button>
                    </div>
                  </li>
                {/each}
              </ul>
            {/if}
          </section>
        {/if}

        {#if workspaceStep === "settings"}
          <section class="space-y-4">
            <h2 class="text-2xl font-semibold">Settings</h2>
            <div class="rounded-xl border border-slate-200 bg-slate-50 p-4">
              <h3 class="mb-2 text-lg font-semibold">Library configuration</h3>
              <p class="text-sm text-slate-700"><span class="font-medium">Name:</span> {library.name}</p>
              <p class="text-sm text-slate-700"><span class="font-medium">Path:</span> {library.path}</p>
              <p class="text-sm text-slate-600">La création de la library se fait au premier démarrage. Les réglages se gèrent ici ensuite.</p>
            </div>

            <div class="rounded-xl border border-slate-200 bg-slate-50 p-4">
              <h3 class="mb-2 text-lg font-semibold">Metadata enrichment proposals</h3>
              {#if metadataEnrichmentProposals.length === 0}
                <p class="text-sm text-slate-600">No proposal yet.</p>
              {:else}
                <ul class="space-y-2">
                  {#each metadataEnrichmentProposals as proposal}
                    <li class="rounded border border-slate-200 p-2 text-sm">
                      <p><span class="font-medium">Provider:</span> {proposal.provider}</p>
                      <p><span class="font-medium">Confidence:</span> {proposal.confidence.toFixed(2)}</p>
                      {#if proposal.title}
                        <p><span class="font-medium">Title:</span> {proposal.title}</p>
                      {/if}
                      {#if proposal.authors.length > 0}
                        <p><span class="font-medium">Authors:</span> {proposal.authors.join(", ")}</p>
                      {/if}
                      {#if proposal.applied_at}
                        <p class="text-emerald-700"><span class="font-medium">Applied at:</span> {proposal.applied_at}</p>
                      {:else}
                        <Button
                          type="button"
                          variant="secondary"
                          onclick={() => applyMetadataEnrichmentProposal(proposal.id)}
                          disabled={applyingProposalId === proposal.id}
                        >
                          {#if applyingProposalId === proposal.id}Application...{:else}Appliquer proposition{/if}
                        </Button>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          </section>
        {/if}

        {#if errorMessage}
          <p class="mt-4 text-sm font-semibold text-red-600" role="alert">{errorMessage}</p>
        {/if}
      </section>
      </div>
    </div>
  {/if}
</main>
