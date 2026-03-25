<template>
  <div class="min-h-screen bg-[#f0ede8]">
    <AppHeader />
    <main class="max-w-lg mx-auto px-6 py-10 space-y-4">
      <ProxyInfoBanner />

      <!-- Tab card -->
      <div class="bg-white rounded-2xl border border-stone-200 shadow-sm overflow-hidden">
        <!-- Tab switcher -->
        <div class="flex border-b border-stone-100">
          <button
            class="flex-1 py-3.5 text-sm font-medium transition-colors border-b-2 -mb-px"
            :class="
              activeTab === 'create'
                ? 'border-[#1a5c5c] text-[#1a5c5c]'
                : 'border-transparent text-stone-400 hover:text-stone-600'
            "
            @click="activeTab = 'create'"
          >
            新建密钥
          </button>
          <button
            class="flex-1 py-3.5 text-sm font-medium transition-colors border-b-2 -mb-px"
            :class="
              activeTab === 'manage'
                ? 'border-[#1a5c5c] text-[#1a5c5c]'
                : 'border-transparent text-stone-400 hover:text-stone-600'
            "
            @click="activeTab = 'manage'"
          >
            管理密钥
          </button>
        </div>

        <!-- Tab content -->
        <CreateKeyCard v-if="activeTab === 'create'" />
        <ManageKeyCard v-else />
      </div>
    </main>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import AppHeader from '../components/AppHeader.vue';
import ProxyInfoBanner from '../components/ProxyInfoBanner.vue';
import CreateKeyCard from '../components/CreateKeyCard.vue';
import ManageKeyCard from '../components/ManageKeyCard.vue';

const activeTab = ref<'create' | 'manage'>('create');
</script>
