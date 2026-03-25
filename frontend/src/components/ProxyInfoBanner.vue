<template>
  <div class="bg-white border border-stone-200 rounded-xl px-4 py-3 text-sm text-stone-600 flex items-start gap-3 shadow-sm">
    <span class="mt-0.5 text-[#1a5c5c] shrink-0">
      <svg class="w-4 h-4" fill="none" stroke="currentColor" stroke-width="1.5" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" d="M13.19 8.688a4.5 4.5 0 011.242 7.244l-4.5 4.5a4.5 4.5 0 01-6.364-6.364l1.757-1.757m13.35-.622l1.757-1.757a4.5 4.5 0 00-6.364-6.364l-4.5 4.5a4.5 4.5 0 001.242 7.244" />
      </svg>
    </span>
    <div>
      <span class="font-medium text-stone-700">代理地址：</span>
      <code
        class="bg-stone-50 border border-stone-200 px-1.5 py-0.5 rounded font-mono text-xs text-stone-700 mx-1 cursor-pointer hover:bg-stone-100 transition-colors"
        :title="copied ? '已复制' : '点击复制'"
        @click="copy"
      >{{ proxyUrl }}</code>
      <span v-if="copied" class="text-[#1a5c5c] text-xs ml-1">已复制</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { copyToClipboard } from '../utils/providers';

const proxyUrl = `${window.location.origin}/v1`;
const copied = ref(false);

async function copy() {
  await copyToClipboard(proxyUrl);
  copied.value = true;
  setTimeout(() => { copied.value = false; }, 2000);
}
</script>
