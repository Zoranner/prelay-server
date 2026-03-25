<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
      @click.self="onCancel"
    >
      <div class="bg-white rounded-xl shadow-2xl w-80 max-w-[90vw] overflow-hidden">
        <div class="px-5 py-4 border-b border-stone-100">
          <h3 class="text-base font-semibold text-stone-800">{{ title }}</h3>
        </div>
        <div class="px-5 py-4">
          <p class="text-sm text-stone-600">{{ message }}</p>
        </div>
        <div class="px-5 py-3 bg-stone-50 flex justify-end gap-2">
          <Button variant="secondary" @click="onCancel">取消</Button>
          <Button variant="danger" :loading="loading" @click="onConfirm">确认</Button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { Button } from './index';

defineProps<{
  open: boolean;
  title?: string;
  message?: string;
  loading?: boolean;
}>();

const emit = defineEmits<{
  confirm: [];
  cancel: [];
}>();

function onConfirm() {
  emit('confirm');
}

function onCancel() {
  emit('cancel');
}
</script>
