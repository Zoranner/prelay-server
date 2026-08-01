<template>
  <button
    :type="type"
    class="inline-flex items-center justify-center gap-2 rounded-lg text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50"
    :class="[variantClass, sizeClass, { 'w-full': block }]"
    :disabled="disabled || loading"
  >
    <LoaderCircle v-if="loading" class="h-4 w-4 animate-spin" aria-hidden="true" />
    <slot />
  </button>
</template>

<script setup lang="ts">
import { LoaderCircle } from 'lucide-vue-next';
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    variant?: 'primary' | 'secondary' | 'danger' | 'teal' | 'ghost';
    size?: 'sm' | 'md' | 'lg';
    disabled?: boolean;
    loading?: boolean;
    block?: boolean;
    type?: 'button' | 'submit' | 'reset';
  }>(),
  {
    variant: 'primary',
    size: 'md',
    disabled: false,
    loading: false,
    block: false,
    type: 'button',
  },
);

const variantClass = computed(() => {
  switch (props.variant) {
    case 'primary':
      return 'bg-[var(--pr-color-brand)] text-white hover:bg-[#12564b]';
    case 'secondary':
      return 'border border-stone-200 bg-white text-stone-600 hover:bg-stone-50';
    case 'danger':
      return 'bg-[var(--pr-color-danger)] text-white hover:bg-red-700';
    case 'teal':
      return 'border border-[var(--pr-color-brand)] text-[var(--pr-color-brand)] hover:bg-[var(--pr-color-brand-panel)]';
    case 'ghost':
      return 'text-stone-500 hover:text-stone-700 hover:bg-stone-100';
    default:
      return '';
  }
});

const sizeClass = computed(() => {
  switch (props.size) {
    case 'sm':
      return 'px-3 py-1.5 text-xs';
    case 'lg':
      return 'px-6 py-3 text-base';
    default:
      return 'px-4 py-2 text-sm';
  }
});
</script>
