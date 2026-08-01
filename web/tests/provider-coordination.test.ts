import { expect, test } from 'bun:test';
import {
  createDrawerSessionCoordinator,
  createKeyedRequestCoordinator,
  fingerprintProviderConfiguration,
  selectProvidersForPing,
  type ProviderPingSnapshot,
} from '../src/utils/providerCoordination';

test('a superseding request invalidates an older response for the same provider', () => {
  const requests = createKeyedRequestCoordinator<string>();
  const older = requests.begin('provider-1');
  const newer = requests.begin('provider-1', { supersede: true });
  let result = 'unchecked';

  expect(older).not.toBeNull();
  expect(newer).not.toBeNull();
  expect(requests.isCurrent(older!)).toBe(false);
  expect(requests.isCurrent(newer!)).toBe(true);
  expect(requests.isBusy('provider-1')).toBe(true);

  if (requests.isCurrent(newer!)) {
    result = 'available';
  }
  expect(requests.finish(newer!)).toBe(true);
  if (requests.isCurrent(older!)) {
    result = 'unavailable';
  }

  expect(result).toBe('available');
  expect(requests.finish(older!)).toBe(false);
  expect(requests.isBusy('provider-1')).toBe(false);
});

test('request busy state is isolated by provider id', () => {
  const requests = createKeyedRequestCoordinator<string>();
  const request = requests.begin('provider-1');

  expect(request).not.toBeNull();
  expect(requests.isBusy('provider-1')).toBe(true);
  expect(requests.isBusy('provider-2')).toBe(false);

  requests.finish(request!);

  expect(requests.isBusy('provider-1')).toBe(false);
});

test('auto ping selects new changed missing-result and stale providers only', () => {
  const currentFingerprint = fingerprintProviderConfiguration({
    baseUrl: 'https://example.test',
    capabilities: { responses: true, chat: true },
  });
  const changedFingerprint = fingerprintProviderConfiguration({
    capabilities: { chat: true },
    baseUrl: 'https://old.example.test',
  });
  const snapshots: Record<string, ProviderPingSnapshot | undefined> = {
    fresh: { configurationFingerprint: currentFingerprint, checkedAt: 1_500 },
    changed: { configurationFingerprint: changedFingerprint, checkedAt: 1_500 },
    missing: { configurationFingerprint: currentFingerprint },
    stale: { configurationFingerprint: currentFingerprint, checkedAt: 900 },
  };

  expect(
    selectProvidersForPing(
      [
        { id: 'fresh', configurationFingerprint: currentFingerprint },
        { id: 'changed', configurationFingerprint: currentFingerprint },
        { id: 'missing', configurationFingerprint: currentFingerprint },
        { id: 'stale', configurationFingerprint: currentFingerprint },
        { id: 'new', configurationFingerprint: currentFingerprint },
      ],
      snapshots,
      2_000,
      1_000,
    ),
  ).toEqual([
    { id: 'changed', configurationFingerprint: currentFingerprint },
    { id: 'missing', configurationFingerprint: currentFingerprint },
    { id: 'stale', configurationFingerprint: currentFingerprint },
    { id: 'new', configurationFingerprint: currentFingerprint },
  ]);
});

test('provider configuration fingerprints are stable across object key order', () => {
  expect(
    fingerprintProviderConfiguration({
      capabilities: { responses: true, chat: true },
      baseUrl: 'https://example.test',
    }),
  ).toBe(
    fingerprintProviderConfiguration({
      baseUrl: 'https://example.test',
      capabilities: { chat: true, responses: true },
    }),
  );
});

test('drawer async results are accepted only by the captured session and provider', () => {
  const sessions = createDrawerSessionCoordinator();
  sessions.begin('provider-1');
  const firstSession = sessions.capture();

  expect(firstSession).not.toBeNull();
  expect(sessions.isCurrent(firstSession!)).toBe(true);

  const secondSession = sessions.begin('provider-2');

  expect(sessions.isCurrent(firstSession!)).toBe(false);
  expect(sessions.isCurrent(secondSession)).toBe(true);
  expect(secondSession.providerId).toBe('provider-2');
  expect(secondSession.revision).toBeGreaterThan(firstSession!.revision);
});

test('reopening a create drawer invalidates async work from the previous create session', () => {
  const sessions = createDrawerSessionCoordinator();
  const previous = sessions.begin(null);
  const current = sessions.begin(null);

  expect(sessions.isCurrent(previous)).toBe(false);
  expect(sessions.isCurrent(current)).toBe(true);
});

test('active save remains busy across drawer sessions until the original request settles', () => {
  const sessions = createDrawerSessionCoordinator() as ReturnType<
    typeof createDrawerSessionCoordinator
  > & {
    beginSave?: () => {
      readonly revision: number;
      readonly session: { providerId: string | null };
    } | null;
    finishSave?: (request: { readonly revision: number }) => boolean;
    isSaving?: () => boolean;
  };
  sessions.begin('provider-1');

  expect(sessions.beginSave).toBeFunction();
  expect(sessions.finishSave).toBeFunction();
  expect(sessions.isSaving).toBeFunction();
  if (!sessions.beginSave || !sessions.finishSave || !sessions.isSaving) {
    return;
  }

  const firstSave = sessions.beginSave();
  expect(firstSave).not.toBeNull();
  expect(sessions.isSaving()).toBe(true);

  sessions.begin('provider-2');
  expect(sessions.beginSave()).toBeNull();
  expect(sessions.isSaving()).toBe(true);

  expect(sessions.finishSave(firstSave!)).toBe(true);
  expect(sessions.isSaving()).toBe(false);
  expect(sessions.beginSave()?.session.providerId).toBe('provider-2');
});

test('delete starts only once per provider while remaining independent across providers', () => {
  const deletes = createKeyedRequestCoordinator<string>();
  const first = deletes.begin('provider-1');

  expect(first).not.toBeNull();
  expect(deletes.begin('provider-1')).toBeNull();
  expect(deletes.isBusy('provider-1')).toBe(true);
  expect(deletes.begin('provider-2')).not.toBeNull();
  expect(deletes.isBusy('provider-2')).toBe(true);
});

test('only the active delete request can clear its provider busy state', () => {
  const deletes = createKeyedRequestCoordinator<string>();
  const request = deletes.begin('provider-1');

  expect(request).not.toBeNull();
  expect(deletes.finish({ key: 'provider-1', revision: -1 })).toBe(false);
  expect(deletes.isBusy('provider-1')).toBe(true);
  expect(deletes.finish(request!)).toBe(true);
  expect(deletes.isBusy('provider-1')).toBe(false);
  expect(deletes.begin('provider-1')).not.toBeNull();
});
