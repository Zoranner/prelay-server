<script setup lang="ts">
import type { Provider } from "~/stores/relay";

type ProviderFormPayload = {
  id?: string;
  name: string;
  provider_type: string;
  base_url: string;
  api_key: string;
  models: string[];
};

const { error, pending, invokeCommand } = useRelayCommand();
const providers = ref<Provider[]>([]);
const editingProvider = ref<Provider | null>(null);
const showForm = ref(false);
const operationMessage = ref<string | null>(null);

async function loadProviders() {
  try {
    providers.value = await invokeCommand<Provider[]>("providers_list");
  } catch {
    // The command composable exposes the error to this view.
  }
}

async function saveProvider(payload: ProviderFormPayload) {
  operationMessage.value = null;
  try {
    await invokeCommand("providers_save", {
      ...(payload.id ? { providerId: payload.id } : {}),
      input: {
        name: payload.name,
        provider_type: payload.provider_type,
        base_url: payload.base_url,
        ...(payload.api_key ? { api_key: payload.api_key } : {}),
        models: payload.models,
      },
    });
    showForm.value = false;
    editingProvider.value = null;
    await loadProviders();
    operationMessage.value = "供应商已保存。";
  } catch {
    // The command composable exposes the error to this view.
  } finally {
    payload.api_key = "";
  }
}

async function deleteProvider(provider: Provider) {
  if (!confirm(`删除供应商“${provider.name}”及其模型？`)) return;
  operationMessage.value = null;
  try {
    await invokeCommand("providers_delete", { providerId: provider.id });
    await loadProviders();
    operationMessage.value = "供应商已删除。";
  } catch {
    // The command composable exposes the error to this view.
  }
}

async function runProviderOperation(
  command: "providers_ping" | "providers_discover_models" | "providers_test_protocol",
  provider: Provider,
) {
  operationMessage.value = null;
  try {
    const result = await invokeCommand<{ message?: string; models?: string[] }>(command, {
      providerId: provider.id,
      ...(command === "providers_test_protocol" ? { protocol: provider.provider_type } : {}),
    });
    if (result.models?.length) {
      operationMessage.value = `发现模型：${result.models.join("、")}`;
    } else {
      operationMessage.value = result.message ?? "操作完成。";
    }
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
  <main class="page">
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 class="page-heading">供应商</h1>
        <p class="page-subheading">密钥仅在保存时传递，列表始终显示脱敏值。</p>
      </div>
      <button class="button-primary" @click="newProvider">添加供应商</button>
    </div>

    <section v-if="showForm" class="panel mt-6">
      <h2 class="mb-5 font-medium text-white">{{ editingProvider ? "编辑供应商" : "添加供应商" }}</h2>
      <ProvidersProviderForm
        :provider="editingProvider"
        :pending="pending"
        @save="saveProvider"
        @cancel="showForm = false"
      />
    </section>

    <p v-if="error" class="mt-5 border border-rose-900 bg-rose-950/40 p-3 text-sm text-rose-200">{{ error.message }}</p>
    <p v-else-if="operationMessage" class="mt-5 border border-emerald-900 bg-emerald-950/30 p-3 text-sm text-emerald-200">{{ operationMessage }}</p>

    <section class="mt-6">
      <ProvidersProviderList
        :providers="providers"
        :pending="pending"
        @edit="editProvider"
        @remove="deleteProvider"
        @ping="runProviderOperation('providers_ping', $event)"
        @discover="runProviderOperation('providers_discover_models', $event)"
        @test-protocol="runProviderOperation('providers_test_protocol', $event)"
      />
    </section>
  </main>
</template>
