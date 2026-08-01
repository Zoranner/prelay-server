import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import type { ProviderConfig } from '../src/api';
import { clearStoredProviderTokens } from '../src/utils/providers';
import {
  providerModelOptions,
  providerOptionsForInterface,
  mergeDiscoveredProviderModels,
  normalizeProviderModelNames,
} from '../src/utils/providerModels';

const apiSource = readFileSync(new URL('../src/api/index.ts', import.meta.url), 'utf8');
const providerViewSource = readFileSync(
  new URL('../src/views/ProvidersView.vue', import.meta.url),
  'utf8',
);

test('interface upstream model options come from provider-owned model list', () => {
  const provider = providerConfig('p1', 'DeepSeek', 'deepseek', [
    'deepseek-chat',
    'deepseek-reasoner',
  ]);

  expect(providerModelOptions(provider)).toEqual([
    { label: 'deepseek-chat', value: 'deepseek-chat' },
    { label: 'deepseek-reasoner', value: 'deepseek-reasoner' },
  ]);
});

test('providers without configured models are not selectable for interface model binding', () => {
  const providers = [
    providerConfig('p1', 'DeepSeek', 'deepseek', ['deepseek-chat']),
    providerConfig('p2', 'Custom', 'openai_compatible', []),
  ];

  expect(providerOptionsForInterface(providers)).toEqual([{ label: 'DeepSeek', value: 'p1' }]);
});

test('interface provider options do not filter by protocol because routing selects bridges at request time', () => {
  const providers = [
    providerConfig('p1', 'OpenAI', 'openai', ['gpt-4.1']),
    providerConfig('p2', 'DeepSeek', 'deepseek', ['deepseek-chat']),
  ];

  expect(providerOptionsForInterface(providers)).toEqual([
    { label: 'OpenAI', value: 'p1' },
    { label: 'DeepSeek', value: 'p2' },
  ]);
});

test('interface model bindings validate only provider-owned model catalog membership', () => {
  const provider = providerConfig('p1', 'OpenAI', 'openai', ['gpt-4.1']);

  expect(providerModelOptions(provider).map((option) => option.value)).toEqual(['gpt-4.1']);
});

test('discovered provider models merge into form list without duplicates', () => {
  expect(
    mergeDiscoveredProviderModels(
      ['manual-model', 'deepseek-chat'],
      ['', 'deepseek-chat', 'deepseek-reasoner', 'manual-model'],
    ),
  ).toEqual(['deepseek-chat', 'deepseek-reasoner', 'manual-model']);
});

test('provider model names are normalized before saving', () => {
  expect(
    normalizeProviderModelNames([' kimi-k2 ', '', 'kimi-k2', 'moonshot-v1', ' moonshot-v1 ']),
  ).toEqual(['kimi-k2', 'moonshot-v1']);
});

test('provider create and update requests declare complete model collections', () => {
  const createRequest = apiSource.slice(
    apiSource.indexOf('export interface CreateConfigRequest'),
    apiSource.indexOf('export interface UpdateConfigRequest'),
  );
  const updateRequest = apiSource.slice(
    apiSource.indexOf('export interface UpdateConfigRequest'),
    apiSource.indexOf('export interface CreateInterfaceRequest'),
  );

  expect(createRequest).toContain('models: string[];');
  expect(updateRequest).toContain('models?: string[];');
});

test('provider save sends normalized models through one persistence command', () => {
  const saveStart = providerViewSource.indexOf('async function saveProvider');
  const saveEnd = providerViewSource.indexOf('\nfunction ', saveStart + 1);
  const saveBody = providerViewSource.slice(saveStart, saveEnd);

  expect(saveBody).toContain('form.value.models = normalizeProviderModelNames(form.value.models)');
  expect(saveBody).toContain('models: form.value.models');
  expect(saveBody).toContain('const command: ProviderPersistenceCommand');
  expect(saveBody).toContain('await persistProviderConfig(configApi, command)');
  expect(providerViewSource).not.toContain('syncProviderModels');
  expect(providerViewSource).not.toContain('createMissingProviderModels');
  expect(providerViewSource).not.toContain('saveToken');
});

test('provider save rejects an empty model list without implicit discovery', () => {
  const saveStart = providerViewSource.indexOf('async function saveProvider');
  const saveEnd = providerViewSource.indexOf('\nfunction ', saveStart + 1);
  const saveBody = providerViewSource.slice(saveStart, saveEnd);

  expect(saveBody).toContain("text: '请至少添加一个上游模型。'");
  expect(saveBody).not.toContain('discoverModels');
});

test('provider save closes and reloads only on success', () => {
  const saveStart = providerViewSource.indexOf('async function saveProvider');
  const saveEnd = providerViewSource.indexOf('\nfunction ', saveStart + 1);
  const saveBody = providerViewSource.slice(saveStart, saveEnd);
  const catchStart = saveBody.indexOf('} catch (error) {');
  const finallyStart = saveBody.indexOf('} finally {', catchStart);
  const successStart = saveBody.indexOf('if (!isCurrentDrawerSession(saveSession))', finallyStart);
  const successBody = saveBody.slice(successStart);
  const catchBody = saveBody.slice(catchStart, finallyStart);

  expect(saveBody.indexOf('await persistProviderConfig(configApi, command)')).toBeLessThan(
    catchStart,
  );
  expect(successBody).toContain('drawerOpen.value = false');
  expect(successBody).toContain('await loadData()');
  expect(catchBody).not.toContain('drawerOpen.value = false');
  expect(catchBody).not.toContain('await loadData()');
});

test('legacy provider token cleanup removes only the retired storage key', () => {
  const removedKeys: string[] = [];
  const storage = {
    removeItem(key: string) {
      removedKeys.push(key);
    },
  };

  clearStoredProviderTokens(storage);

  expect(removedKeys).toEqual(['provider-relay-tokens']);
  expect(removedKeys).not.toContain('provider-relay-admin-token');
});

function providerConfig(
  id: string,
  name: string,
  providerType: string,
  models: string[],
): ProviderConfig {
  return {
    id,
    name,
    provider_type: providerType,
    base_url: 'https://example.test',
    api_key_masked: 'sk-...test',
    token: `token-${id}`,
    capabilities: {},
    models: models.map((modelName, index) => ({
      id: `${id}-model-${index}`,
      provider_id: id,
      model_name: modelName,
      created_at: '2026-07-24T00:00:00Z',
    })),
    created_at: '2026-07-24T00:00:00Z',
  };
}
