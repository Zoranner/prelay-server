<script setup lang="ts">
defineProps<{ label: string; value: number | string }>();
</script>

<template>
  <div class="metric-card">
    <p>{{ label }}</p>
    <strong>{{ value }}</strong>
  </div>
</template>
