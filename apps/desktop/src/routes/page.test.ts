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

const readyLibrary = {
  id: 1,
  name: "Main Library",
  path: "/tmp/caudex-library",
  created_at: "2026-03-05T15:00:00Z",
};

const oneItemList = {
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
      tags: ["to-read"],
      collections: ["Classics"],
    },
  ],
};

const oneItemDetails = {
  id: 12,
  title: "Old Title",
  authors: ["Alice"],
  language: "en",
  published_at: "2024-01-01",
  format: "epub",
  source_path: "/tmp/book.epub",
  tags: ["to-read"],
  collections: ["Classics"],
};

function installLibraryReadyInvokeHandler(overrides?: Record<string, unknown>) {
  const map: Record<string, unknown> = {
    get_library: readyLibrary,
    list_library_items: oneItemList,
    get_library_item_metadata: oneItemDetails,
    list_metadata_enrichment_proposals: { item_id: 12, proposals: [] },
    list_metadata_conflicts: { item_id: 12, conflicts: [] },
    list_metadata_tags: { names: ["to-read", "favorite"] },
    list_metadata_collections: { names: ["Classics", "Sci-Fi"] },
    get_index_queue_status: {
      queued_count: 1,
      running_count: 0,
      success_count: 0,
      failed_count: 0,
      retry_count: 0,
      recovered_count: 0,
      index_root: "/tmp/caudex-library/.caudex/search-index-v1",
    },
    ...overrides,
  };

  invokeMock.mockImplementation(async (command: string) => {
    if (!(command in map)) {
      throw new Error(`Unhandled command: ${command}`);
    }
    return map[command];
  });
}

describe("workspace flow", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    openMock.mockReset();
    documentDirMock.mockReset();
    homeDirMock.mockReset();
    joinMock.mockReset();
    window.localStorage.clear();

    documentDirMock.mockResolvedValue("/Users/test/Documents");
    homeDirMock.mockResolvedValue("/Users/test");
    joinMock.mockResolvedValue("/Users/test/Documents/Caudex");
  });

  it("shows first-run setup when no library exists", async () => {
    invokeMock.mockResolvedValueOnce(null);

    render(Page);

    expect(await screen.findByRole("heading", { name: /set up your library/i })).toBeTruthy();
  });

  it("creates the library and opens import step", async () => {
    invokeMock.mockResolvedValueOnce(null);
    invokeMock.mockResolvedValueOnce(readyLibrary);
    openMock.mockResolvedValueOnce("/tmp/caudex-library");

    render(Page);

    await fireEvent.input(await screen.findByLabelText(/library name/i), {
      target: { value: "Main Library" },
    });
    await fireEvent.click(screen.getByRole("button", { name: /choisir un emplacement/i }));
    await fireEvent.click(screen.getByRole("button", { name: /create library/i }));

    expect(await screen.findByRole("heading", { name: /import/i })).toBeTruthy();
    expect(invokeMock).toHaveBeenCalledWith("create_library", {
      input: {
        name: "Main Library",
        path: "/tmp/caudex-library",
      },
    });
  });

  it("imports selected files from the import step", async () => {
    installLibraryReadyInvokeHandler({
      list_library_items: { page: 1, page_size: 50, total: 0, items: [] },
      start_import: {
        job_id: 77,
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
      },
    });
    openMock.mockResolvedValueOnce(["/tmp/good.epub", "/tmp/bad.txt"]);

    render(Page);

    await fireEvent.click(await screen.findByRole("button", { name: /^Import$/i }));
    await fireEvent.click(await screen.findByRole("button", { name: /importer des fichiers/i }));

    expect(invokeMock).toHaveBeenCalledWith("start_import", {
      input: {
        paths: ["/tmp/good.epub", "/tmp/bad.txt"],
      },
    });
    expect(await screen.findByText(/unsupported file format/i)).toBeTruthy();
  });

  it("loads metadata and saves edited values including tags/collections", async () => {
    installLibraryReadyInvokeHandler({
      update_library_item_metadata: {
        ...oneItemDetails,
        title: "New Title",
        authors: ["Alice", "Bob"],
        language: "fr",
        published_at: "2024-12-31",
        tags: ["favorite", "batch-updated"],
        collections: ["Curated"],
      },
    });

    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: /^Metadata$/i }));

    await fireEvent.input(await screen.findByLabelText(/metadata title/i), {
      target: { value: "New Title" },
    });
    await fireEvent.input(screen.getByLabelText(/metadata authors/i), {
      target: { value: "Alice, Bob" },
    });
    await fireEvent.input(screen.getByLabelText(/metadata language/i), {
      target: { value: "fr" },
    });
    await fireEvent.input(screen.getByLabelText(/metadata published date/i), {
      target: { value: "2024-12-31" },
    });
    await fireEvent.input(screen.getByLabelText(/metadata tags/i), {
      target: { value: "favorite, batch-updated" },
    });
    await fireEvent.input(screen.getByLabelText(/metadata collections/i), {
      target: { value: "Curated" },
    });

    await fireEvent.click(screen.getByRole("button", { name: /enregistrer metadata/i }));

    expect(invokeMock).toHaveBeenCalledWith("update_library_item_metadata", {
      input: {
        item_id: 12,
        title: "New Title",
        authors: ["Alice", "Bob"],
        language: "fr",
        published_at: "2024-12-31",
        tags: ["favorite", "batch-updated"],
        collections: ["Curated"],
      },
    });
    expect(await screen.findByText(/metadata enregistrée/i)).toBeTruthy();
  });

  it("runs batch preview and execute on selected items", async () => {
    installLibraryReadyInvokeHandler({
      preview_batch_metadata_update: {
        run_id: "batch-preview-1",
        mode: "preview",
        status: "success",
        total_targets: 1,
        updated_count: 1,
        skipped_count: 0,
        failed_count: 0,
        outcomes: [
          {
            item_id: 12,
            status: "updated",
            reason: null,
            retry_eligible: false,
            before: oneItemDetails,
            after: { ...oneItemDetails, title: "Batch Title" },
          },
        ],
      },
      execute_batch_metadata_update: {
        run_id: "batch-exec-1",
        mode: "execute",
        status: "success",
        total_targets: 1,
        updated_count: 1,
        skipped_count: 0,
        failed_count: 0,
        outcomes: [
          {
            item_id: 12,
            status: "updated",
            reason: null,
            retry_eligible: false,
            before: oneItemDetails,
            after: { ...oneItemDetails, title: "Batch Title" },
          },
        ],
      },
    });

    render(Page);

    await fireEvent.click(await screen.findByRole("button", { name: /^Batch$/i }));
    await fireEvent.click(await screen.findByRole("button", { name: /sélectionner visibles/i }));

    await fireEvent.input(screen.getByLabelText(/batch title/i), {
      target: { value: "Batch Title" },
    });

    await fireEvent.click(screen.getByRole("button", { name: /aperçu batch/i }));
    expect(invokeMock).toHaveBeenCalledWith("preview_batch_metadata_update", {
      input: {
        item_ids: [12],
        patch: {
          title: "Batch Title",
        },
      },
    });

    await fireEvent.click(screen.getByRole("button", { name: /exécuter batch/i }));
    expect(invokeMock).toHaveBeenCalledWith("execute_batch_metadata_update", {
      input: {
        item_ids: [12],
        patch: {
          title: "Batch Title",
        },
      },
    });

    expect(await screen.findByText(/execute run batch-exec-1/i)).toBeTruthy();
  });

  it("detects and resolves metadata conflicts", async () => {
    installLibraryReadyInvokeHandler({
      detect_metadata_conflicts: {
        item_id: 12,
        conflicts: [
          {
            id: 700,
            item_id: 12,
            field_name: "title",
            current_value: "Old Title",
            candidate_value: "Draft Title",
            candidate_source: "manual_edit",
            status: "pending",
            rationale: null,
            created_at: "2026-03-05T19:00:00Z",
            resolved_at: null,
          },
        ],
      },
      list_metadata_conflicts: {
        item_id: 12,
        conflicts: [
          {
            id: 700,
            item_id: 12,
            field_name: "title",
            current_value: "Old Title",
            candidate_value: "Draft Title",
            candidate_source: "manual_edit",
            status: "pending",
            rationale: null,
            created_at: "2026-03-05T19:00:00Z",
            resolved_at: null,
          },
        ],
      },
      resolve_metadata_conflict: {
        conflict: {
          id: 700,
          item_id: 12,
          field_name: "title",
          current_value: "Old Title",
          candidate_value: "Draft Title",
          candidate_source: "manual_edit",
          status: "resolved_use_candidate",
          rationale: "Applied from explicit curator decision",
          created_at: "2026-03-05T19:00:00Z",
          resolved_at: "2026-03-05T19:02:00Z",
        },
        item: {
          ...oneItemDetails,
          title: "Draft Title",
        },
      },
    });

    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: /^Metadata$/i }));

    await fireEvent.input(await screen.findByLabelText(/metadata title/i), {
      target: { value: "Draft Title" },
    });

    await fireEvent.click(screen.getByRole("button", { name: /^Conflicts$/i }));
    await fireEvent.click(screen.getByRole("button", { name: /détecter conflits/i }));
    expect(invokeMock).toHaveBeenCalledWith("detect_metadata_conflicts", {
      input: {
        item_id: 12,
        source: "manual_edit",
        candidate: {
          title: "Draft Title",
          authors: ["Alice"],
          language: "en",
          published_at: "2024-01-01",
        },
      },
    });

    await fireEvent.click(await screen.findByRole("button", { name: /appliquer candidat/i }));

    expect(invokeMock).toHaveBeenCalledWith("resolve_metadata_conflict", {
      input: {
        conflict_id: 700,
        resolution: "use_candidate",
        rationale: "Applied from explicit curator decision",
      },
    });
    expect(await screen.findByText(/conflit résolu/i)).toBeTruthy();
  });

  it("runs enrichment and applies a proposal", async () => {
    installLibraryReadyInvokeHandler({
      enrich_library_item_metadata: {
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
      },
      apply_metadata_enrichment_proposal: {
        proposal_id: 45,
        item: {
          ...oneItemDetails,
          title: "Enriched Title",
          authors: ["Alice", "Bob"],
          language: "fr",
          published_at: "2024-12-31",
        },
      },
      list_metadata_enrichment_proposals: {
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
      },
    });

    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: /^Metadata$/i }));

    await fireEvent.click(await screen.findByRole("button", { name: /enrichir metadata/i }));
    await fireEvent.click(screen.getByRole("button", { name: /^Settings$/i }));
    await fireEvent.click(await screen.findByRole("button", { name: /appliquer proposition/i }));

    expect(invokeMock).toHaveBeenCalledWith("apply_metadata_enrichment_proposal", {
      input: {
        proposal_id: 45,
      },
    });
    expect(await screen.findByText(/applied at:/i)).toBeTruthy();
  });

  it("restores persisted workspace filters and step", async () => {
    window.localStorage.setItem(
      "caudex.workspace.v1",
      JSON.stringify({
        step: "import",
        selected_metadata_item_id: 12,
        filters: {
          author: "Alice",
          language: "en",
          tag: "to-read",
          collection: "Classics",
          sort_by: "title",
          sort_direction: "desc",
        },
      }),
    );

    installLibraryReadyInvokeHandler();

    render(Page);

    expect(await screen.findByRole("heading", { name: /import/i })).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: /^Library$/i }));
    const filterAuthorInput = (await screen.findByLabelText(/^Author$/i)) as HTMLInputElement;
    const filterLanguageInput = screen.getByLabelText(/^Language$/i) as HTMLInputElement;
    expect(filterAuthorInput.value).toBe("Alice");
    expect(filterLanguageInput.value).toBe("en");

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("list_library_items", {
        input: {
          page: 1,
          page_size: 50,
          author: "Alice",
          language: "en",
          published_from: null,
          published_to: null,
          tag: "to-read",
          collection: "Classics",
          sort_by: "title",
          sort_direction: "desc",
        },
      });
    });
  });

  it("opens metadata editor from library table with keyboard enter", async () => {
    installLibraryReadyInvokeHandler();

    render(Page);

    const titleCell = await screen.findByText("Old Title");
    const row = titleCell.closest("tr") as HTMLElement;
    row.focus();

    await fireEvent.keyDown(row, { key: "Enter" });

    expect(await screen.findByRole("heading", { name: /metadata editor/i })).toBeTruthy();
  });

  it("shows index queue status and can trigger index actions from settings", async () => {
    installLibraryReadyInvokeHandler({
      process_index_work_queue: {
        processed_count: 1,
        success_count: 1,
        failed_count: 0,
      },
      retry_failed_index_work_units: {
        marked_retry_count: 2,
      },
      ensure_search_index_health: {
        repair_performed: true,
        rebuild_queued_count: 1,
        index_root: "/tmp/caudex-library/.caudex/search-index-v1",
        diagnostic: "Search index repaired after corruption detection.",
      },
    });

    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: /^Settings$/i }));

    expect(await screen.findByText(/index queue/i)).toBeTruthy();
    expect(screen.getByText(/queued: 1/i)).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: /traiter la file/i }));
    expect(invokeMock).toHaveBeenCalledWith("process_index_work_queue", {
      input: {
        batch_size: 100,
        include_failed: false,
      },
    });

    await fireEvent.click(screen.getByRole("button", { name: /retry failed/i }));
    expect(invokeMock).toHaveBeenCalledWith("retry_failed_index_work_units", {
      input: {
        limit: null,
      },
    });

    await fireEvent.click(screen.getByRole("button", { name: /vérifier\/réparer l'index/i }));
    expect(invokeMock).toHaveBeenCalledWith("ensure_search_index_health");
  });
});
