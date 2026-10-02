export interface DriveDescription {
  description?: string | null;
  description_e2e?: string | null;
}

export interface DriveGroundingEvidence {
  id: number;
  recorded_at: number;
  last_reviewed_at: number | null;
  importance: number;
  revision: string;
}

export interface DriveGrounding {
  reason: string;
  evidence: DriveGroundingEvidence[];
  uncertainty: string[];
  fresh_until: number | null;
  complete: boolean;
}

export interface DriveGroundingSignal {
  grounding?: DriveGrounding | null;
  grounding_e2e?: string | null;
}

export interface PresentedDriveGrounding {
  reason: string;
  evidence: DriveGroundingEvidence[];
  uncertainty: string[];
  freshUntil: number | null;
  encrypted: boolean;
}

export interface PresentedDriveDescription {
  text: string;
  encrypted: boolean;
}

export function isDriveDescriptionEncrypted(signal: DriveDescription): boolean {
  return !signal.description?.trim() && Boolean(signal.description_e2e);
}

export function presentDriveDescription(
  signal: DriveDescription,
  encryptedMessage: string,
  fallbackMessage: string
): PresentedDriveDescription {
  const description = signal.description?.trim();
  if (description) return { text: description, encrypted: false };
  if (isDriveDescriptionEncrypted(signal))
    return { text: encryptedMessage, encrypted: true };
  return { text: fallbackMessage, encrypted: false };
}

export function presentDriveGrounding(
  signal: DriveGroundingSignal,
  encryptedMessage: string
): PresentedDriveGrounding | null {
  if (signal.grounding_e2e?.trim()) {
    return {
      reason: encryptedMessage,
      evidence: [],
      uncertainty: [],
      freshUntil: null,
      encrypted: true,
    };
  }
  const grounding = signal.grounding;
  if (!grounding) return null;
  return {
    reason: grounding.reason,
    evidence: grounding.evidence,
    uncertainty: grounding.uncertainty,
    freshUntil: grounding.fresh_until,
    encrypted: false,
  };
}

export function canAcknowledgeDriveSignal(
  signal: DriveDescription,
  canAcknowledge: boolean,
  isLoading: boolean
): boolean {
  return canAcknowledge && !isLoading && !isDriveDescriptionEncrypted(signal);
}
