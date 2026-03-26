<template>
  <p class="text-sm border rounded-lg px-3 py-2" :class="alertClass">
    <slot />
  </p>
</template>

<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    type?: 'success' | 'error' | 'warning';
  }>(),
  {
    type: 'success',
  },
);

const alertClass = computed(() => {
  switch (props.type) {
    case 'success':
      return 'text-[#256047] bg-[#f2f8f5] border-[#9fc9b2]';
    case 'error':
      return 'text-[#a83232] bg-[#fdf3f3] border-[#ecc8c8]';
    case 'warning':
      return 'text-[#8b6230] bg-[#fef8f0] border-[#ecd8b8]';
    default:
      return '';
  }
});
</script>
