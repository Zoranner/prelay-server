<template>
  <div class="page-titlebar">
    <div>
      <h2 class="text-[22px] font-semibold text-stone-900">{{ title }}</h2>
      <p v-if="description" class="mt-1 text-sm text-stone-500">{{ description }}</p>
    </div>
    <div v-if="$slots.actions" class="page-titlebar__actions">
      <slot name="actions" />
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  title: string;
  description?: string;
}>();
</script>
