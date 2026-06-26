import { describe, it, expect } from 'vitest';
import { cn } from '../utils';

describe('cn (className merge)', () => {
  it('joins multiple class names', () => {
    expect(cn('foo', 'bar')).toBe('foo bar');
  });

  it('handles conditional classes', () => {
    const isActive = true;
    const isDisabled = false;
    expect(cn('base', isDisabled && 'no', isActive && 'yes', undefined, null)).toBe(
      'base yes',
    );
  });

  it('resolves tailwind conflicts (last wins)', () => {
    expect(cn('p-2', 'p-4')).toBe('p-4');
  });

  it('returns empty string for no input', () => {
    expect(cn()).toBe('');
  });

  it('does not collapse identical non-tailwind classes (clsx behavior)', () => {
    expect(cn('foo', 'foo')).toBe('foo foo');
  });
});
