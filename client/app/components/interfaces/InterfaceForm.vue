<script setup lang="ts">
import type { InterfaceModel, Provider, RelayInterface } from "~/stores/relay";

const props = defineProps<{
  interface?: RelayInterface | null;
  providers: Provider[];
  pending?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { id?: string; name: string; protocol: string; models: InterfaceModel[] }];
  cancel: [];
}>();

const name = ref("");
const protocol = ref("openai");
const models = ref<InterfaceModel[]>([]);
const availableProviders = computed(() => props.providers.filter((provider) => provider.models.length > 0));

function modelsForProvider(providerId: string) {
  return availableProviders.value.find((provider) => provider.id === providerId)?.models ?? [];
}

function ensureMappingSelection(model: InterfaceModel) {
  const provider = availableProviders.value.find((item) => item.id === model.provider_id) ?? availableProviders.value[0];
  if (!provider) return false;

  model.provider_id = provider.id;
  if (!provider.models.some((item) => item.model_name === model.upstream_model)) {
    model.upstream_model = provider.models[0]!.model_name;
  }
  return true;
}

watch(
  () => props.interface,
  (current) => {
    name.value = current?.name ?? "";
    protocol.value = current?.protocol ?? "openai";
    models.value = current?.models.map((model) => ({ ...model })) ?? [];
  },
  { immediate: true },
);

watch(
  availableProviders,
  () => {
    models.value = models.value.filter(ensureMappingSelection);
  },
  { immediate: true },
);

function addModel() {
  const provider = availableProviders.value[0];
  if (!provider) return;
  const upstream = provider.models[0]!.model_name;
  models.value.push({ model_name: upstream, upstream_model: upstream, provider_id: provider.id });
}

function submit() {
  emit("save", {
    ...(props.interface ? { id: props.interface.id } : {}),
    name: name.value.trim(),
    protocol: protocol.value,
    models: models.value.map((model) => ({
      model_name: model.model_name.trim() || model.upstream_model.trim(),
      upstream_model: model.upstream_model.trim(),
      provider_id: model.provider_id,
    })),
  });
}
</script>

<template>
  <form class="space-y-4" @submit.prevent="submit">
    <div class="grid gap-4 sm:grid-cols-2">
      <label class="field">
        <span>名称</span>
        <input v-model="name" required autocomplete="off" />
      </label>
      <label class="field">
        <span>协议</span>
        <select v-model="protocol">
          <option value="openai">OpenAI</option>
          <option value="anthropic">Anthropic</option>
        </select>
      </label>
    </div>
    <section class="space-y-3">
      <div class="flex items-center justify-between gap-3">
        <h3 class="text-sm font-medium">模型映射</h3>
        <button class="button-secondary" type="button" :disabled="!availableProviders.length" @click="addModel">添加映射</button>
      </div>
      <p v-if="!availableProviders.length" class="text-sm text-amber-300">请先配置包含模型的供应商。</p>
      <div v-for="(model, index) in models" :key="`${model.provider_id}-${index}`" class="grid gap-2 sm:grid-cols-[1fr_1fr_1fr_auto]">
        <select v-model="model.provider_id" aria-label="供应商" @change="ensureMappingSelection(model)">
          <option v-for="provider in availableProviders" :key="provider.id" :value="provider.id">{{ provider.name }}</option>
        </select>
        <select v-model="model.upstream_model" required aria-label="上游模型">
          <option v-for="upstream in modelsForProvider(model.provider_id)" :key="upstream.id" :value="upstream.model_name">{{ upstream.model_name }}</option>
        </select>
        <input v-model="model.model_name" placeholder="对外模型名" aria-label="对外模型名" />
        <button class="button-danger" type="button" @click="models.splice(index, 1)">移除</button>
      </div>
    </section>
    <div class="flex justify-end gap-2">
      <button class="button-secondary" type="button" @click="emit('cancel')">取消</button>
      <button class="button-primary" :disabled="pending" type="submit">{{ pending ? "保存中" : "保存接口" }}</button>
    </div>
  </form>
</template>
