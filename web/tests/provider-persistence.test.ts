import { expect, test } from 'bun:test';
import type { CreateConfigRequest, UpdateConfigRequest } from '../src/api';
import { persistProviderConfig } from '../src/utils/providerPersistence';

test('create command calls create exactly once and never calls update', async () => {
  const calls = persistenceCalls();
  const payload = createPayload();

  const result = await persistProviderConfig(calls.api, {
    type: 'create',
    payload,
  });

  expect(result).toBe('created');
  expect(calls.created).toEqual([payload]);
  expect(calls.updated).toEqual([]);
});

test('update command calls update exactly once and never calls create', async () => {
  const calls = persistenceCalls();
  const payload: UpdateConfigRequest = {
    name: 'Updated Provider',
    models: ['model-a', 'model-b'],
  };

  const result = await persistProviderConfig(calls.api, {
    type: 'update',
    id: 'provider-1',
    payload,
  });

  expect(result).toBe('updated');
  expect(calls.created).toEqual([]);
  expect(calls.updated).toEqual([{ id: 'provider-1', payload }]);
});

test('persistence command keeps the complete model collection in its payload', async () => {
  const calls = persistenceCalls();
  const payload = createPayload();

  await persistProviderConfig(calls.api, { type: 'create', payload });

  expect(calls.created[0]?.models).toEqual(['model-a', 'model-b']);
});

test('persistence rejection propagates the original error', async () => {
  const failure = new Error('write failed');
  const api = {
    create: async (_payload: CreateConfigRequest): Promise<never> => {
      throw failure;
    },
    update: async (_id: string, _payload: UpdateConfigRequest) => 'unused',
  };

  expect(persistProviderConfig(api, { type: 'create', payload: createPayload() })).rejects.toBe(
    failure,
  );
});

function createPayload(): CreateConfigRequest {
  return {
    name: 'Provider',
    provider_type: 'openai_compatible',
    base_url: 'https://example.test',
    api_key: 'secret',
    capabilities: { upstream_protocols: ['openai'] },
    models: ['model-a', 'model-b'],
  };
}

function persistenceCalls() {
  const created: CreateConfigRequest[] = [];
  const updated: Array<{ id: string; payload: UpdateConfigRequest }> = [];
  const api = {
    create: async (payload: CreateConfigRequest) => {
      created.push(payload);
      return 'created';
    },
    update: async (id: string, payload: UpdateConfigRequest) => {
      updated.push({ id, payload });
      return 'updated';
    },
  };
  return { api, created, updated };
}
