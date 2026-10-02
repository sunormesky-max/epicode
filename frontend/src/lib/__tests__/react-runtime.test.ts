import { expect, it } from 'vitest';
import { version as reactVersion } from 'react';
import { version as rendererVersion } from 'react-dom';

it('locks React and its DOM renderer to the same version to avoid a blank page', () => {
  expect(rendererVersion).toBe(reactVersion);
});
