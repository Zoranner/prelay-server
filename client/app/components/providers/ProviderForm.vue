<script setup lang="ts">
import type { Provider } from "~/stores/relay";

const props = defineProps<{
  provider?: Provider | null;
  pending?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { id?: string; name: string; provider_type: string; base_url: string; api_key: string; models: string[] }];
  cancel: [];
}>();

const name = ref("");
const providerType = ref("openai_compatible");
const baseUrl = ref("");
const apiKey = ref("");
const modelsText = ref("");

watch(
  () => props.provider,
  (provider) => {
    name.value = provider?.name ?? "";
    providerType.value = provider?.provider_type ?? "openai_compatible";
    baseUrl.value = provider?.base_url ?? "";
    apiKey.value = "";
    modelsText.value = provider?.models.map((model) => model.model_name).join("\n") ?? "";
  },
  { immediate: true },
);

function submit() {
  emit("save", {
    ...(props.provider ? { id: props.provider.id } : {}),
    name: name.value.trim(),
    provider_type: providerType.value,
    base_url: baseUrl.value.trim(),
    api_key: apiKey.value,
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
    <div class="flex justify-end gap-2">
      <button class="button-secondary" type="button" @click="emit('cancel')">取消</button>
      <button class="button-primary" :disabled="pending" type="submit">{{ pending ? "保存中" : "保存供应商" }}</button>
    </div>
  </form>
</template>
