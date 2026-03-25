<template>
  <div class="min-h-screen bg-[#f0ede8]">
    <AppHeader />
    <main class="max-w-lg mx-auto px-6 py-10 space-y-4">
      <ProxyInfoBanner />

      <!-- Tab card -->
      <div class="bg-white rounded-2xl border border-stone-200 shadow-sm overflow-hidden">
        <Tabs
          v-model="activeTab"
          :tabs="[
            { label: '新建密钥', value: 'create' },
            { label: '管理密钥', value: 'manage' },
          ]"
        />

        <!-- Tab content -->
        <CreateKeyCard v-if="activeTab === 'create'" />
        <ManageKeyCard v-else :confirm-modal="confirmModal" />

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
import ManageKeyCard from '../components/ManageKeyCard.vue';
import { Tabs, Modal } from '../components/base';
import { useModal } from '../composables/useModal';

const activeTab = ref<'create' | 'manage'>('create');
const confirmModal = useModal();
</script>
