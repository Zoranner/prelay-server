<template>
  <div class="min-h-screen bg-[#f0ede8]">
    <AppHeader />
    <main
      class="mx-auto px-6 py-10 space-y-4"
      :class="activeTab === 'catalog' ? 'max-w-6xl' : 'max-w-lg'"
    >
      <ProxyInfoBanner />

      <!-- Tab card -->
      <div class="bg-white rounded-2xl border border-stone-200 shadow-sm overflow-hidden">
        <Tabs
          v-model="activeTab"
          :tabs="[
            { label: '新建密钥', value: 'create' },
            { label: '管理密钥', value: 'manage' },
            { label: '模型别名', value: 'alias' },
            { label: '模型目录', value: 'catalog' },
          ]"
        />

        <!-- Tab content -->
        <CreateKeyCard v-if="activeTab === 'create'" />
        <ManageKeyCard v-else-if="activeTab === 'manage'" :confirm-modal="confirmModal" />
        <CreateAliasCard v-else-if="activeTab === 'alias'" />
        <ModelCatalogCard v-else />

        <Modal
          :open="confirmModal.open.value"
          :title="confirmModal.title.value"
          :message="confirmModal.message.value"
          @confirm="confirmModal.onConfirm.value?.()"
          @cancel="confirmModal.hide()"
        />
      </div>
    </main>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import AppHeader from '../components/AppHeader.vue';
import ProxyInfoBanner from '../components/ProxyInfoBanner.vue';
import CreateKeyCard from '../components/CreateKeyCard.vue';
import CreateAliasCard from '../components/CreateAliasCard.vue';
import ManageKeyCard from '../components/ManageKeyCard.vue';
import ModelCatalogCard from '../components/ModelCatalogCard.vue';
import { Tabs, Modal } from '../components/base';
import { useModal } from '../composables/useModal';

const activeTab = ref<'create' | 'manage' | 'alias' | 'catalog'>('create');
const confirmModal = useModal();
</script>
