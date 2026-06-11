<template>
  <div class="px-6 py-5 space-y-4">
    <div class="grid grid-cols-2 gap-4">
      <Select v-model="form.provider_type" label="提供商" @change="onProviderChange" />
      <Input v-model="form.name" label="名称" optional placeholder="例如：公司 OpenAI Key" />
    </div>

    <Input v-model="form.base_url" label="上游 Base URL" placeholder="填写上游 Base URL" mono />

    <Input
      v-model="form.api_key"
      label="上游 API Key"
      type="password"
      placeholder="填写上游 API Key"
      mono
    />

    <CapabilityOverridesForm v-model="capabilityForm" />

    <Alert v-if="error" type="error">
      {{ error }}
    </Alert>

    <Button block :loading="creating" @click="submit">
      {{ creating ? '创建中…' : '生成代理密钥' }}
    </Button>
  </div>

  <!-- Created token result -->
  <div v-if="createdToken" class="px-6 pb-5">
    <div class="bg-[#f2f8f5] border border-[#9fc9b2] rounded-xl p-4">
      <div class="flex items-center gap-2 text-[#256047] font-medium text-sm mb-2">
        <svg class="w-4 h-4 shrink-0" fill="currentColor" viewBox="0 0 20 20">
          <path
            fill-rule="evenodd"
            d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z"
            clip-rule="evenodd"
          />
        </svg>
        密钥创建成功
      </div>
      <p class="text-xs text-[#3d7060] mb-3">请妥善保存此密钥，之后可通过它查询和管理配置：</p>
      <div class="flex items-center gap-2">
        <code
          class="flex-1 flex items-center bg-white border border-[#9fc9b2] rounded-lg px-3 text-xs font-mono text-stone-700 break-all select-all self-stretch"
          >{{ createdToken }}</code
        >
        <Button variant="primary" @click="copyToken">
          {{ copied ? '已复制' : '复制' }}
        </Button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useCreateKey } from '../composables/useCreateKey';
import { Button, Input, Select, Alert } from './base';
import CapabilityOverridesForm from './CapabilityOverridesForm.vue';

const {
  form,
  capabilityForm,
  creating,
  error,
  createdToken,
  copied,
  onProviderChange,
  submit,
  copyToken,
} = useCreateKey();
</script>
