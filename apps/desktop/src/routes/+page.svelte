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

  let isLoading = $state(true);
  let isSubmitting = $state(false);
  let isPickingLocation = $state(false);
  let errorMessage = $state("");
  let library = $state<Library | null>(null);
  let libraryName = $state("My Library");
  let libraryPath = $state("");

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
          <section class="space-y-2">
            <h2 class="text-lg font-semibold">Library ready</h2>
            <p><span class="font-medium">Name:</span> {library.name}</p>
            <p><span class="font-medium">Path:</span> {library.path}</p>
            <p><span class="font-medium">Created:</span> {library.created_at}</p>
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
