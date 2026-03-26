<template>
  <button
    :type="type"
    class="inline-flex items-center justify-center gap-2 font-medium rounded-lg text-sm transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
    :class="[variantClass, sizeClass, { 'w-full': block }]"
    :disabled="disabled || loading"
  >
    <svg v-if="loading" class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
      <path
        class="opacity-75"
        fill="currentColor"
        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
      />
    </svg>
    <slot />
  </button>
</template>

<script setup lang="ts">
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
      return 'bg-[#1a5c5c] hover:bg-[#134848] text-white';
    case 'secondary':
      return 'border border-stone-200 hover:bg-stone-50 text-stone-600';
    case 'danger':
      return 'bg-[#a83232] hover:bg-[#8c2828] text-white';
    case 'teal':
      return 'border border-[#93bfbf] hover:bg-[#f0f8f8] text-[#1a5c5c]';
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
