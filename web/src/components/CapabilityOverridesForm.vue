<template>
  <section class="rounded-xl border border-stone-100 bg-stone-50/60 px-3 py-3 space-y-3">
    <div class="flex items-center justify-between gap-3">
      <div>
        <h3 class="text-sm font-semibold text-stone-700">能力覆盖</h3>
        <p class="mt-0.5 text-xs text-stone-400">留空保持 Provider 默认能力</p>
      </div>
    </div>

    <div class="grid gap-2 sm:grid-cols-2">
      <label
        v-for="capability in BOOLEAN_CAPABILITIES"
        :key="capability.key"
        class="flex items-center justify-between gap-2 rounded-lg border border-stone-100 bg-white px-2.5 py-2"
      >
        <span class="text-xs font-medium text-stone-600">{{ capability.label }}</span>
        <select
          v-model="model.boolean[capability.key]"
          class="rounded-md border border-stone-200 bg-white px-2 py-1 text-xs text-stone-700 focus:border-[#1a5c5c] focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/20"
        >
          <option value="default">默认</option>
          <option value="enabled">开启</option>
          <option value="disabled">关闭</option>
        </select>
      </label>
    </div>

    <div class="grid gap-3 sm:grid-cols-2">
      <div>
        <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide">
          Context Tokens
        </label>
        <input
          v-model="model.max_context_tokens"
          type="number"
          min="1"
          step="1"
          placeholder="默认"
          class="w-full rounded-lg border border-stone-200 bg-white px-3 py-2 text-sm font-mono text-stone-800 focus:border-[#1a5c5c] focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25"
        />
      </div>
      <div>
        <label class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide">
          Output Tokens
        </label>
        <input
          v-model="model.max_output_tokens"
          type="number"
          min="1"
          step="1"
          placeholder="默认"
          class="w-full rounded-lg border border-stone-200 bg-white px-3 py-2 text-sm font-mono text-stone-800 focus:border-[#1a5c5c] focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25"
        />
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { CapabilityOverrideForm } from '../utils/capabilityOverrides';
import { BOOLEAN_CAPABILITIES } from '../utils/modelCapabilities';

const model = defineModel<CapabilityOverrideForm>({ required: true });
</script>
