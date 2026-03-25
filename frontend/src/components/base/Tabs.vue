<template>
  <div class="flex border-b border-stone-100">
    <button
      v-for="tab in tabs"
      :key="tab.value"
      class="flex-1 py-3.5 text-sm font-medium transition-colors border-b-2 -mb-px"
      :class="modelValue === tab.value ? 'border-[#1a5c5c] text-[#1a5c5c]' : 'border-transparent text-stone-400 hover:text-stone-600'"
      @click="$emit('update:modelValue', tab.value)"
    >
      {{ tab.label }}
    </button>
  </div>
</template>

<script setup lang="ts">
withDefaults(
  defineProps<{
    modelValue: string;
    tabs: { label: string; value: string }[];
  }>(),
  {
    tabs: () => [],
  },
);

defineEmits<{ 'update:modelValue': [value: string] }>();
</script>
