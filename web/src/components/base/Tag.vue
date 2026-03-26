<template>
  <div class="relative inline-flex" @click="$emit('click')">
    <span
      class="inline-flex items-center gap-1.5 bg-stone-50 border border-stone-200 rounded-lg px-2.5 py-1.5 text-xs transition-colors"
      :class="[variantClass, { 'max-w-[200px]': truncate }]"
    >
      <span v-if="dot" class="shrink-0 w-1.5 h-1.5 rounded-full" :class="dotClass"></span>
      <span v-if="$slots.icon" class="shrink-0">
        <slot name="icon" />
      </span>
      <span v-if="label" class="truncate" :class="textClass">{{ label }}</span>
      <slot v-else />
    </span>
    <button
      v-if="closable"
      type="button"
      class="absolute top-0 right-0 -mt-1.5 -mr-1.5 w-3.5 h-3.5 rounded-full flex items-center justify-center transition-colors hover:bg-[#a83232] bg-stone-400"
      @click.stop="$emit('close')"
    >
      <svg
        class="w-2 h-2 text-white"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        viewBox="0 0 24 24"
      >
        <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
      </svg>
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    label?: string;
    closable?: boolean;
    variant?: 'default' | 'primary' | 'danger';
    dot?: boolean;
    dotClass?: string;
    truncate?: boolean;
  }>(),
  {
    label: '',
    closable: false,
    variant: 'default',
    dot: false,
    dotClass: '',
    truncate: true,
  },
);

defineEmits<{ close: []; click: [] }>();

const variantClass = computed(() => {
  switch (props.variant) {
    case 'primary':
      return 'bg-[#f0f8f8] hover:bg-[#e0f0f0] border-[#93bfbf] hover:border-[#1a5c5c]';
    case 'danger':
      return 'bg-[#fdf3f3] hover:bg-[#fce8e8] border-[#ecc8c8] hover:border-[#a83232]';
    default:
      return 'hover:bg-stone-100 hover:border-stone-300';
  }
});

const textClass = computed(() => {
  switch (props.variant) {
    case 'primary':
      return 'text-[#1a5c5c]';
    case 'danger':
      return 'text-[#a83232]';
    default:
      return 'text-stone-600';
  }
});
</script>
