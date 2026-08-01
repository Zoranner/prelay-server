import type { CreateConfigRequest, UpdateConfigRequest } from '../api';

export type ProviderPersistenceCommand =
  | { type: 'create'; payload: CreateConfigRequest }
  | { type: 'update'; id: string; payload: UpdateConfigRequest };

export interface ProviderPersistenceApi {
  create(payload: CreateConfigRequest): Promise<unknown>;
  update(id: string, payload: UpdateConfigRequest): Promise<unknown>;
}

export function persistProviderConfig(
  api: ProviderPersistenceApi,
  command: ProviderPersistenceCommand,
): Promise<unknown> {
  if (command.type === 'create') {
    return api.create(command.payload);
  }
  return api.update(command.id, command.payload);
}
