<template>
  <div class="px-6 py-5 space-y-5">
    <div class="space-y-3">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-end">
        <div class="flex-1">
          <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide">
            代理密钥
          </label>
          <select
            v-model="selectedToken"
            class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm bg-white text-stone-800 focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c]"
          >
            <option value="">选择本地已保存密钥</option>
            <option v-for="token in storedTokens" :key="token.token" :value="token.token">
              {{ token.name }} · {{ providerLabel(token.providerType) }}
            </option>
          </select>
        </div>
        <Button :loading="loading" @click="loadCatalog">
          {{ loading ? '刷新中…' : '刷新目录' }}
        </Button>
      </div>

      <Alert v-if="storedTokens.length === 0" type="warning">
        暂无本地密钥。先新建或查询一次密钥后，可在这里查看模型目录。
      </Alert>
      <Alert v-if="error" type="error">
        {{ error }}
      </Alert>
    </div>

    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-5">
      <div
        v-for="metric in metrics"
        :key="metric.label"
        class="rounded-xl border border-stone-100 bg-stone-50/70 px-4 py-3"
      >
        <p class="text-xs font-medium text-stone-400">{{ metric.label }}</p>
        <p class="text-2xl font-semibold text-stone-800 mt-2 tabular-nums">
          {{ metric.value }}
        </p>
      </div>
    </div>

    <div class="overflow-x-auto rounded-xl border border-stone-100">
      <table class="min-w-full divide-y divide-stone-100 text-sm">
        <thead class="bg-stone-50 text-xs font-medium text-stone-400">
          <tr>
            <th class="px-4 py-3 text-left whitespace-nowrap">模型</th>
            <th class="px-4 py-3 text-left whitespace-nowrap">类型</th>
            <th class="px-4 py-3 text-left whitespace-nowrap">Provider</th>
            <th class="px-4 py-3 text-left whitespace-nowrap">上游</th>
            <th class="px-4 py-3 text-left whitespace-nowrap">可服务协议</th>
            <th class="px-4 py-3 text-left whitespace-nowrap">能力</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-stone-100 bg-white">
          <tr v-if="!loading && catalog.length === 0">
            <td colspan="6" class="px-4 py-8 text-center text-stone-400">暂无模型目录数据</td>
          </tr>
          <tr v-for="model in catalog" :key="model.id" class="text-stone-600 hover:bg-stone-50/60">
            <td class="px-4 py-3 whitespace-nowrap">
              <div class="font-mono text-xs text-stone-800">{{ model.id }}</div>
              <div
                v-if="model.upstream_model !== model.id"
                class="mt-1 font-mono text-[11px] text-stone-400"
              >
                {{ model.upstream_model }}
              </div>
            </td>
            <td class="px-4 py-3 whitespace-nowrap">
              <span
                class="inline-flex rounded-full px-2 py-0.5 text-xs font-medium"
                :class="entryKindClass(model)"
              >
                {{ entryKindLabel(model) }}
              </span>
            </td>
            <td class="px-4 py-3 whitespace-nowrap">
              <div class="font-medium text-stone-700">{{ model.provider_name }}</div>
              <div class="mt-1 font-mono text-[11px] text-stone-400">{{ model.provider_id }}</div>
            </td>
            <td class="px-4 py-3 whitespace-nowrap">
              <span class="font-mono text-xs text-stone-700">
                {{ protocolLabel(model.upstream_protocol) }}
              </span>
            </td>
            <td class="px-4 py-3">
              <div class="flex min-w-[220px] flex-wrap gap-1.5">
                <span
                  v-for="protocol in model.downstream_protocols"
                  :key="protocol"
                  class="font-mono text-[11px] px-1.5 py-0.5 rounded border border-stone-200 bg-white text-stone-500"
                >
                  {{ protocolLabel(protocol) }}
                </span>
              </div>
            </td>
            <td class="px-4 py-3 whitespace-nowrap">
              <span
                class="text-[11px] px-1.5 py-0.5 rounded border"
                :class="
                  model.capabilities.tool_calls
                    ? 'border-[#9fc9b2] bg-[#f2f8f5] text-[#256047]'
                    : 'border-stone-200 bg-stone-50 text-stone-400'
                "
              >
                {{ model.capabilities.tool_calls ? '工具调用' : '无工具调用声明' }}
              </span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { modelsApi, type ModelCatalogEntry } from '../api';
import { getStoredTokens, providerLabel, type StoredToken } from '../utils/providers';
import { Alert, Button } from './base';

const storedTokens = ref<StoredToken[]>(getStoredTokens());
const selectedToken = ref(storedTokens.value[0]?.token ?? '');
const catalog = ref<ModelCatalogEntry[]>([]);
const loading = ref(false);
const error = ref('');

const providerEntries = computed(() =>
  catalog.value.filter((model) => model.entry_type === 'provider'),
);
const aliasEntries = computed(() => catalog.value.length - providerEntries.value.length);
const protocolCount = computed(
  () => new Set(catalog.value.flatMap((model) => model.downstream_protocols)).size,
);
const toolCallEntries = computed(
  () => catalog.value.filter((model) => model.capabilities.tool_calls).length,
);

const metrics = computed(() => [
  { label: '目录条目', value: catalog.value.length },
  { label: 'Provider', value: providerEntries.value.length },
  { label: '模型别名', value: aliasEntries.value },
  { label: '服务协议', value: protocolCount.value },
  { label: '工具调用', value: toolCallEntries.value },
]);

onMounted(() => {
  if (selectedToken.value) {
    loadCatalog();
  }
});

async function loadCatalog() {
  error.value = '';

  if (!selectedToken.value) {
    catalog.value = [];
    error.value = '请选择一个代理密钥。';
    return;
  }

  loading.value = true;
  try {
    const response = await modelsApi.list(selectedToken.value);
    catalog.value = response.data.data;
  } catch {
    catalog.value = [];
    error.value = '模型目录加载失败，请检查密钥是否仍然有效。';
  } finally {
    loading.value = false;
  }
}

function entryKindLabel(model: ModelCatalogEntry) {
  return model.entry_type === 'provider' ? 'Provider' : 'Alias';
}

function entryKindClass(model: ModelCatalogEntry) {
  return model.entry_type === 'provider'
    ? 'bg-[#f0f8f8] text-[#1a5c5c]'
    : 'bg-stone-100 text-stone-600';
}

function protocolLabel(protocol: string) {
  const labels: Record<string, string> = {
    chat_completions: 'Chat Completions',
    responses: 'Responses',
    anthropic_messages: 'Anthropic Messages',
    ollama_native: 'Ollama Native',
  };

  return labels[protocol] ?? protocol;
}
</script>
