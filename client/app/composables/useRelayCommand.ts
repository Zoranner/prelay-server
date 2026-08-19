import { invoke } from "@tauri-apps/api/core";
import { computed, readonly, ref, type ComputedRef, type Ref } from "vue";

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
  pending: ComputedRef<boolean>;
  error: Ref<RelayError | null>;
}

const managementApiError = ref<RelayError | null>(null);

export function useRelayManagementApiStatus() {
  return { error: readonly(managementApiError) };
}

export function useRelayCommand(): CommandState & {
  invokeCommand<T>(
    command: RelayCommand,
    payload?: Record<string, unknown>,
  ): Promise<T>;
} {
  const pendingRequests = ref(0);
  const pending = computed(() => pendingRequests.value > 0);
  const error = ref<RelayError | null>(null);

  async function invokeCommand<T>(
    command: RelayCommand,
    payload?: Record<string, unknown>,
  ): Promise<T> {
    pendingRequests.value += 1;
    error.value = null;
    managementApiError.value = null;
    try {
      return await invoke<T>(command, payload);
    } catch (caught) {
      const relayError = toRelayError(caught);
      error.value = relayError;
      if (relayError.code === "network_error") {
        managementApiError.value = relayError;
      }
      throw relayError;
    } finally {
      pendingRequests.value = Math.max(0, pendingRequests.value - 1);
    }
  }

  return { pending, error, invokeCommand };
}
