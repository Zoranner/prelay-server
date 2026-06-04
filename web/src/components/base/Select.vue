<template>
  <div>
    <label
      v-if="label"
      class="block text-xs font-medium text-stone-500 mb-1.5 uppercase tracking-wide"
    >
      {{ label }}
    </label>
    <div ref="rootEl" class="relative">
      <button
        type="button"
        class="w-full border border-stone-200 rounded-lg px-3 py-2 text-sm bg-white text-left flex items-center justify-between gap-2 focus:outline-none focus:ring-2 focus:ring-[#1a5c5c]/25 focus:border-[#1a5c5c] transition-colors hover:border-stone-300"
        :class="open ? 'border-[#1a5c5c] ring-2 ring-[#1a5c5c]/25' : ''"
        @click="toggle"
      >
        <span class="flex items-center gap-2 min-w-0">
          <span class="shrink-0 w-2 h-2 rounded-full" :class="dotClass(modelValue)"></span>
          <span class="truncate text-stone-800">{{ selectedLabel }}</span>
        </span>
        <svg
          class="w-4 h-4 text-stone-400 shrink-0 transition-transform duration-150"
          :class="open ? 'rotate-180' : ''"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
        </svg>
      </button>

      <Teleport to="body">
        <Transition
          enter-active-class="transition duration-100 ease-out"
          enter-from-class="opacity-0 scale-95"
          enter-to-class="opacity-100 scale-100"
          leave-active-class="transition duration-75 ease-in"
          leave-from-class="opacity-100 scale-100"
          leave-to-class="opacity-0 scale-95"
        >
          <div
            v-if="open"
            :style="dropdownStyle"
            class="fixed z-9999 bg-white border border-stone-200 rounded-xl shadow-xl overflow-y-auto max-h-64 origin-top"
          >
            <template v-for="(group, gi) in GROUPS" :key="group.label">
              <div
                class="px-3 py-1.5 text-[10px] font-semibold uppercase tracking-widest text-stone-400 select-none sticky top-0 bg-white"
                :class="gi > 0 ? 'border-t border-stone-100' : ''"
              >
                {{ group.label }}
              </div>
              <button
                v-for="opt in group.options"
                :key="opt.value"
                type="button"
                class="w-full text-left px-3 py-2 text-sm flex items-center gap-2.5 transition-colors"
                :class="
                  modelValue === opt.value
                    ? 'bg-[#f0f8f8] text-[#1a5c5c]'
                    : 'text-stone-700 hover:bg-stone-50'
                "
                @click="select(opt.value)"
              >
                <span class="shrink-0 w-2 h-2 rounded-full" :class="dotClass(opt.value)"></span>
                <span class="flex-1 truncate">{{ opt.label }}</span>
                <svg
                  v-if="modelValue === opt.value"
                  class="w-3.5 h-3.5 text-[#1a5c5c] shrink-0"
                  fill="currentColor"
                  viewBox="0 0 20 20"
                >
                  <path
                    fill-rule="evenodd"
                    d="M16.707 5.293a1 1 0 00-1.414 0L8 12.586 4.707 9.293a1 1 0 00-1.414 1.414l4 4a1 1 0 001.414 0l8-8a1 1 0 000-1.414z"
                    clip-rule="evenodd"
                  />
                </svg>
              </button>
            </template>
          </div>
        </Transition>
      </Teleport>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { PROVIDER_LABELS, providerDotClass } from '../../utils/providers';

const props = withDefaults(
  defineProps<{
    modelValue: string;
    label?: string;
  }>(),
  {
    label: '',
  },
);
const emit = defineEmits<{ 'update:modelValue': [value: string]; change: [value: string] }>();

const open = ref(false);
const rootEl = ref<HTMLElement | null>(null);
const dropdownStyle = ref<Record<string, string>>({});

const GROUPS = [
  {
    label: '订阅服务',
    options: [
      { value: 'zhipu_coding', label: PROVIDER_LABELS['zhipu_coding'] },
      { value: 'minimax_token', label: PROVIDER_LABELS['minimax_token'] },
    ],
  },
  {
    label: '接口服务',
    options: [
      { value: 'openai', label: PROVIDER_LABELS['openai'] },
      { value: 'anthropic', label: PROVIDER_LABELS['anthropic'] },
      { value: 'zhipu', label: PROVIDER_LABELS['zhipu'] },
      { value: 'minimax', label: PROVIDER_LABELS['minimax'] },
    ],
  },
  {
    label: '其他服务',
    options: [
      { value: 'openai_compatible', label: PROVIDER_LABELS['openai_compatible'] },
      { value: 'anthropic_compatible', label: PROVIDER_LABELS['anthropic_compatible'] },
      { value: 'ollama_native', label: PROVIDER_LABELS['ollama_native'] },
    ],
  },
];

const selectedLabel = computed(() => PROVIDER_LABELS[props.modelValue] ?? props.modelValue);

function dotClass(value: string) {
  return providerDotClass(value);
}

function toggle() {
  if (!open.value && rootEl.value) {
    const rect = rootEl.value.getBoundingClientRect();
    dropdownStyle.value = {
      top: `${rect.bottom + 6}px`,
      left: `${rect.left}px`,
      width: `${rect.width}px`,
    };
  }
  open.value = !open.value;
}

function select(value: string) {
  emit('update:modelValue', value);
  emit('change', value);
  open.value = false;
}

function onOutsideClick(e: MouseEvent) {
  if (rootEl.value && !rootEl.value.contains(e.target as Node)) {
    open.value = false;
  }
}

onMounted(() => document.addEventListener('mousedown', onOutsideClick));
onUnmounted(() => document.removeEventListener('mousedown', onOutsideClick));
</script>
