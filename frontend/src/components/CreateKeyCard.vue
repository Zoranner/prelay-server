<template>
  <div class="px-6 py-5 space-y-4">
    <div class="grid grid-cols-2 gap-4">
      <div>
        <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide"
          >提供商</label
        >
        <ProviderSelect v-model="form.provider_type" @change="onProviderChange" />
      </div>
      <div>
        <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide">
          名称 <span class="text-stone-400 normal-case font-normal">（可选）</span>
        </label>
        <input
          v-model="form.name"
          type="text"
          placeholder="例如：公司 OpenAI Key"
          class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c] transition-colors"
        />
      </div>
    </div>

    <div>
      <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide"
        >Base URL</label
      >
      <input
        v-model="form.base_url"
        type="text"
        placeholder="https://api.openai.com"
        class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c] transition-colors"
      />
    </div>

    <div>
      <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide"
        >API Key</label
      >
      <input
        v-model="form.api_key"
        type="password"
        placeholder="填写真实的 API Key"
        class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c] transition-colors"
      />
    </div>

    <p
      v-if="error"
      class="text-sm text-[#a83232] bg-[#fdf3f3] border border-[#ecc8c8] rounded-lg px-3 py-2"
    >
      {{ error }}
    </p>

    <button
      :disabled="creating"
      class="w-full bg-[#1a5c5c] hover:bg-[#134848] disabled:opacity-50 text-white font-medium rounded-lg px-4 py-2.5 text-sm transition-colors"
      @click="submit"
    >
      {{ creating ? '创建中…' : '生成代理密钥' }}
    </button>
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
          class="flex-1 bg-white border border-[#9fc9b2] rounded-lg px-3 py-2 text-xs font-mono text-stone-700 break-all select-all"
          >{{ createdToken }}</code
        >
        <button
          class="shrink-0 bg-[#256047] hover:bg-[#1c4c38] text-white text-xs rounded-lg px-3 py-2 transition-colors"
          @click="copyToken"
        >
          {{ copied ? '已复制' : '复制' }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useCreateKey } from '../composables/useCreateKey';
import ProviderSelect from './ProviderSelect.vue';

const { form, creating, error, createdToken, copied, onProviderChange, submit, copyToken } =
  useCreateKey();
</script>
