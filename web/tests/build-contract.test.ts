import { readFileSync } from 'node:fs';
import { expect, test } from 'bun:test';

const packageJson = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
const buildSource = readFileSync(new URL('../../build.rs', import.meta.url), 'utf8');
const dockerIgnoreSource = readFileSync(new URL('../../.dockerignore', import.meta.url), 'utf8');

test('package scripts expose repeatable test typecheck and build commands', () => {
  expect(packageJson.scripts.test).toBe('bun test');
  expect(packageJson.scripts.typecheck).toBe('vue-tsc --noEmit');
  expect(packageJson.scripts.build).toBe('bun run typecheck && vite build');
});

test('cargo frontend build requires dependencies installed from the lockfile', () => {
  expect(buildSource).toContain('cargo:rerun-if-changed=web/bun.lock');
  expect(buildSource).not.toContain('run("bun", &["install"]');
  expect(buildSource).toContain(
    'web/node_modules is missing; run `cd web` then `bun install --frozen-lockfile`',
  );
});

test('docker build context excludes local environment files', () => {
  const rules = dockerIgnoreSource.split(/\r?\n/);

  expect(rules).toContain('.env');
  expect(rules).toContain('.env.*');
  expect(rules).toContain('web/.env');
  expect(rules).toContain('web/.env.*');
});
