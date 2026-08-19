<script setup lang="ts">
withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    description?: string;
    label: string;
    size?: "md" | "lg";
  }>(),
  { description: "", size: "md" },
);

defineEmits<{ close: [] }>();
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="drawer-root">
      <button
        class="drawer-backdrop"
        aria-label="关闭"
        @click="$emit('close')"
      />
      <section
        class="drawer-panel"
        :class="size === 'md' ? 'drawer-panel--md' : ''"
        :aria-label="label"
      >
        <header
          class="drawer-header flex items-start justify-between gap-4 border-b px-5 py-4"
        >
          <div>
            <h2 class="font-semibold text-stone-900">{{ title }}</h2>
            <p v-if="description" class="mt-1 text-xs text-stone-500">
              {{ description }}
            </p>
          </div>
          <button class="button-secondary" @click="$emit('close')">关闭</button>
        </header>
        <div class="space-y-5 overflow-y-auto p-5">
          <slot />
        </div>
        <footer
          class="drawer-footer flex items-center justify-between gap-3 border-t px-5 py-4"
        >
          <slot name="footer" />
        </footer>
      </section>
    </div>
  </Teleport>
</template>
