<script setup lang="ts">
import type {
  Provider,
  ProviderCapabilities,
  UpstreamProtocol,
} from "~/stores/relay";
import {
  getProviderOperationFeedback,
  type ProviderOperationFeedback,
  type ProviderOperationResult,
} from "~/utils/providerOperations";
import { providerProtocolOptions } from "~/utils/providerCapabilities";
import { DrawerPanel, PageHeader, PageShell } from "~/components/base";

type ProviderFormPayload = {
  id?: string;
  name: string;
  provider_type: string;
  base_url: string;
  api_key: string;
  capabilities: ProviderCapabilities;
  models: string[];
};

const { error, pending, invokeCommand } = useRelayCommand();
const providers = ref<Provider[]>([]);
const editingProvider = ref<Provider | null>(null);
const showForm = ref(false);
const operationFeedback = ref<ProviderOperationFeedback | null>(null);

async function loadProviders() {
  try {
    providers.value = await invokeCommand<Provider[]>("providers_list");
  } catch {
    // The command composable exposes the error to this view.
  }
}

async function saveProvider(payload: ProviderFormPayload) {
  operationFeedback.value = null;
  try {
    await invokeCommand("providers_save", {
      ...(payload.id ? { providerId: payload.id } : {}),
      input: {
        name: payload.name,
        provider_type: payload.provider_type,
        base_url: payload.base_url,
        api_key: payload.api_key,
        capabilities: payload.capabilities,
        models: payload.models,
      },
    });
    showForm.value = false;
    editingProvider.value = null;
    await loadProviders();
    operationFeedback.value = {
      success: true,
      message: "供应商已保存。",
      metrics: null,
    };
  } catch {
    // The command composable exposes the error to this view.
  } finally {
    payload.api_key = "";
  }
}

async function deleteProvider(provider: Provider) {
  if (!confirm(`删除供应商“${provider.name}”及其模型？`)) return;
  operationFeedback.value = null;
  try {
    await invokeCommand("providers_delete", { providerId: provider.id });
    await loadProviders();
    operationFeedback.value = {
      success: true,
      message: "供应商已删除。",
      metrics: null,
    };
  } catch {
    // The command composable exposes the error to this view.
  }
}

async function runProviderOperation(
  command:
    "providers_ping" | "providers_discover_models" | "providers_test_protocol",
  provider: Provider,
  protocol?: UpstreamProtocol,
) {
  operationFeedback.value = null;
  try {
    const result = await invokeCommand<ProviderOperationResult>(command, {
      providerId: provider.id,
      ...(command === "providers_test_protocol"
        ? { protocol: protocol ?? providerProtocolOptions(provider)[0] }
        : {}),
    });
    operationFeedback.value = getProviderOperationFeedback(result);
    if (command === "providers_discover_models" && result.ok)
      await loadProviders();
  } catch {
    // The command composable exposes the error to this view.
  }
}

function editProvider(provider: Provider) {
  editingProvider.value = provider;
  showForm.value = true;
}

function newProvider() {
  editingProvider.value = null;
  showForm.value = true;
}

onMounted(loadProviders);
</script>

<template>
  <PageShell>
    <PageHeader
      title="供应商"
      description="添加上游服务，管理可用模型，做连接测试。"
    >
      <template #actions>
        <button
          class="button-secondary"
          :disabled="pending"
          @click="loadProviders"
        >
          {{ pending ? "刷新中..." : "刷新" }}
        </button>
        <button class="button-primary" @click="newProvider">添加供应商</button>
      </template>
    </PageHeader>
    <p v-if="error" class="notice notice--error">{{ error.message }}</p>
    <p
      v-else-if="operationFeedback"
      class="notice"
      :class="operationFeedback.success ? '' : 'notice--error'"
    >
      {{ operationFeedback.message }}
      <span v-if="operationFeedback.metrics">{{
        operationFeedback.metrics
      }}</span>
    </p>
    <section class="min-h-0 flex-1">
      <ProvidersProviderList
        :providers="providers"
        :pending="pending"
        @edit="editProvider"
        @remove="deleteProvider"
        @ping="runProviderOperation('providers_ping', $event)"
        @discover="runProviderOperation('providers_discover_models', $event)"
        @test-protocol="
          runProviderOperation(
            'providers_test_protocol',
            $event.provider,
            $event.protocol,
          )
        "
      />
    </section>
    <DrawerPanel
      :open="showForm"
      :title="editingProvider ? '编辑供应商' : '添加供应商'"
      :description="
        editingProvider
          ? '修改供应商连接信息；API Key 留空表示不更换。'
          : '填写连接信息，并维护模型清单。'
      "
      label="供应商配置"
      size="lg"
      @close="showForm = false"
    >
      <ProvidersProviderForm
        :provider="editingProvider"
        :pending="pending"
        @save="saveProvider"
        @cancel="showForm = false"
      />
    </DrawerPanel>
  </PageShell>
</template>
