<template>
  <IconButton v-bind="$attrs" :label="label" :size="size">
    <Copy :class="size === 'sm' ? 'h-3.5 w-3.5' : 'h-4 w-4'" aria-hidden="true" />
  </IconButton>
</template>

<script setup lang="ts">
import { Copy } from 'lucide-vue-next';
import IconButton from './IconButton.vue';

withDefaults(
  defineProps<{
    label: string;
    size?: 'sm' | 'md';
  }>(),
  {
    size: 'md',
  },
);
</script>
