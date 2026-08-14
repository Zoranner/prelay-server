import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

test('client uses Nuxt Tauri and Tailwind entrypoints', () => {
  const packageJson = JSON.parse(
    readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
  );
  const config = readFileSync(new URL('../nuxt.config.ts', import.meta.url), 'utf8');

  expect(packageJson.scripts.typecheck).toBe('nuxt typecheck');
  expect(packageJson.devDependencies['@tauri-apps/cli']).toBeDefined();
  expect(config).toContain('@tailwindcss/vite');
});
