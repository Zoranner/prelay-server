<template>
  <!-- Lookup input -->
  <div class="px-6 py-5">
    <div class="flex gap-2">
      <input
        v-model="lookupToken"
        type="text"
        placeholder="输入你的代理密钥…"
        class="flex-1 border border-stone-200 rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c] transition-colors"
        @keydown.enter="lookup"
      />
      <button
        :disabled="looking"
        class="bg-[#1a5c5c] hover:bg-[#134848] disabled:opacity-50 text-white font-medium rounded-lg px-4 py-2 text-sm transition-colors whitespace-nowrap"
        @click="lookup"
      >
        {{ looking ? '查询中…' : '查询' }}
      </button>
    </div>
    <p v-if="lookupError" class="mt-3 text-sm text-[#a83232] bg-[#fdf3f3] border border-[#ecc8c8] rounded-lg px-3 py-2">
      {{ lookupError }}
    </p>
  </div>

  <!-- Found config -->
  <div v-if="config" class="border-t border-stone-100">
    <!-- Detail view -->
    <div v-if="!editing" class="px-6 py-5 space-y-5">
      <dl class="grid grid-cols-2 gap-x-6 gap-y-4 text-sm">
        <div>
          <dt class="text-xs font-medium text-stone-400 uppercase tracking-wide">名称</dt>
          <dd class="font-medium text-stone-800 mt-1">{{ config.name || '（未命名）' }}</dd>
        </div>
        <div>
          <dt class="text-xs font-medium text-stone-400 uppercase tracking-wide">提供商</dt>
          <dd class="mt-1">
            <span :class="providerBadgeClass(config.provider_type)" class="px-2 py-0.5 rounded-full text-xs font-medium">
              {{ providerLabel(config.provider_type) }}
            </span>
          </dd>
        </div>
        <div class="col-span-2">
          <dt class="text-xs font-medium text-stone-400 uppercase tracking-wide">Base URL</dt>
          <dd class="font-mono text-xs text-stone-600 mt-1 break-all">{{ config.base_url }}</dd>
        </div>
        <div class="col-span-2">
          <dt class="text-xs font-medium text-stone-400 uppercase tracking-wide">上游 API Key</dt>
          <dd class="font-mono text-xs text-stone-600 mt-1">{{ config.api_key_masked }}</dd>
        </div>
      </dl>

      <div class="flex gap-2">
        <button
          class="flex-1 border border-stone-200 hover:bg-stone-50 text-stone-600 font-medium rounded-lg px-3 py-2 text-sm transition-colors"
          @click="startEdit"
        >
          编辑配置
        </button>
        <button
          :disabled="regenerating"
          class="flex-1 border border-[#93bfbf] hover:bg-[#f0f8f8] text-[#1a5c5c] font-medium rounded-lg px-3 py-2 text-sm transition-colors disabled:opacity-50"
          @click="regenerate"
        >
          {{ regenerating ? '刷新中…' : '刷新密钥' }}
        </button>
        <button
          :disabled="deleting"
          class="flex-1 border border-[#ecc8c8] hover:bg-[#fdf3f3] text-[#a83232] font-medium rounded-lg px-3 py-2 text-sm transition-colors disabled:opacity-50"
          @click="remove"
        >
          {{ deleting ? '删除中…' : '删除' }}
        </button>
      </div>

      <p
        v-if="actionMsg"
        :class="actionMsg.type === 'success' ? 'text-[#256047] bg-[#f2f8f5] border-[#9fc9b2]' : 'text-[#a83232] bg-[#fdf3f3] border-[#ecc8c8]'"
        class="text-sm border rounded-lg px-3 py-2"
      >
        {{ actionMsg.text }}
      </p>
    </div>

    <!-- Edit form -->
    <div v-else class="px-6 py-5 space-y-4">
      <div class="grid grid-cols-2 gap-4">
        <div>
          <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide">提供商</label>
          <select
            v-model="editForm.provider_type"
            class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm bg-white focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c] transition-colors"
            @change="onEditProviderChange"
          >
            <option value="openai">OpenAI</option>
            <option value="anthropic">Anthropic Claude</option>
            <option value="azure">Azure OpenAI</option>
            <option value="custom">自定义兼容接口</option>
          </select>
        </div>
        <div>
          <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide">名称</label>
          <input
            v-model="editForm.name"
            type="text"
            class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c] transition-colors"
          />
        </div>
      </div>

      <div>
        <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide">Base URL</label>
        <input
          v-model="editForm.base_url"
          type="text"
          class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c] transition-colors"
        />
      </div>

      <div>
        <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide">
          上游 API Key <span class="text-stone-400 normal-case font-normal">（留空则不修改）</span>
        </label>
        <input
          v-model="editForm.api_key"
          type="password"
          placeholder="留空则保持原密钥不变"
          class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c] transition-colors"
        />
      </div>

      <p v-if="editError" class="text-sm text-[#a83232] bg-[#fdf3f3] border border-[#ecc8c8] rounded-lg px-3 py-2">
        {{ editError }}
      </p>

      <div class="flex gap-2">
        <button
          class="flex-1 border border-stone-200 hover:bg-stone-50 text-stone-600 font-medium rounded-lg px-3 py-2 text-sm transition-colors"
          @click="cancelEdit"
        >
          取消
        </button>
        <button
          :disabled="saving"
          class="flex-1 bg-[#1a5c5c] hover:bg-[#134848] disabled:opacity-50 text-white font-medium rounded-lg px-3 py-2 text-sm transition-colors"
          @click="saveEdit"
        >
          {{ saving ? '保存中…' : '保存修改' }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useManageKey } from '../composables/useManageKey';
import { providerLabel, providerBadgeClass } from '../utils/providers';

const {
  lookupToken, looking, lookupError, config,
  editing, editForm, editError, saving,
  regenerating, deleting, actionMsg,
  lookup, startEdit, cancelEdit, onEditProviderChange, saveEdit, regenerate, remove,
} = useManageKey();
</script>
