import { describe, expect, it, vi } from "vitest";
import { promptRequestNote } from "../library-request";

describe("promptRequestNote", () => {
  it("returns null when the prompt is cancelled (accepted)", () => {
    expect(promptRequestNote("accepted", () => null)).toBeNull();
  });

  it("returns null when the prompt is cancelled (rejected)", () => {
    expect(promptRequestNote("rejected", () => null)).toBeNull();
  });

  it("keeps an empty string for OK-with-no-input so the note is simply skipped", () => {
    expect(promptRequestNote("accepted", () => "")).toBe("");
    expect(promptRequestNote("rejected", () => "")).toBe("");
  });

  it("returns the typed note", () => {
    expect(promptRequestNote("accepted", () => "looks good")).toBe("looks good");
    expect(promptRequestNote("rejected", () => "duplicate")).toBe("duplicate");
  });

  it("uses a different prompt message per action", () => {
    const ask = vi.fn<(message: string) => string | null>(() => "");
    promptRequestNote("accepted", ask);
    promptRequestNote("rejected", ask);
    expect(ask.mock.calls[0][0]).not.toBe(ask.mock.calls[1][0]);
  });
});
