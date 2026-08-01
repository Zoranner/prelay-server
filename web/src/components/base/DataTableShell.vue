<template>
  <div class="table-scroll">
    <table class="data-table" :class="tableClass">
      <slot />
    </table>
  </div>
</template>

<script setup lang="ts">
withDefaults(
  defineProps<{
    tableClass?: string;
  }>(),
  {
    tableClass: '',
  },
);
</script>
