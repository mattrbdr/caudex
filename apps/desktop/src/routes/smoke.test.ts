import { render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import Page from "./+page.svelte";

const invokeMock = vi.fn();
const documentDirMock = vi.fn();
const homeDirMock = vi.fn();
const joinMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("@tauri-apps/api/path", () => ({
  documentDir: (...args: unknown[]) => documentDirMock(...args),
  homeDir: (...args: unknown[]) => homeDirMock(...args),
  join: (...args: unknown[]) => joinMock(...args),
}));

describe("smoke startup", () => {
  it("renders first-run route under clean state", async () => {
    invokeMock.mockResolvedValueOnce(null);
    documentDirMock.mockResolvedValue("/Users/test/Documents");
    homeDirMock.mockResolvedValue("/Users/test");
    joinMock.mockResolvedValue("/Users/test/Documents/Caudex");

    render(Page);

    expect(
      await screen.findByRole("heading", {
        name: /set up your library/i,
      }),
    ).toBeTruthy();
  });
});
