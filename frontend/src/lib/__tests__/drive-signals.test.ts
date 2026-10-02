import { describe, expect, it } from "vitest";
import {
  canAcknowledgeDriveSignal,
  presentDriveDescription,
  presentDriveGrounding,
} from "../drive-signals";

describe("drive signal description presentation", () => {
  it("uses plaintext when available", () => {
    expect(
      presentDriveDescription(
        { description: "  Inspect the repository  " },
        "Encrypted for the registered executor",
        "No description"
      )
    ).toEqual({ text: "Inspect the repository", encrypted: false });
  });

  describe("drive signal grounding presentation", () => {
    it("shows memory provenance and uncertainty without implying a confidence score", () => {
      expect(
        presentDriveGrounding(
          {
            grounding: {
              reason:
                "Recent decision memory matched the existing follow-up gate.",
              evidence: [
                {
                  id: 42,
                  recorded_at: 1_780_000_000,
                  last_reviewed_at: null,
                  importance: 2.5,
                  revision: "0000000000000065",
                },
              ],
              uncertainty: ["single_memory_source", "not_reviewed"],
              fresh_until: 1_780_001_800,
              complete: true,
            },
          },
          "Encrypted for the registered executor"
        )
      ).toEqual({
        reason: "Recent decision memory matched the existing follow-up gate.",
        evidence: [
          {
            id: 42,
            recorded_at: 1_780_000_000,
            last_reviewed_at: null,
            importance: 2.5,
            revision: "0000000000000065",
          },
        ],
        uncertainty: ["single_memory_source", "not_reviewed"],
        freshUntil: 1_780_001_800,
        encrypted: false,
      });
    });

    it("does not expose grounding details when the E2E projection is present", () => {
      expect(
        presentDriveGrounding(
          {
            grounding: {
              reason: "Sensitive source reason",
              evidence: [
                {
                  id: 42,
                  recorded_at: 1_780_000_000,
                  last_reviewed_at: null,
                  importance: 2.5,
                  revision: "0000000000000065",
                },
              ],
              uncertainty: ["single_memory_source"],
              fresh_until: null,
              complete: true,
            },
            grounding_e2e: "ciphertext",
          },
          "Encrypted for the registered executor"
        )
      ).toEqual({
        reason: "Encrypted for the registered executor",
        evidence: [],
        uncertainty: [],
        freshUntil: null,
        encrypted: true,
      });
    });
  });

  it("marks encrypted payloads instead of presenting evidence as the description", () => {
    expect(
      presentDriveDescription(
        { description: null, description_e2e: "base64-ciphertext" },
        "Encrypted for the registered executor",
        "No description"
      )
    ).toEqual({
      text: "Encrypted for the registered executor",
      encrypted: true,
    });
  });

  it("uses the fallback only when neither plaintext nor ciphertext is present", () => {
    expect(
      presentDriveDescription(
        { description: null, description_e2e: null },
        "Encrypted for the registered executor",
        "No description"
      )
    ).toEqual({ text: "No description", encrypted: false });
  });

  it("does not allow acknowledgement when the dashboard cannot read the encrypted description", () => {
    expect(
      canAcknowledgeDriveSignal(
        { description: null, description_e2e: "base64-ciphertext" },
        true,
        false
      )
    ).toBe(false);
    expect(
      canAcknowledgeDriveSignal(
        { description: "Readable description", description_e2e: null },
        true,
        false
      )
    ).toBe(true);
    expect(
      canAcknowledgeDriveSignal(
        { description: "Readable description" },
        true,
        true
      )
    ).toBe(false);
  });
});
