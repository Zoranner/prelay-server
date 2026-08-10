import { readFileSync } from 'node:fs';
import { expect, test } from 'bun:test';

const packageJson = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
const buildSource = readFileSync(new URL('../../build.rs', import.meta.url), 'utf8');
const dockerIgnoreSource = readFileSync(new URL('../../.dockerignore', import.meta.url), 'utf8');
const mainSource = readFileSync(new URL('../../src/main.rs', import.meta.url), 'utf8');
const dockerfileSource = readFileSync(new URL('../../Dockerfile', import.meta.url), 'utf8');
const composeSource = readFileSync(
  new URL('../../docker/docker-compose.yml', import.meta.url),
  'utf8',
);
const viteConfigSource = readFileSync(new URL('../vite.config.ts', import.meta.url), 'utf8');
const readmeSource = readFileSync(new URL('../../README.md', import.meta.url), 'utf8');

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

test('service defaults consistently use port 18080', () => {
  expect(mainSource).toContain('.unwrap_or(18080)');
  expect(dockerfileSource).toContain('EXPOSE 18080');
  expect(dockerfileSource).toContain('ENV LISTEN_PORT=18080');
  expect(composeSource).toContain('- 18080:18080');
  expect(composeSource).toContain('- LISTEN_PORT=18080');
  expect(viteConfigSource).toContain('http://localhost:18080');
  expect(readmeSource).toContain('0.0.0.0:18080');
  expect(readmeSource).toContain('| `LISTEN_PORT` | `18080` |');
});
