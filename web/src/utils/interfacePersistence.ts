import type { CreateInterfaceRequest, UpdateInterfaceRequest } from '../api';

export interface InterfacePersistenceApi {
  createInterface(payload: CreateInterfaceRequest): Promise<unknown>;
  updateInterface(id: string, payload: UpdateInterfaceRequest): Promise<unknown>;
}

export type InterfacePersistenceCommand =
  | { type: 'create'; payload: CreateInterfaceRequest }
  | { type: 'update'; id: string; payload: UpdateInterfaceRequest };

export async function persistInterface(
  api: InterfacePersistenceApi,
  command: InterfacePersistenceCommand,
): Promise<void> {
  if (command.type === 'create') {
    await api.createInterface(command.payload);
    return;
  }
  await api.updateInterface(command.id, command.payload);
}
