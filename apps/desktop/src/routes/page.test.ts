import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Page from "./+page.svelte";

const invokeMock = vi.fn();
const openMock = vi.fn();
const documentDirMock = vi.fn();
const homeDirMock = vi.fn();
const joinMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openMock(...args),
}));

vi.mock("@tauri-apps/api/path", () => ({
  documentDir: (...args: unknown[]) => documentDirMock(...args),
  homeDir: (...args: unknown[]) => homeDirMock(...args),
  join: (...args: unknown[]) => joinMock(...args),
}));

describe("first-run library setup", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    openMock.mockReset();
    documentDirMock.mockReset();
    homeDirMock.mockReset();
    joinMock.mockReset();

    documentDirMock.mockResolvedValue("/Users/test/Documents");
    homeDirMock.mockResolvedValue("/Users/test");
    joinMock.mockResolvedValue("/Users/test/Documents/Caudex");
  });

  it("shows first-run wizard when no library exists", async () => {
    invokeMock.mockResolvedValueOnce(null);
    render(Page);

    const heading = await screen.findByRole("heading", {
      name: /set up your library/i,
    });

    expect(heading).toBeTruthy();
  });

  it("falls back to an absolute default path when document directory is unavailable", async () => {
    documentDirMock.mockRejectedValueOnce(new Error("unavailable"));
    homeDirMock.mockRejectedValueOnce(new Error("home unavailable"));
    invokeMock.mockResolvedValueOnce(null);
    openMock.mockResolvedValueOnce("/tmp/caudex-library");

    render(Page);

    const chooseLocationButton = await screen.findByRole("button", {
      name: /choisir un emplacement/i,
    });

    await fireEvent.click(chooseLocationButton);

    expect(openMock).toHaveBeenNthCalledWith(1, {
      directory: true,
      multiple: false,
      title: "Choisir un emplacement",
      defaultPath: "/tmp/Caudex",
    });
  });

  it("keeps the flow usable when picker is cancelled", async () => {
    invokeMock.mockResolvedValueOnce(null);
    openMock.mockResolvedValueOnce(null);

    render(Page);

    const chooseLocationButton = await screen.findByRole("button", {
      name: /choisir un emplacement/i,
    });
    await fireEvent.click(chooseLocationButton);

    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows actionable error when picker permission is denied", async () => {
    invokeMock.mockResolvedValueOnce(null);
    openMock.mockRejectedValueOnce(new Error("permission denied"));

    render(Page);

    const chooseLocationButton = await screen.findByRole("button", {
      name: /choisir un emplacement/i,
    });
    await fireEvent.click(chooseLocationButton);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent?.toLowerCase()).toContain("impossible d'ouvrir le sélecteur de dossier");
  });

  it("shows actionable error when picker returns invalid value", async () => {
    invokeMock.mockResolvedValueOnce(null);
    openMock.mockResolvedValueOnce(["/tmp/a", "/tmp/b"]);

    render(Page);

    const chooseLocationButton = await screen.findByRole("button", {
      name: /choisir un emplacement/i,
    });
    await fireEvent.click(chooseLocationButton);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent?.toLowerCase()).toContain("valeur invalide");
  });

  it("creates the library and shows configured state", async () => {
    invokeMock.mockResolvedValueOnce(null);
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    openMock.mockResolvedValueOnce("/tmp/caudex-library");

    render(Page);

    const nameInput = await screen.findByLabelText(/library name/i);
    const chooseLocationButton = screen.getByRole("button", {
      name: /choisir un emplacement/i,
    });
    const button = screen.getByRole("button", { name: /create library/i });

    await fireEvent.input(nameInput, { target: { value: "Main Library" } });
    await fireEvent.click(chooseLocationButton);
    await fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: /library ready/i })).toBeTruthy();
    });

    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_library");
    expect(openMock).toHaveBeenNthCalledWith(1, {
      directory: true,
      multiple: false,
      title: "Choisir un emplacement",
      defaultPath: "/Users/test/Documents/Caudex",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "create_library", {
      input: {
        name: "Main Library",
        path: "/tmp/caudex-library",
      },
    });
  });

  it("imports selected files and displays per-file outcomes", async () => {
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    openMock.mockResolvedValueOnce(["/tmp/good.epub", "/tmp/bad.txt"]);
    invokeMock.mockResolvedValueOnce({
      job_id: 77,
      status: "partial_success",
      processed_count: 2,
      success_count: 1,
      failed_count: 1,
      skipped_count: 0,
      items: [
        {
          source_path: "/tmp/good.epub",
          status: "success",
          format: "epub",
          title: "good",
          error_message: null,
        },
        {
          source_path: "/tmp/bad.txt",
          status: "failed",
          format: null,
          title: null,
          error_message: "Unsupported file format.",
        },
      ],
    });

    render(Page);

    const importButton = await screen.findByRole("button", {
      name: /importer des fichiers/i,
    });
    await fireEvent.click(importButton);

    expect(openMock).toHaveBeenNthCalledWith(1, {
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
    expect(invokeMock).toHaveBeenNthCalledWith(2, "start_import", {
      input: {
        paths: ["/tmp/good.epub", "/tmp/bad.txt"],
      },
    });

    expect(await screen.findByText(/1 successful/i)).toBeTruthy();
    expect(screen.getByText(/1 failed/i)).toBeTruthy();
    expect(screen.getByText("/tmp/good.epub")).toBeTruthy();
    expect(screen.getByText("/tmp/bad.txt")).toBeTruthy();
    expect(screen.getByText("Unsupported file format.")).toBeTruthy();
  });

  it("imports a folder tree and shows duplicate decision diagnostics", async () => {
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    openMock.mockResolvedValueOnce("/tmp/library-tree");
    invokeMock.mockResolvedValueOnce({
      job_id: 88,
      status: "partial_success",
      scanned_count: 4,
      processed_count: 4,
      success_count: 2,
      failed_count: 1,
      skipped_count: 1,
      items: [
        {
          source_path: "/tmp/library-tree/a.pdf",
          status: "success",
          format: "pdf",
          title: "a",
          error_message: null,
          dedupe_decision: null,
        },
        {
          source_path: "/tmp/library-tree/nested/b.pdf",
          status: "skipped",
          format: "pdf",
          title: "b",
          error_message: "Duplicate content detected and skipped.",
          dedupe_decision: "skip_duplicate",
        },
      ],
    });

    render(Page);

    const importFolderButton = await screen.findByRole("button", {
      name: /importer un dossier/i,
    });
    await fireEvent.click(importFolderButton);

    expect(openMock).toHaveBeenNthCalledWith(1, {
      directory: true,
      multiple: false,
      title: "Sélectionner un dossier à importer",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "start_bulk_import", {
      input: {
        root_path: "/tmp/library-tree",
        duplicate_mode: "skip_duplicate",
        dry_run: false,
      },
    });

    expect(await screen.findByText(/Scanned: 4/i)).toBeTruthy();
    expect(screen.getByText(/Dedupe:/i)).toBeTruthy();
    expect(screen.getByText("skip_duplicate")).toBeTruthy();
  });

  it("retries all failed items from the current import result", async () => {
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    openMock.mockResolvedValueOnce(["/tmp/good.epub", "/tmp/bad.txt"]);
    invokeMock.mockResolvedValueOnce({
      job_id: 91,
      status: "partial_success",
      scanned_count: 2,
      processed_count: 2,
      success_count: 1,
      failed_count: 1,
      skipped_count: 0,
      items: [
        {
          source_path: "/tmp/good.epub",
          status: "success",
          format: "epub",
          title: "good",
          error_message: null,
        },
        {
          source_path: "/tmp/bad.txt",
          status: "failed",
          format: null,
          title: null,
          error_message: "Unsupported file format.",
        },
      ],
    });
    invokeMock.mockResolvedValueOnce({
      job_id: 92,
      status: "success",
      scanned_count: 1,
      processed_count: 1,
      success_count: 1,
      failed_count: 0,
      skipped_count: 0,
      items: [
        {
          source_path: "/tmp/bad.txt",
          status: "success",
          format: "pdf",
          title: "bad",
          error_message: null,
        },
      ],
    });

    render(Page);

    const importButton = await screen.findByRole("button", {
      name: /importer des fichiers/i,
    });
    await fireEvent.click(importButton);

    const retryAllButton = await screen.findByRole("button", {
      name: /retry failed \(all\)/i,
    });
    await fireEvent.click(retryAllButton);

    expect(invokeMock).toHaveBeenNthCalledWith(3, "start_import_retry", {
      input: {
        job_id: 91,
        source_paths: null,
      },
    });
    expect(await screen.findByText(/Import #92/i)).toBeTruthy();
  });

  it("retries only selected failed items", async () => {
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    openMock.mockResolvedValueOnce(["/tmp/good.epub", "/tmp/bad-a.txt", "/tmp/bad-b.txt"]);
    invokeMock.mockResolvedValueOnce({
      job_id: 101,
      status: "partial_success",
      scanned_count: 3,
      processed_count: 3,
      success_count: 1,
      failed_count: 2,
      skipped_count: 0,
      items: [
        {
          source_path: "/tmp/good.epub",
          status: "success",
          format: "epub",
          title: "good",
          error_message: null,
        },
        {
          source_path: "/tmp/bad-a.txt",
          status: "failed",
          format: null,
          title: null,
          error_message: "Unsupported file format.",
        },
        {
          source_path: "/tmp/bad-b.txt",
          status: "failed",
          format: null,
          title: null,
          error_message: "Unsupported file format.",
        },
      ],
    });
    invokeMock.mockResolvedValueOnce({
      job_id: 102,
      status: "partial_success",
      scanned_count: 1,
      processed_count: 1,
      success_count: 0,
      failed_count: 1,
      skipped_count: 0,
      items: [
        {
          source_path: "/tmp/bad-a.txt",
          status: "failed",
          format: null,
          title: null,
          error_message: "Still invalid.",
        },
      ],
    });

    render(Page);

    const importButton = await screen.findByRole("button", {
      name: /importer des fichiers/i,
    });
    await fireEvent.click(importButton);

    const badACheckbox = await screen.findByRole("checkbox", {
      name: /retry \/tmp\/bad-a\.txt/i,
    });
    await fireEvent.click(badACheckbox);

    const retrySelectedButton = await screen.findByRole("button", {
      name: /retry selected failed/i,
    });
    await fireEvent.click(retrySelectedButton);

    expect(invokeMock).toHaveBeenNthCalledWith(3, "start_import_retry", {
      input: {
        job_id: 101,
        source_paths: ["/tmp/bad-a.txt"],
      },
    });
  });

  it("loads metadata list, opens one item, and saves edited metadata", async () => {
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    invokeMock.mockResolvedValueOnce({
      page: 1,
      page_size: 50,
      total: 1,
      items: [
        {
          id: 12,
          title: "Old Title",
          authors: ["Alice"],
          language: "en",
          published_at: "2024-01-01",
          format: "epub",
          source_path: "/tmp/book.epub",
        },
      ],
    });
    invokeMock.mockResolvedValueOnce({
      id: 12,
      title: "Old Title",
      authors: ["Alice"],
      language: "en",
      published_at: "2024-01-01",
      format: "epub",
      source_path: "/tmp/book.epub",
    });
    invokeMock.mockResolvedValueOnce({
      item_id: 12,
      proposals: [],
    });
    invokeMock.mockResolvedValueOnce({
      id: 12,
      title: "New Title",
      authors: ["Alice", "Bob"],
      language: "fr",
      published_at: "2024-12-31",
      format: "epub",
      source_path: "/tmp/book.epub",
    });

    render(Page);

    const loadMetadataButton = await screen.findByRole("button", {
      name: /charger les métadonnées/i,
    });
    await fireEvent.click(loadMetadataButton);

    const titleInput = await screen.findByLabelText(/metadata title/i);
    const authorsInput = screen.getByLabelText(/metadata authors/i);
    const languageInput = screen.getByLabelText(/metadata language/i);
    const publishedAtInput = screen.getByLabelText(/metadata published date/i);

    await fireEvent.input(titleInput, { target: { value: "New Title" } });
    await fireEvent.input(authorsInput, { target: { value: "Alice, Bob" } });
    await fireEvent.input(languageInput, { target: { value: "fr" } });
    await fireEvent.input(publishedAtInput, { target: { value: "2024-12-31" } });

    const saveButton = screen.getByRole("button", { name: /enregistrer metadata/i });
    await fireEvent.click(saveButton);

    expect(invokeMock).toHaveBeenNthCalledWith(2, "list_library_items", {
      input: {
        page: 1,
        page_size: 50,
      },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "get_library_item_metadata", {
      input: {
        item_id: 12,
      },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(4, "list_metadata_enrichment_proposals", {
      input: {
        item_id: 12,
      },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(5, "update_library_item_metadata", {
      input: {
        item_id: 12,
        title: "New Title",
        authors: ["Alice", "Bob"],
        language: "fr",
        published_at: "2024-12-31",
      },
    });

    expect(await screen.findByText(/metadata enregistrée/i)).toBeTruthy();
  });

  it("resets metadata form values when cancel is clicked", async () => {
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    invokeMock.mockResolvedValueOnce({
      page: 1,
      page_size: 50,
      total: 1,
      items: [
        {
          id: 12,
          title: "Old Title",
          authors: ["Alice"],
          language: "en",
          published_at: "2024-01-01",
          format: "epub",
          source_path: "/tmp/book.epub",
        },
      ],
    });
    invokeMock.mockResolvedValueOnce({
      id: 12,
      title: "Old Title",
      authors: ["Alice"],
      language: "en",
      published_at: "2024-01-01",
      format: "epub",
      source_path: "/tmp/book.epub",
    });
    invokeMock.mockResolvedValueOnce({
      item_id: 12,
      proposals: [],
    });

    render(Page);

    const loadMetadataButton = await screen.findByRole("button", {
      name: /charger les métadonnées/i,
    });
    await fireEvent.click(loadMetadataButton);

    const titleInput = await screen.findByLabelText(/metadata title/i);
    await fireEvent.input(titleInput, { target: { value: "Draft title" } });

    const cancelButton = screen.getByRole("button", { name: /annuler modifications/i });
    await fireEvent.click(cancelButton);

    expect((titleInput as HTMLInputElement).value).toBe("Old Title");
    expect(await screen.findByText(/modifications annulées/i)).toBeTruthy();
  });

  it("keeps edited metadata form values when save fails", async () => {
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    invokeMock.mockResolvedValueOnce({
      page: 1,
      page_size: 50,
      total: 1,
      items: [
        {
          id: 12,
          title: "Old Title",
          authors: ["Alice"],
          language: "en",
          published_at: "2024-01-01",
          format: "epub",
          source_path: "/tmp/book.epub",
        },
      ],
    });
    invokeMock.mockResolvedValueOnce({
      id: 12,
      title: "Old Title",
      authors: ["Alice"],
      language: "en",
      published_at: "2024-01-01",
      format: "epub",
      source_path: "/tmp/book.epub",
    });
    invokeMock.mockResolvedValueOnce({
      item_id: 12,
      proposals: [],
    });
    invokeMock.mockRejectedValueOnce(new Error("Title is required."));

    render(Page);

    const loadMetadataButton = await screen.findByRole("button", {
      name: /charger les métadonnées/i,
    });
    await fireEvent.click(loadMetadataButton);

    const titleInput = await screen.findByLabelText(/metadata title/i);
    await fireEvent.input(titleInput, { target: { value: "   " } });

    const saveButton = screen.getByRole("button", { name: /enregistrer metadata/i });
    await fireEvent.click(saveButton);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Title is required.");
    expect((titleInput as HTMLInputElement).value).toBe("   ");
  });

  it("triggers enrichment and renders proposal provenance and confidence", async () => {
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    invokeMock.mockResolvedValueOnce({
      page: 1,
      page_size: 50,
      total: 1,
      items: [
        {
          id: 12,
          title: "Old Title",
          authors: ["Alice"],
          language: "en",
          published_at: "2024-01-01",
          format: "epub",
          source_path: "/tmp/book.epub",
        },
      ],
    });
    invokeMock.mockResolvedValueOnce({
      id: 12,
      title: "Old Title",
      authors: ["Alice"],
      language: "en",
      published_at: "2024-01-01",
      format: "epub",
      source_path: "/tmp/book.epub",
    });
    invokeMock.mockResolvedValueOnce({
      item_id: 12,
      proposals: [],
    });
    invokeMock.mockResolvedValueOnce({
      run_id: 300,
      status: "degraded",
      diagnostic: "google_books: timeout",
      proposals: [
        {
          id: 44,
          provider: "open_library",
          confidence: 0.55,
          title: "Enriched Title",
          authors: ["Alice", "Bob"],
          language: "fr",
          published_at: "2024-12-31",
          diagnostic: "Primary provider degraded; fallback provider proposal used.",
          applied_at: null,
        },
      ],
    });

    render(Page);

    const loadMetadataButton = await screen.findByRole("button", {
      name: /charger les métadonnées/i,
    });
    await fireEvent.click(loadMetadataButton);

    const enrichButton = await screen.findByRole("button", {
      name: /enrichir metadata/i,
    });
    await fireEvent.click(enrichButton);

    expect(invokeMock).toHaveBeenNthCalledWith(5, "enrich_library_item_metadata", {
      input: {
        item_id: 12,
      },
    });
    expect(await screen.findByText(/open_library/i)).toBeTruthy();
    expect(screen.getByText(/0.55/)).toBeTruthy();
    expect(screen.getByText(/fallback provider proposal used/i)).toBeTruthy();
  });

  it("applies selected enrichment proposal and refreshes proposal list", async () => {
    invokeMock.mockResolvedValueOnce({
      id: 1,
      name: "Main Library",
      path: "/tmp/caudex-library",
      created_at: "2026-03-05T15:00:00Z",
    });
    invokeMock.mockResolvedValueOnce({
      page: 1,
      page_size: 50,
      total: 1,
      items: [
        {
          id: 12,
          title: "Old Title",
          authors: ["Alice"],
          language: "en",
          published_at: "2024-01-01",
          format: "epub",
          source_path: "/tmp/book.epub",
        },
      ],
    });
    invokeMock.mockResolvedValueOnce({
      id: 12,
      title: "Old Title",
      authors: ["Alice"],
      language: "en",
      published_at: "2024-01-01",
      format: "epub",
      source_path: "/tmp/book.epub",
    });
    invokeMock.mockResolvedValueOnce({
      item_id: 12,
      proposals: [],
    });
    invokeMock.mockResolvedValueOnce({
      run_id: 301,
      status: "success",
      diagnostic: null,
      proposals: [
        {
          id: 45,
          provider: "google_books",
          confidence: 0.91,
          title: "Enriched Title",
          authors: ["Alice", "Bob"],
          language: "fr",
          published_at: "2024-12-31",
          diagnostic: null,
          applied_at: null,
        },
      ],
    });
    invokeMock.mockResolvedValueOnce({
      proposal_id: 45,
      item: {
        id: 12,
        title: "Enriched Title",
        authors: ["Alice", "Bob"],
        language: "fr",
        published_at: "2024-12-31",
        format: "epub",
        source_path: "/tmp/book.epub",
      },
    });
    invokeMock.mockResolvedValueOnce({
      item_id: 12,
      proposals: [
        {
          id: 45,
          provider: "google_books",
          confidence: 0.91,
          title: "Enriched Title",
          authors: ["Alice", "Bob"],
          language: "fr",
          published_at: "2024-12-31",
          diagnostic: null,
          applied_at: "2026-03-05T19:30:00Z",
        },
      ],
    });

    render(Page);

    const loadMetadataButton = await screen.findByRole("button", {
      name: /charger les métadonnées/i,
    });
    await fireEvent.click(loadMetadataButton);

    const enrichButton = await screen.findByRole("button", {
      name: /enrichir metadata/i,
    });
    await fireEvent.click(enrichButton);

    const applyButton = await screen.findByRole("button", {
      name: /appliquer proposition/i,
    });
    await fireEvent.click(applyButton);

    expect(invokeMock).toHaveBeenNthCalledWith(6, "apply_metadata_enrichment_proposal", {
      input: {
        proposal_id: 45,
      },
    });
    expect(await screen.findByText(/proposition appliquée avec succès/i)).toBeTruthy();
    expect(screen.getByText(/Applied at:/i)).toBeTruthy();
  });
});
