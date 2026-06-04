<template>
  <div class="px-6 py-5 space-y-4">
    <div class="grid grid-cols-2 gap-4">
      <Input v-model="form.alias" label="下游模型别名" placeholder="例如：coder" mono />
      <Input v-model="form.upstream_model" label="上游模型" placeholder="例如：deepseek-chat" mono />
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
            <span class="font-mono text-xs text-stone-500 truncate">{{ alias.upstream_model }}</span>
          </div>
          <div class="mt-1 text-xs text-stone-400 truncate">
            {{ providerLabel(alias.provider_id) }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { configApi, type ModelAliasResponse, type ProviderConfig } from '../api';
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
const loadingAliases = ref(false);

onMounted(() => {
  loadProviders();
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

async function submit() {
  message.value = null;

  if (!form.value.alias.trim() || !form.value.provider_id.trim() || !form.value.upstream_model.trim()) {
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
    await loadAliases();
  } catch {
    message.value = { type: 'error', text: '模型别名创建失败，请检查 Provider ID 和别名是否重复。' };
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
</script>
