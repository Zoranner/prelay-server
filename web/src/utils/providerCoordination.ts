export interface KeyedRequestToken<TKey> {
  readonly key: TKey;
  readonly revision: number;
}

export interface KeyedRequestCoordinator<TKey> {
  begin(key: TKey, options?: { supersede?: boolean }): KeyedRequestToken<TKey> | null;
  isCurrent(request: KeyedRequestToken<TKey>): boolean;
  finish(request: KeyedRequestToken<TKey>): boolean;
  isBusy(key: TKey): boolean;
}

export interface ProviderPingCandidate {
  readonly id: string;
  readonly configurationFingerprint: string;
}

export interface ProviderPingSnapshot {
  readonly configurationFingerprint: string;
  readonly checkedAt?: number;
}

export interface DrawerSession {
  readonly providerId: string | null;
  readonly revision: number;
}

export interface DrawerSaveRequest {
  readonly revision: number;
  readonly session: DrawerSession;
}

export interface DrawerSessionCoordinator {
  begin(providerId: string | null): DrawerSession;
  capture(): DrawerSession | null;
  isCurrent(session: DrawerSession): boolean;
  beginSave(): DrawerSaveRequest | null;
  finishSave(request: DrawerSaveRequest): boolean;
  isSaving(): boolean;
}

export function createKeyedRequestCoordinator<TKey>(): KeyedRequestCoordinator<TKey> {
  const revisions = new Map<TKey, number>();
  const activeRevisions = new Map<TKey, number>();

  return {
    begin(key, options = {}) {
      if (activeRevisions.has(key) && !options.supersede) {
        return null;
      }

      const revision = (revisions.get(key) ?? 0) + 1;
      revisions.set(key, revision);
      activeRevisions.set(key, revision);
      return { key, revision };
    },
    isCurrent(request) {
      return activeRevisions.get(request.key) === request.revision;
    },
    finish(request) {
      if (activeRevisions.get(request.key) !== request.revision) {
        return false;
      }
      activeRevisions.delete(request.key);
      return true;
    },
    isBusy(key) {
      return activeRevisions.has(key);
    },
  };
}

export function fingerprintProviderConfiguration(configuration: unknown): string {
  return JSON.stringify(stabilizeConfiguration(configuration)) ?? 'undefined';
}

export function selectProvidersForPing(
  providers: readonly ProviderPingCandidate[],
  snapshots: Readonly<Record<string, ProviderPingSnapshot | undefined>>,
  now: number,
  freshForMs: number,
): ProviderPingCandidate[] {
  return providers.filter((provider) => {
    const snapshot = snapshots[provider.id];
    if (!snapshot) {
      return true;
    }
    if (snapshot.configurationFingerprint !== provider.configurationFingerprint) {
      return true;
    }
    if (snapshot.checkedAt === undefined) {
      return true;
    }
    return now - snapshot.checkedAt >= freshForMs;
  });
}

export function createDrawerSessionCoordinator(): DrawerSessionCoordinator {
  let revision = 0;
  let current: DrawerSession | null = null;
  let saveRevision = 0;
  let activeSaveRevision: number | null = null;

  return {
    begin(providerId) {
      revision += 1;
      current = { providerId, revision };
      return current;
    },
    capture() {
      return current ? { ...current } : null;
    },
    isCurrent(session) {
      return current?.revision === session.revision && current.providerId === session.providerId;
    },
    beginSave() {
      if (!current || activeSaveRevision !== null) {
        return null;
      }
      saveRevision += 1;
      activeSaveRevision = saveRevision;
      return {
        revision: saveRevision,
        session: { ...current },
      };
    },
    finishSave(request) {
      if (activeSaveRevision !== request.revision) {
        return false;
      }
      activeSaveRevision = null;
      return true;
    },
    isSaving() {
      return activeSaveRevision !== null;
    },
  };
}

function stabilizeConfiguration(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(stabilizeConfiguration);
  }
  if (value === null || typeof value !== 'object') {
    return value;
  }

  const object = value as Record<string, unknown>;
  return Object.fromEntries(
    Object.keys(object)
      .sort()
      .map((key) => [key, stabilizeConfiguration(object[key])]),
  );
}
