<template>
  <Teleport to="body">
    <div v-if="open" class="drawer-root">
      <button class="drawer-backdrop" aria-label="关闭" @click="$emit('close')"></button>
      <section class="drawer-panel" :class="sizeClass" :aria-label="label">
        <div class="drawer-header flex items-start justify-between gap-4 border-b px-5 py-4">
          <div>
            <h3 class="font-semibold text-stone-900">{{ title }}</h3>
            <p v-if="description" class="mt-1 text-xs text-stone-500">{{ description }}</p>
          </div>
          <button
            type="button"
            class="rounded-lg border border-stone-200 px-3 py-1.5 text-sm text-stone-500"
            @click="$emit('close')"
          >
            关闭
          </button>
        </div>

        <div class="space-y-5 overflow-y-auto p-5">
          <slot />
        </div>

        <div class="drawer-footer flex items-center justify-between gap-3 border-t px-5 py-4">
          <slot name="footer" />
        </div>
      </section>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    description?: string;
    label: string;
    size?: 'md' | 'lg';
  }>(),
  {
    description: '',
    size: 'md',
  },
);

defineEmits<{ close: [] }>();

const sizeClass = computed(() => (props.size === 'lg' ? 'drawer-panel--lg' : 'drawer-panel--md'));
</script>
