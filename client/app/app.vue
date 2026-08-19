<script setup lang="ts">
const tabs = [
  { label: "统计", path: "/stats" },
  { label: "供应商", path: "/providers" },
  { label: "接口", path: "/interfaces" },
];
</script>

<template>
  <div class="app-root">
    <header class="app-header">
      <div
        class="app-frame flex flex-col gap-4 px-6 py-4 lg:flex-row lg:items-center lg:justify-between"
      >
        <NuxtLink class="shrink-0 no-underline" to="/">
          <h1 class="text-xl font-semibold text-stone-900">Provider Relay</h1>
          <p class="mt-1 text-xs tracking-wide text-stone-500">
            大模型服务透传代理
          </p>
        </NuxtLink>
        <nav class="app-nav" aria-label="主导航">
          <NuxtLink
            v-for="tab in tabs"
            :key="tab.path"
            class="app-nav__link"
            :to="tab.path"
          >
            {{ tab.label }}
          </NuxtLink>
        </nav>
      </div>
    </header>
    <main class="min-h-0 flex-1 overflow-hidden">
      <div class="app-frame h-full min-h-0 px-6 py-8">
        <NuxtPage />
      </div>
    </main>
  </div>
</template>
