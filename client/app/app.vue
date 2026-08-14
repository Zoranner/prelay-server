<template>
  <div class="min-h-screen bg-slate-950 text-slate-100">
    <header class="border-b border-slate-800 bg-slate-950/95">
      <div class="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-4 px-6 py-4">
        <NuxtLink class="text-lg font-semibold text-white" to="/">Provider Relay</NuxtLink>
        <nav class="flex flex-wrap gap-1 text-sm">
          <NuxtLink class="nav-link" to="/providers">供应商</NuxtLink>
          <NuxtLink class="nav-link" to="/interfaces">接口</NuxtLink>
          <NuxtLink class="nav-link" to="/stats">统计</NuxtLink>
          <NuxtLink class="nav-link" to="/diagnostics">诊断</NuxtLink>
        </nav>
      </div>
    </header>
    <NuxtPage />
  </div>
</template>
