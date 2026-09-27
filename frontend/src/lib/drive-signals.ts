export interface DriveDescription {
  description?: string | null;
  description_e2e?: string | null;
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
  fallbackMessage: string,
): PresentedDriveDescription {
  const description = signal.description?.trim();
  if (description) return { text: description, encrypted: false };
  if (isDriveDescriptionEncrypted(signal)) return { text: encryptedMessage, encrypted: true };
  return { text: fallbackMessage, encrypted: false };
}

export function canAcknowledgeDriveSignal(
  signal: DriveDescription,
  canAcknowledge: boolean,
  isLoading: boolean,
): boolean {
  return canAcknowledge && !isLoading && !isDriveDescriptionEncrypted(signal);
}
