<template>
  <div :class="span ? 'col-span-2' : ''">
    <dt class="text-xs font-medium text-stone-400 uppercase tracking-wide">{{ term }}</dt>
    <dd class="mt-1 font-medium text-stone-800 truncate" :title="value">
      <slot>{{ value || '—' }}</slot>
    </dd>
  </div>
</template>

<script setup lang="ts">
withDefaults(
  defineProps<{
    term: string;
    value?: string;
    span?: boolean;
  }>(),
  {
    value: '',
    span: false,
  },
);
</script>
