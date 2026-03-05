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
});
