import { expect, test } from 'bun:test';
import type { CreateInterfaceRequest, UpdateInterfaceRequest } from '../src/api';
import { persistInterface, type InterfacePersistenceApi } from '../src/utils/interfacePersistence';

test('create sends the complete model collection in exactly one interface request', async () => {
  const calls = persistenceCalls();
  const payload = createPayload();

  await persistInterface(calls.api, { type: 'create', payload });

  expect(calls.created).toEqual([payload]);
  expect(calls.updated).toEqual([]);
});

test('update sends the complete model collection in exactly one interface request', async () => {
  const calls = persistenceCalls();
  const payload: UpdateInterfaceRequest = {
    name: 'Updated Interface',
    models: interfaceModels(),
  };

  await persistInterface(calls.api, { type: 'update', id: 'interface-1', payload });

  expect(calls.created).toEqual([]);
  expect(calls.updated).toEqual([{ id: 'interface-1', payload }]);
});

test('interface persistence propagates a rejected request', async () => {
  const failure = new Error('write failed');
  const api: InterfacePersistenceApi = {
    createInterface: async (_payload) => {
      throw failure;
    },
    updateInterface: async (_id, _payload) => undefined,
  };

  expect(persistInterface(api, { type: 'create', payload: createPayload() })).rejects.toBe(failure);
});

function createPayload(): CreateInterfaceRequest {
  return {
    name: 'Primary Interface',
    models: interfaceModels(),
  };
}

function interfaceModels() {
  return [
    {
      provider_id: 'provider-1',
      upstream_model: 'model-a',
      model_name: 'public-a',
    },
    {
      provider_id: 'provider-2',
      upstream_model: 'model-b',
      model_name: 'public-b',
    },
  ];
}

function persistenceCalls() {
  const created: CreateInterfaceRequest[] = [];
  const updated: Array<{ id: string; payload: UpdateInterfaceRequest }> = [];
  const api: InterfacePersistenceApi = {
    createInterface: async (payload) => {
      created.push(payload);
      return undefined;
    },
    updateInterface: async (id, payload) => {
      updated.push({ id, payload });
      return undefined;
    },
  };
  return { api, created, updated };
}
