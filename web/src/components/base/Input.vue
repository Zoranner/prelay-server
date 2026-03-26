<template>
  <div :class="flex ? 'flex-1' : ''">
    <label
      v-if="label"
      class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide"
    >
      {{ label }}
      <span v-if="optional" class="text-stone-400 normal-case font-normal ml-1">(可选)</span>
    </label>
    <input
      v-model="model"
      :type="type"
      :placeholder="placeholder"
      :disabled="disabled"
      class="border border-stone-200 rounded-lg text-sm transition-colors bg-white focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c]"
      :class="[
        sizeClass,
        mono ? 'font-mono' : '',
        disabled ? 'bg-stone-50 text-stone-400 cursor-not-allowed' : 'text-stone-800',
        flex ? 'flex-1 w-full' : 'w-full',
      ]"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    modelValue: string;
    type?: 'text' | 'password' | 'email' | 'url';
    placeholder?: string;
    disabled?: boolean;
    size?: 'sm' | 'md' | 'lg';
    mono?: boolean;
    label?: string;
    optional?: boolean;
    flex?: boolean;
  }>(),
  {
    type: 'text',
    placeholder: '',
    disabled: false,
    size: 'md',
    mono: false,
    optional: false,
    flex: false,
  },
);

const emit = defineEmits<{ 'update:modelValue': [value: string] }>();

const model = computed({
  get: () => props.modelValue,
  set: (v) => emit('update:modelValue', v),
});

const sizeClass = computed(() => {
  switch (props.size) {
    case 'sm':
      return 'px-2.5 py-1.5';
    case 'lg':
      return 'px-4 py-3';
    default:
      return 'px-3 py-2';
  }
});
</script>
