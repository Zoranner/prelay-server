<template>
  <div class="px-6 py-5 space-y-4">
    <div class="grid grid-cols-2 gap-4">
      <Input v-model="form.alias" label="下游模型别名" placeholder="例如：coder" mono />
      <Input
        v-model="form.upstream_model"
        label="上游模型"
        placeholder="例如：deepseek-chat"
        mono
      />
    </div>

    <div>
      <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide">
        Provider
      </label>
      <select
        v-model="form.provider_id"
        class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm bg-white text-stone-800 focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c]"
      >
        <option value="">选择已有配置</option>
        <option v-for="provider in providers" :key="provider.id" :value="provider.id">
          {{ provider.name }} · {{ provider.provider_type }}
        </option>
      </select>
    </div>

    <Input
      v-model="protocolsText"
      label="下游协议"
      placeholder="responses, chat_completions, anthropic_messages"
      mono
    />

    <div
      v-if="selectedProviderModel"
      class="rounded-lg border border-stone-100 bg-stone-50/70 px-3 py-2 text-xs text-stone-500"
    >
      <div class="flex items-center justify-between gap-3">
        <span>当前 Provider 上游协议</span>
        <span class="font-mono text-stone-700">{{ selectedProviderModel.upstream_protocol }}</span>
      </div>
      <div class="mt-2 flex flex-wrap items-center gap-1.5">
        <span class="mr-1 text-stone-400">可服务</span>
        <span
          v-for="protocol in selectedProviderModel.downstream_protocols"
          :key="protocol"
          class="font-mono text-[11px] px-1.5 py-0.5 rounded border border-stone-200 bg-white text-stone-500"
        >
          {{ protocol }}
        </span>
      </div>
      <div class="mt-2 flex flex-wrap items-center gap-1.5">
        <span class="mr-1 text-stone-400">能力</span>
        <span
          v-for="capability in enabledCapabilityChips(selectedProviderModel)"
          :key="capability.key"
          class="text-[11px] px-1.5 py-0.5 rounded border border-[#9fc9b2] bg-[#f2f8f5] text-[#256047]"
        >
          {{ capability.label }}
        </span>
        <span
          v-if="enabledCapabilityChips(selectedProviderModel).length === 0"
          class="text-[11px] px-1.5 py-0.5 rounded border border-stone-200 bg-white text-stone-400"
        >
          无能力声明
        </span>
      </div>
      <div class="mt-2 grid grid-cols-2 gap-2 text-[11px]">
        <div class="rounded border border-stone-100 bg-white px-2 py-1">
          <span class="text-stone-400">Context</span>
          <span class="ml-1 font-mono text-stone-700">
            {{ formatTokenLimit(selectedProviderModel.capabilities?.max_context_tokens) }}
          </span>
        </div>
        <div class="rounded border border-stone-100 bg-white px-2 py-1">
          <span class="text-stone-400">Output</span>
          <span class="ml-1 font-mono text-stone-700">
            {{ formatTokenLimit(selectedProviderModel.capabilities?.max_output_tokens) }}
          </span>
        </div>
      </div>
    </div>

    <Alert v-if="message" :type="message.type">
      {{ message.text }}
    </Alert>

    <Button block :loading="creating" @click="submit">
      {{ creating ? '创建中…' : '创建模型别名' }}
    </Button>

    <div class="border-t border-stone-100 pt-4">
      <div class="flex items-center justify-between mb-3">
        <h3 class="text-sm font-semibold text-stone-700">已有别名</h3>
        <button
          type="button"
          class="text-xs text-[#1a5c5c] hover:text-[#134848]"
          :disabled="loadingAliases"
          @click="loadAliases"
        >
          {{ loadingAliases ? '刷新中…' : '刷新' }}
        </button>
      </div>

      <div v-if="aliases.length === 0" class="text-sm text-stone-400 py-3">暂无模型别名</div>
      <div v-else class="space-y-2">
        <div
          v-for="alias in aliases"
          :key="alias.alias"
          class="border border-stone-100 rounded-lg px-3 py-2 bg-stone-50/60"
        >
          <div class="flex items-center justify-between gap-3">
            <span class="font-mono text-sm text-stone-800 truncate">{{ alias.alias }}</span>
            <span class="font-mono text-xs text-stone-500 truncate">{{
              alias.upstream_model
            }}</span>
          </div>
          <div class="mt-1 text-xs text-stone-400 truncate">
            {{ providerLabel(alias.provider_id) }}
          </div>
          <div
            v-if="catalogEntryForAlias(alias)"
            class="mt-2 flex flex-wrap items-center gap-1.5 text-xs"
          >
            <span class="font-mono text-[11px] px-1.5 py-0.5 rounded bg-stone-100 text-stone-500">
              {{ catalogEntryForAlias(alias)?.upstream_protocol }}
            </span>
            <span
              class="text-[11px] px-1.5 py-0.5 rounded border border-stone-200 bg-white text-stone-500"
            >
              {{ capabilitySummaryForAlias(alias) }}
            </span>
            <span class="font-mono text-[11px] px-1.5 py-0.5 rounded bg-white text-stone-400">
              Context {{ contextLimitForAlias(alias) }}
            </span>
            <span class="font-mono text-[11px] px-1.5 py-0.5 rounded bg-white text-stone-400">
              Output {{ outputLimitForAlias(alias) }}
            </span>
          </div>
          <div class="mt-2 flex flex-wrap gap-1.5">
            <span
              v-for="protocol in alias.downstream_protocols"
              :key="protocol"
              class="font-mono text-[11px] px-1.5 py-0.5 rounded border border-stone-200 bg-white text-stone-500"
            >
              {{ protocol }}
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import {
  configApi,
  modelsApi,
  type ModelAliasResponse,
  type ModelCatalogEntry,
  type ProviderConfig,
} from '../api';
import {
  enabledCapabilityChips,
  enabledCapabilitySummary,
  formatTokenLimit,
} from '../utils/modelCapabilities';
import { Alert, Button, Input } from './base';

const form = ref({
  alias: '',
  provider_id: '',
  upstream_model: '',
});
const protocolsText = ref('responses, chat_completions, anthropic_messages');
const creating = ref(false);
const message = ref<{ type: 'success' | 'error'; text: string } | null>(null);
const providers = ref<ProviderConfig[]>([]);
const aliases = ref<ModelAliasResponse[]>([]);
const modelCatalog = ref<ModelCatalogEntry[]>([]);
const loadingAliases = ref(false);

onMounted(() => {
  loadProviders().then(loadModelCatalog);
  loadAliases();
});

async function loadProviders() {
  try {
    const response = await configApi.list();
    providers.value = response.data;
  } catch {
    message.value = { type: 'error', text: 'Provider 列表加载失败。' };
  }
}

async function loadModelCatalog() {
  const token = providers.value.find((provider) => provider.token)?.token;
  if (!token) {
    modelCatalog.value = [];
    return;
  }

  try {
    const response = await modelsApi.list(token);
    modelCatalog.value = response.data.data;
  } catch {
    modelCatalog.value = [];
  }
}

async function loadAliases() {
  loadingAliases.value = true;
  try {
    const response = await configApi.listModelAliases();
    aliases.value = response.data;
  } catch {
    message.value = { type: 'error', text: '模型别名列表加载失败。' };
  } finally {
    loadingAliases.value = false;
  }
}

const selectedProviderModel = computed(() => {
  if (!form.value.provider_id) {
    return null;
  }
  return (
    modelCatalog.value.find(
      (model) => model.provider_id === form.value.provider_id && model.entry_type === 'provider',
    ) ?? null
  );
});

async function submit() {
  message.value = null;

  if (
    !form.value.alias.trim() ||
    !form.value.provider_id.trim() ||
    !form.value.upstream_model.trim()
  ) {
    message.value = { type: 'error', text: '请填写别名、Provider ID 和上游模型。' };
    return;
  }

  creating.value = true;
  try {
    const protocols = protocolsText.value
      .split(',')
      .map((protocol) => protocol.trim())
      .filter(Boolean);
    const response = await configApi.createModelAlias({
      alias: form.value.alias.trim(),
      provider_id: form.value.provider_id.trim(),
      upstream_model: form.value.upstream_model.trim(),
      downstream_protocols: protocols,
    });
    message.value = { type: 'success', text: `模型别名 ${response.data.alias} 已创建。` };
    form.value.alias = '';
    form.value.upstream_model = '';
    await loadModelCatalog();
    await loadAliases();
  } catch {
    message.value = {
      type: 'error',
      text: '模型别名创建失败，请检查 Provider ID 和别名是否重复。',
    };
  } finally {
    creating.value = false;
  }
}

function providerLabel(providerId: string) {
  const provider = providers.value.find((item) => item.id === providerId);
  if (!provider) {
    return providerId;
  }
  return `${provider.name} · ${provider.provider_type}`;
}

function catalogEntryForAlias(alias: ModelAliasResponse) {
  return (
    modelCatalog.value.find(
      (model) => model.provider_id === alias.provider_id && model.id === alias.alias,
    ) ?? null
  );
}

function capabilitySummaryForAlias(alias: ModelAliasResponse) {
  const entry = catalogEntryForAlias(alias);
  return entry ? enabledCapabilitySummary(entry) : '无能力声明';
}

function contextLimitForAlias(alias: ModelAliasResponse) {
  return formatTokenLimit(catalogEntryForAlias(alias)?.capabilities?.max_context_tokens);
}

function outputLimitForAlias(alias: ModelAliasResponse) {
  return formatTokenLimit(catalogEntryForAlias(alias)?.capabilities?.max_output_tokens);
}
</script>
