<script setup lang="ts">
withDefaults(
  defineProps<{
    label: string;
    variant?: "default" | "danger";
    disabled?: boolean;
  }>(),
  { variant: "default", disabled: false },
);
</script>

<template>
  <button
    class="icon-button"
    :class="variant === 'danger' ? 'icon-button--danger' : ''"
    :aria-label="label"
    :title="label"
    :disabled="disabled"
  >
    <slot />
  </button>
</template>
