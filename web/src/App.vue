<template>
  <div>
    <div class="sticky top-0 z-20 bg-white/95 border-b border-stone-200 shadow-sm">
      <Tabs
        v-model="activeView"
        class="max-w-lg mx-auto"
        :tabs="[
          { label: '配置', value: 'providers' },
          { label: '统计', value: 'stats' },
        ]"
      />
    </div>

    <ProvidersView v-if="activeView === 'providers'" />
    <StatsView v-else />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { Tabs } from './components/base';
import ProvidersView from './views/ProvidersView.vue';
import StatsView from './views/StatsView.vue';

const activeView = ref<'providers' | 'stats'>('providers');
</script>
