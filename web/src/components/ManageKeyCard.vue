<template>
  <!-- Lookup input -->
  <div class="px-6 py-5">
    <div v-if="storedTokens.length > 0" class="mb-3">
      <div class="flex items-center gap-2 mb-2">
        <span class="text-xs font-medium text-stone-400 uppercase tracking-wide"
          >本地存储的密钥</span
        >
        <span class="text-xs text-stone-400">(点击选择)</span>
      </div>
      <div class="flex flex-wrap gap-2">
        <Tag
          v-for="item in storedTokens"
          :key="item.token"
          :label="item.name"
          :dot="true"
          :dot-class="providerDotClass(item.providerType)"
          :variant="lookupToken === item.token ? 'primary' : 'default'"
          :title="item.token"
          @click="selectStored(item)"
        />
      </div>
    </div>
    <div class="flex gap-2">
      <Input
        v-model="lookupToken"
        placeholder="输入你的代理密钥…"
        mono
        flex
        @keydown.enter="lookup"
      />
      <Button :loading="looking" @click="lookup">
        {{ looking ? '查询中…' : '查询' }}
      </Button>
    </div>
    <Alert v-if="lookupError" type="error" class="mt-3">
      {{ lookupError }}
    </Alert>
  </div>

  <!-- Found config -->
  <div v-if="config" class="border-t border-stone-100">
    <!-- Detail view -->
    <div v-if="!editing" class="px-6 py-5 space-y-5">
      <dl class="grid grid-cols-2 gap-x-6 gap-y-4 text-sm">
        <Field term="名称" :value="config!.name || '（未命名）'" />
        <Field term="提供商">
          <Badge :provider-type="config!.provider_type" />
        </Field>
        <Field term="上游 Base URL" :value="config!.base_url" />
        <Field term="上游 API Key" :value="config!.api_key_masked" />
      </dl>

      <section class="rounded-xl border border-stone-100 bg-stone-50/60 px-3 py-3 space-y-3">
        <div class="flex items-start justify-between gap-3">
          <div>
            <h3 class="text-sm font-semibold text-stone-700">能力覆盖</h3>
            <p class="mt-0.5 text-xs text-stone-400">未覆盖表示沿用 Provider 默认能力</p>
          </div>
          <span
            class="shrink-0 rounded-full px-2 py-0.5 text-xs font-medium"
            :class="
              hasCapabilityOverrides ? 'bg-[#f2f8f5] text-[#256047]' : 'bg-stone-100 text-stone-400'
            "
          >
            {{ hasCapabilityOverrides ? '已设置覆盖' : '未设置覆盖' }}
          </span>
        </div>

        <div class="grid gap-2 sm:grid-cols-2">
          <div
            v-for="capability in booleanCapabilityOverrides"
            :key="capability.key"
            class="flex items-center justify-between gap-2 rounded-lg border border-stone-100 bg-white px-2.5 py-2"
          >
            <span class="text-xs font-medium text-stone-600">{{ capability.label }}</span>
            <span
              class="rounded-full px-2 py-0.5 text-xs font-medium"
              :class="booleanOverrideClass(capability.value)"
            >
              {{ booleanOverrideLabel(capability.value) }}
            </span>
          </div>
        </div>

        <div class="grid gap-2 sm:grid-cols-2">
          <div class="rounded-lg border border-stone-100 bg-white px-2.5 py-2">
            <div class="text-xs font-medium text-stone-400 uppercase tracking-wide">
              Context Tokens
            </div>
            <div class="mt-1 font-mono text-sm font-medium text-stone-700">
              {{ tokenOverrideLabel(config!.capabilities?.max_context_tokens) }}
            </div>
          </div>
          <div class="rounded-lg border border-stone-100 bg-white px-2.5 py-2">
            <div class="text-xs font-medium text-stone-400 uppercase tracking-wide">
              Output Tokens
            </div>
            <div class="mt-1 font-mono text-sm font-medium text-stone-700">
              {{ tokenOverrideLabel(config!.capabilities?.max_output_tokens) }}
            </div>
          </div>
        </div>
      </section>

      <div class="flex gap-2">
        <Button variant="secondary" block @click="startEdit"> 编辑配置 </Button>
        <Button variant="teal" block :loading="regenerating" @click="onRegenerate">
          {{ regenerating ? '刷新中…' : '刷新密钥' }}
        </Button>
        <Button variant="danger" block :loading="deleting" @click="onDelete">
          {{ deleting ? '删除中…' : '删除' }}
        </Button>
      </div>

      <Alert v-if="actionMsg" :type="actionMsg.type === 'success' ? 'success' : 'error'">
        {{ actionMsg.text }}
      </Alert>
    </div>

    <!-- Edit form -->
    <div v-else class="px-6 py-5 space-y-4">
      <div class="grid grid-cols-2 gap-4">
        <Select v-model="editForm.provider_type" label="提供商" @change="onEditProviderChange" />
        <Input v-model="editForm.name" label="名称" />
      </div>

      <Input v-model="editForm.base_url" label="上游 Base URL" mono />

      <Input
        v-model="editForm.api_key"
        label="上游 API Key"
        type="password"
        placeholder="留空则保持原密钥不变"
        mono
      />

      <CapabilityOverridesForm v-model="editCapabilityForm" />

      <Alert v-if="editError" type="error">
        {{ editError }}
      </Alert>

      <div class="flex gap-2">
        <Button variant="secondary" block @click="cancelEdit"> 取消 </Button>
        <Button variant="primary" block :loading="saving" @click="onSaveEdit">
          {{ saving ? '保存中…' : '保存修改' }}
        </Button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import type { ModelCatalogCapabilities } from '../api';
import { useManageKey } from '../composables/useManageKey';
import { useModal } from '../composables/useModal';
import { Button, Input, Select, Tag, Alert, Field, Badge } from './base';
import { getStoredTokens, providerDotClass, type StoredToken } from '../utils/providers';
import { BOOLEAN_CAPABILITIES, formatTokenLimit } from '../utils/modelCapabilities';
import CapabilityOverridesForm from './CapabilityOverridesForm.vue';

const props = defineProps<{
  confirmModal: ReturnType<typeof useModal>;
}>();
const { confirmModal } = props;

const {
  lookupToken,
  looking,
  lookupError,
  config,
  editing,
  editForm,
  editCapabilityForm,
  editError,
  saving,
  regenerating,
  deleting,
  actionMsg,
  lookup,
  startEdit,
  cancelEdit,
  onEditProviderChange,
  saveEdit,
  regenerate,
  remove,
} = useManageKey();

const storedTokens = ref<StoredToken[]>(getStoredTokens());
const booleanCapabilityOverrides = computed(() =>
  BOOLEAN_CAPABILITIES.map((capability) => ({
    ...capability,
    value: config.value?.capabilities?.[capability.key],
  })),
);
const hasCapabilityOverrides = computed(() => {
  const capabilities = config.value?.capabilities;

  if (!capabilities) {
    return false;
  }

  return (
    BOOLEAN_CAPABILITIES.some((capability) => typeof capabilities[capability.key] === 'boolean') ||
    hasTokenOverride(capabilities.max_context_tokens) ||
    hasTokenOverride(capabilities.max_output_tokens)
  );
});

function onDelete() {
  confirmModal.show({
    title: '删除配置',
    message: `确定要删除「${config.value?.name}」的配置吗？此操作不可撤销。`,
    onConfirm: async () => {
      await remove();
      storedTokens.value = getStoredTokens();
      confirmModal.hide();
    },
  });
}

async function onSaveEdit() {
  await saveEdit();
  storedTokens.value = getStoredTokens();
}

function onRegenerate() {
  confirmModal.show({
    title: '刷新密钥',
    message: '刷新后，使用旧密钥的连接将立即失效。确定要刷新吗？',
    onConfirm: async () => {
      await regenerate();
      storedTokens.value = getStoredTokens();
      confirmModal.hide();
    },
  });
}

function selectStored(item: StoredToken) {
  lookupToken.value = item.token;
  lookup();
}

function booleanOverrideLabel(value: boolean | undefined) {
  if (value === true) {
    return '启用覆盖';
  }

  if (value === false) {
    return '禁用覆盖';
  }

  return '未覆盖';
}

function booleanOverrideClass(value: boolean | undefined) {
  if (value === true) {
    return 'bg-[#f2f8f5] text-[#256047]';
  }

  if (value === false) {
    return 'bg-red-50 text-red-600';
  }

  return 'bg-stone-100 text-stone-400';
}

function tokenOverrideLabel(value: ModelCatalogCapabilities['max_context_tokens']) {
  return hasTokenOverride(value) ? formatTokenLimit(value) : '未覆盖';
}

function hasTokenOverride(value: ModelCatalogCapabilities['max_context_tokens']) {
  return typeof value === 'number' && Number.isFinite(value) && value > 0;
}
</script>
