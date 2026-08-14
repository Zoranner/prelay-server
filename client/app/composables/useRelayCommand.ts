import { invoke } from "@tauri-apps/api/core";

import { toRelayError, type RelayError } from "~/utils/errors";

export type RelayCommand =
  | "bootstrap"
  | "providers_list"
  | "providers_save"
  | "providers_delete"
  | "providers_ping"
  | "providers_discover_models"
  | "providers_test_protocol"
  | "interfaces_list"
  | "interfaces_save"
  | "interfaces_delete"
  | "interfaces_regenerate_token"
  | "stats_overview"
  | "stats_requests"
  | "stats_models"
  | "stats_providers"
  | "credential_rotate";

export interface CommandState {
  pending: Ref<boolean>;
  error: Ref<RelayError | null>;
}

export function useRelayCommand(): CommandState & {
  invokeCommand<T>(command: RelayCommand, payload?: Record<string, unknown>): Promise<T>;
} {
  const pending = ref(false);
  const error = ref<RelayError | null>(null);

  async function invokeCommand<T>(command: RelayCommand, payload?: Record<string, unknown>): Promise<T> {
    pending.value = true;
    error.value = null;
    try {
      return await invoke<T>(command, payload);
    } catch (caught) {
      const relayError = toRelayError(caught);
      error.value = relayError;
      throw relayError;
    } finally {
      pending.value = false;
    }
  }

  return { pending, error, invokeCommand };
}
