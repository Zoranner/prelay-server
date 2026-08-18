<script setup lang="ts">
import type { Provider } from "~/stores/relay";
import type { UpstreamProtocol } from "~/stores/relay";
import { providerProtocolOptions } from "~/utils/providerCapabilities";

defineProps<{ providers: Provider[]; pending?: boolean }>();
const emit = defineEmits<{
  edit: [provider: Provider];
  remove: [provider: Provider];
  ping: [provider: Provider];
  discover: [provider: Provider];
  testProtocol: [payload: { provider: Provider; protocol: UpstreamProtocol }];
}>();

const protocolSelections = reactive<Record<string, UpstreamProtocol>>({});

function protocolFor(provider: Provider): UpstreamProtocol {
  return protocolSelections[provider.id] ?? providerProtocolOptions(provider)[0] ?? "openai";
}
</script>

<template>
  <div v-if="providers.length" class="divide-y divide-slate-800 border-y border-slate-800">
    <article v-for="provider in providers" :key="provider.id" class="py-4">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 class="font-medium text-slate-100">{{ provider.name }}</h3>
          <p class="mt-1 text-sm text-slate-400">{{ provider.base_url }}</p>
          <p class="mt-1 text-xs text-slate-500">密钥：{{ provider.api_key_masked }}</p>
        </div>
        <div class="flex flex-wrap gap-2">
          <button class="button-secondary" :disabled="pending" @click="emit('ping', provider)">连通性</button>
          <button class="button-secondary" :disabled="pending" @click="emit('discover', provider)">发现模型</button>
          <select v-model="protocolSelections[provider.id]" :aria-label="`${provider.name} 上游协议`" class="min-w-24">
            <option v-for="protocol in providerProtocolOptions(provider)" :key="protocol" :value="protocol">{{ protocol }}</option>
          </select>
          <button class="button-secondary" :disabled="pending" @click="emit('testProtocol', { provider, protocol: protocolFor(provider) })">协议测试</button>
          <button class="button-secondary" :disabled="pending" @click="emit('edit', provider)">编辑</button>
          <button class="button-danger" :disabled="pending" @click="emit('remove', provider)">删除</button>
        </div>
      </div>
      <p class="mt-3 text-sm text-slate-300">
        {{ provider.models.length ? provider.models.map((model) => model.model_name).join("、") : "尚未配置模型" }}
      </p>
    </article>
  </div>
  <p v-else class="empty-state">尚未配置供应商。</p>
</template>
