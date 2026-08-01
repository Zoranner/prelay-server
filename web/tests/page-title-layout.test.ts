import { readFileSync } from 'node:fs';
import { expect, test } from 'bun:test';

const styleSource = readFileSync(new URL('../src/style.css', import.meta.url), 'utf8');
const pageHeaderSource = readFileSync(
  new URL('../src/components/base/PageHeader.vue', import.meta.url),
  'utf8',
);

const pages = [
  {
    name: '统计',
    source: readFileSync(new URL('../src/views/StatsView.vue', import.meta.url), 'utf8'),
  },
  {
    name: '供应商',
    source: readFileSync(new URL('../src/views/ProvidersView.vue', import.meta.url), 'utf8'),
  },
  {
    name: '接口',
    source: readFileSync(new URL('../src/views/InterfacesView.vue', import.meta.url), 'utf8'),
  },
];

test('page title actions align with the bottom of the description across main pages', () => {
  for (const page of pages) {
    expect(page.source).toContain(`<PageHeader title="${page.name}"`);
  }

  expect(pageHeaderSource).toContain('class="page-titlebar"');
  expect(pageHeaderSource).toContain('class="page-titlebar__actions"');
  expect(styleSource).toContain('.page-titlebar');
  expect(styleSource).toContain('align-items: end');
  expect(styleSource).not.toContain('align-items: start');
});
