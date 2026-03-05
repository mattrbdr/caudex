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
  let latestImport = $state<ImportJobResult | null>(null);
  let bulkDuplicateMode = $state<"skip_duplicate" | "merge_metadata" | "force_import">(
    "skip_duplicate",
  );
  let bulkDryRun = $state(false);

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
    } catch (error) {
      importErrorMessage =
        error instanceof Error
          ? error.message
          : "Impossible de lancer l'import de dossier. Vérifiez la sélection et réessayez.";
    } finally {
      isBulkImporting = false;
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
                <ul class="space-y-2 text-sm">
                  {#each latestImport.items as item}
                    <li class="rounded border p-2">
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
