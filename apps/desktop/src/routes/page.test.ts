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
});
