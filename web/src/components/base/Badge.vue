<template>
  <span
    class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium"
    :class="badgeClass"
  >
    <slot>{{ label }}</slot>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { PROVIDER_BADGE_CLASSES, PROVIDER_LABELS } from '../../utils/providers';

const props = withDefaults(
  defineProps<{
    providerType?: string;
    variant?: string;
    size?: 'sm' | 'md';
  }>(),
  {
    size: 'md',
  },
);

const badgeClass = computed(() => {
  if (props.providerType) {
    return PROVIDER_BADGE_CLASSES[props.providerType] ?? 'bg-stone-100 text-stone-600';
  }
  if (props.variant) {
    return props.variant;
  }
  return 'bg-stone-100 text-stone-600';
});

const label = computed(() => {
  if (props.providerType) {
    return PROVIDER_LABELS[props.providerType] ?? props.providerType;
  }
  return '';
});
</script>
