<script setup lang="ts">
import type { Provider, ProviderCapabilities, UpstreamProtocol } from "~/stores/relay";

const props = defineProps<{
  provider?: Provider | null;
  pending?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { id?: string; name: string; provider_type: string; base_url: string; api_key: string; capabilities: ProviderCapabilities; models: string[] }];
  cancel: [];
}>();

const name = ref("");
const providerType = ref("openai_compatible");
const baseUrl = ref("");
const apiKey = ref("");
const modelsText = ref("");
const allUpstreamProtocols: UpstreamProtocol[] = ["responses", "openai", "anthropic"];
const upstreamProtocols = ref<UpstreamProtocol[]>([]);
const protocolBaseUrls = reactive<Record<UpstreamProtocol, string>>({ responses: "", openai: "", anthropic: "" });
const toolCalls = ref<boolean | null>(null);
const reasoning = ref<boolean | null>(null);
const toolChoice = ref<boolean | null>(null);
const parallelToolCalls = ref<boolean | null>(null);
const systemMessages = ref<boolean | null>(null);
const structuredOutputs = ref<boolean | null>(null);
const streamingUsage = ref<boolean | null>(null);
const maxContextTokens = ref<number | null>(null);
const maxOutputTokens = ref<number | null>(null);

watch(
  () => props.provider,
  (provider) => {
    name.value = provider?.name ?? "";
    providerType.value = provider?.provider_type ?? "openai_compatible";
    baseUrl.value = provider?.base_url ?? "";
    apiKey.value = "";
    modelsText.value = provider?.models.map((model) => model.model_name).join("\n") ?? "";
    const capabilities = provider?.capabilities;
    upstreamProtocols.value = (capabilities?.upstream_protocols ?? []).filter(isUpstreamProtocol);
    protocolBaseUrls.responses = capabilities?.protocol_base_urls?.responses ?? "";
    protocolBaseUrls.openai = capabilities?.protocol_base_urls?.openai ?? "";
    protocolBaseUrls.anthropic = capabilities?.protocol_base_urls?.anthropic ?? "";
    toolCalls.value = capabilities?.tool_calls ?? null;
    reasoning.value = capabilities?.reasoning ?? null;
    toolChoice.value = capabilities?.tool_choice ?? null;
    parallelToolCalls.value = capabilities?.parallel_tool_calls ?? null;
    systemMessages.value = capabilities?.system_messages ?? null;
    structuredOutputs.value = capabilities?.structured_outputs ?? null;
    streamingUsage.value = capabilities?.streaming_usage ?? null;
    maxContextTokens.value = capabilities?.max_context_tokens ?? null;
    maxOutputTokens.value = capabilities?.max_output_tokens ?? null;
  },
  { immediate: true },
);

function isUpstreamProtocol(value: string): value is UpstreamProtocol {
  return value === "responses" || value === "openai" || value === "anthropic";
}

function trimOrNull(value: string): string | null {
  return value.trim() || null;
}

function submit() {
  emit("save", {
    ...(props.provider ? { id: props.provider.id } : {}),
    name: name.value.trim(),
    provider_type: providerType.value,
    base_url: baseUrl.value.trim(),
    api_key: apiKey.value,
    capabilities: {
      upstream_protocols: upstreamProtocols.value,
      protocol_base_urls: {
        responses: trimOrNull(protocolBaseUrls.responses),
        openai: trimOrNull(protocolBaseUrls.openai),
        anthropic: trimOrNull(protocolBaseUrls.anthropic),
      },
      tool_calls: toolCalls.value,
      reasoning: reasoning.value,
      tool_choice: toolChoice.value,
      parallel_tool_calls: parallelToolCalls.value,
      system_messages: systemMessages.value,
      structured_outputs: structuredOutputs.value,
      streaming_usage: streamingUsage.value,
      max_context_tokens: maxContextTokens.value,
      max_output_tokens: maxOutputTokens.value,
    },
    models: modelsText.value
      .split(/\r?\n|,/)
      .map((model) => model.trim())
      .filter(Boolean),
  });
  apiKey.value = "";
}
</script>

<template>
  <form class="space-y-4" @submit.prevent="submit">
    <label class="field">
      <span>名称</span>
      <input v-model="name" required autocomplete="off" placeholder="例如 DeepSeek" />
    </label>
    <div class="grid gap-4 sm:grid-cols-2">
      <label class="field">
        <span>供应商类型</span>
        <select v-model="providerType">
          <option value="openai_compatible">OpenAI 兼容</option>
          <option value="anthropic">Anthropic</option>
        </select>
      </label>
      <label class="field">
        <span>服务地址</span>
        <input v-model="baseUrl" required inputmode="url" autocomplete="url" placeholder="https://api.example.com" />
      </label>
    </div>
    <label class="field">
      <span>密钥 <small v-if="provider">留空则保持现有脱敏密钥</small></span>
      <input v-model="apiKey" type="password" autocomplete="new-password" :required="!provider" />
    </label>
    <label class="field">
      <span>模型</span>
      <textarea v-model="modelsText" rows="4" placeholder="每行一个模型名称" />
    </label>
    <fieldset class="space-y-3 border border-slate-800 p-4">
      <legend class="px-1 text-sm text-slate-300">上游能力覆盖</legend>
      <div class="grid gap-3 sm:grid-cols-3">
        <label v-for="protocol in allUpstreamProtocols" :key="protocol" class="flex items-center gap-2 text-sm text-slate-300">
          <input v-model="upstreamProtocols" type="checkbox" :value="protocol" />
          {{ protocol }}
        </label>
      </div>
      <div class="grid gap-3 sm:grid-cols-3">
        <label v-for="protocol in allUpstreamProtocols" :key="`${protocol}-base-url`" class="field">
          <span>{{ protocol }} 服务地址</span>
          <input v-model="protocolBaseUrls[protocol]" inputmode="url" autocomplete="url" placeholder="留空则使用服务地址" />
        </label>
      </div>
      <div class="grid gap-3 sm:grid-cols-2">
        <label class="field"><span>最大上下文 Token</span><input v-model.number="maxContextTokens" min="1" type="number" /></label>
        <label class="field"><span>最大输出 Token</span><input v-model.number="maxOutputTokens" min="1" type="number" /></label>
      </div>
      <div class="grid gap-3 sm:grid-cols-2">
        <label class="flex items-center gap-2 text-sm text-slate-300"><input v-model="toolCalls" type="checkbox" />工具调用</label>
        <label class="flex items-center gap-2 text-sm text-slate-300"><input v-model="reasoning" type="checkbox" />推理</label>
        <label class="flex items-center gap-2 text-sm text-slate-300"><input v-model="toolChoice" type="checkbox" />工具选择</label>
        <label class="flex items-center gap-2 text-sm text-slate-300"><input v-model="parallelToolCalls" type="checkbox" />并行工具调用</label>
        <label class="flex items-center gap-2 text-sm text-slate-300"><input v-model="systemMessages" type="checkbox" />系统消息</label>
        <label class="flex items-center gap-2 text-sm text-slate-300"><input v-model="structuredOutputs" type="checkbox" />结构化输出</label>
        <label class="flex items-center gap-2 text-sm text-slate-300"><input v-model="streamingUsage" type="checkbox" />流式用量</label>
      </div>
    </fieldset>
    <div class="flex justify-end gap-2">
      <button class="button-secondary" type="button" @click="emit('cancel')">取消</button>
      <button class="button-primary" :disabled="pending" type="submit">{{ pending ? "保存中" : "保存供应商" }}</button>
    </div>
  </form>
</template>
