import { expect, test } from 'bun:test';
import { useManagementList } from '../src/composables/useManagementList';

test('management list starts loading without exposing an empty state', () => {
  const list = useManagementList(async () => ['unused']);

  expect(list.items.value).toEqual([]);
  expect(list.loading.value).toBe(true);
  expect(list.loadError.value).toBeNull();
  expect(isManagementListEmpty(list)).toBe(false);
});

test('management list enters loading while its request is pending', async () => {
  const request = deferred<string[]>();
  const list = useManagementList(() => request.promise);

  const loading = list.load();

  expect(list.loading.value).toBe(true);
  expect(list.loadError.value).toBeNull();
  request.resolve([]);
  await loading;
});

test('management list resolves a successful empty response', async () => {
  const list = useManagementList<string>(async () => []);

  expect(await list.load()).toBe(true);
  expect(list.items.value).toEqual([]);
  expect(list.loading.value).toBe(false);
  expect(list.loadError.value).toBeNull();
});

test('management list exposes successful items', async () => {
  const list = useManagementList(async () => ['provider-a', 'provider-b']);

  expect(await list.load()).toBe(true);
  expect(list.items.value).toEqual(['provider-a', 'provider-b']);
  expect(list.loading.value).toBe(false);
  expect(list.loadError.value).toBeNull();
});

test('management list maps 401 rejection without rejecting load', async () => {
  const list = useManagementList(async (): Promise<string[]> => {
    throw {
      isAxiosError: true,
      response: { status: 401, data: { error: 'unauthorized' } },
    };
  });

  expect(await list.load()).toBe(false);
  expect(list.loadError.value).toBe('管理凭据无效或缺失，请检查 ADMIN_TOKEN。');
  expect(list.loading.value).toBe(false);
});

test('management list maps network rejection and retains previously loaded items', async () => {
  let attempt = 0;
  const list = useManagementList(async () => {
    attempt += 1;
    if (attempt === 1) {
      return ['provider-a'];
    }
    throw { isAxiosError: true, request: {} };
  });

  expect(await list.load()).toBe(true);
  expect(await list.load()).toBe(false);
  expect(list.items.value).toEqual(['provider-a']);
  expect(list.loadError.value).toBe('无法连接管理服务，请检查服务状态后重试。');
  expect(list.loading.value).toBe(false);
});

test('management list ignores an older success that resolves after the latest response', async () => {
  const requests = [deferred<string[]>(), deferred<string[]>()];
  let requestIndex = 0;
  const list = useManagementList(() => requests[requestIndex++].promise);

  const olderLoad = list.load();
  const latestLoad = list.load();

  requests[1].resolve(['latest-provider']);
  expect(await latestLoad).toBe(true);
  expect(list.items.value).toEqual(['latest-provider']);
  expect(list.loading.value).toBe(false);

  requests[0].resolve(['older-provider']);
  expect(await olderLoad).toBe(false);
  expect(list.items.value).toEqual(['latest-provider']);
  expect(list.loadError.value).toBeNull();
  expect(list.loading.value).toBe(false);
});

test('management list ignores an older failure without ending the latest loading state', async () => {
  const requests = [deferred<string[]>(), deferred<string[]>()];
  let requestIndex = 0;
  const list = useManagementList(() => requests[requestIndex++].promise);

  const olderLoad = list.load();
  const latestLoad = list.load();

  requests[0].reject(new Error('older request failed'));
  expect(await olderLoad).toBe(false);
  expect(list.items.value).toEqual([]);
  expect(list.loadError.value).toBeNull();
  expect(list.loading.value).toBe(true);

  requests[1].resolve(['latest-provider']);
  expect(await latestLoad).toBe(true);
  expect(list.items.value).toEqual(['latest-provider']);
  expect(list.loadError.value).toBeNull();
  expect(list.loading.value).toBe(false);
});

test('management list state is always one of loading error empty or ready', async () => {
  let nextResponse: () => Promise<string[]> = async () => [];
  const list = useManagementList(() => nextResponse());

  expect(managementListState(list)).toBe('loading');

  const pending = deferred<string[]>();
  nextResponse = () => pending.promise;
  const loading = list.load();
  expect(managementListState(list)).toBe('loading');
  pending.resolve(['provider-a']);
  await loading;
  expect(managementListState(list)).toBe('ready');

  nextResponse = async () => {
    throw new Error('offline');
  };
  await list.load();
  expect(managementListState(list)).toBe('error');

  nextResponse = async () => [];
  await list.load();
  expect(managementListState(list)).toBe('empty');
});

function managementListState<T>(list: ReturnType<typeof useManagementList<T>>) {
  if (list.loading.value) {
    return 'loading';
  }
  if (list.loadError.value) {
    return 'error';
  }
  return list.items.value.length === 0 ? 'empty' : 'ready';
}

function isManagementListEmpty<T>(list: ReturnType<typeof useManagementList<T>>) {
  return !list.loading.value && !list.loadError.value && list.items.value.length === 0;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
