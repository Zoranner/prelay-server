import { readFileSync } from 'node:fs';
import { expect, test } from 'bun:test';

const source = readFileSync(new URL('../src/App.vue', import.meta.url), 'utf8');

test('app header does not expose global token management or search controls', () => {
  expect(source).not.toContain('管理 Token');
  expect(source).not.toContain('搜索供应商、模型、接口');
  expect(source).not.toContain('v-model="searchQuery"');
  expect(source).not.toContain(':search-query=');
});
