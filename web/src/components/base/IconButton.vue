<template>
  <button
    type="button"
    class="icon-button"
    :class="[
      size === 'sm' ? 'icon-button--sm' : '',
      variant === 'danger' ? 'icon-button--danger' : '',
    ]"
    :aria-label="label"
    :title="label"
    :disabled="disabled"
  >
    <slot />
  </button>
</template>

<script setup lang="ts">
withDefaults(
  defineProps<{
    label: string;
    variant?: 'default' | 'danger';
    size?: 'sm' | 'md';
    disabled?: boolean;
  }>(),
  {
    variant: 'default',
    size: 'md',
    disabled: false,
  },
);
</script>
