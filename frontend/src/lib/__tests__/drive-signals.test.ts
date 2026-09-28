import { describe, expect, it } from 'vitest';
import { canAcknowledgeDriveSignal, presentDriveDescription } from '../drive-signals';

describe('drive signal description presentation', () => {
  it('uses plaintext when available', () => {
    expect(presentDriveDescription(
      { description: '  Inspect the repository  ' },
      'Encrypted for the registered executor',
      'No description',
    )).toEqual({ text: 'Inspect the repository', encrypted: false });
  });

  it('marks encrypted payloads instead of presenting evidence as the description', () => {
    expect(presentDriveDescription(
      { description: null, description_e2e: 'base64-ciphertext' },
      'Encrypted for the registered executor',
      'No description',
    )).toEqual({ text: 'Encrypted for the registered executor', encrypted: true });
  });

  it('uses the fallback only when neither plaintext nor ciphertext is present', () => {
    expect(presentDriveDescription(
      { description: null, description_e2e: null },
      'Encrypted for the registered executor',
      'No description',
    )).toEqual({ text: 'No description', encrypted: false });
  });

  it('does not allow acknowledgement when the dashboard cannot read the encrypted description', () => {
    expect(canAcknowledgeDriveSignal(
      { description: null, description_e2e: 'base64-ciphertext' },
      true,
      false,
    )).toBe(false);
    expect(canAcknowledgeDriveSignal(
      { description: 'Readable description', description_e2e: null },
      true,
      false,
    )).toBe(true);
    expect(canAcknowledgeDriveSignal(
      { description: 'Readable description' },
      true,
      true,
    )).toBe(false);
  });
});
