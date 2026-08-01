import { readFileSync } from 'node:fs';
import { expect, test } from 'bun:test';

const source = readFileSync(new URL('../src/views/ProvidersView.vue', import.meta.url), 'utf8');
const apiSource = readFileSync(new URL('../src/api/index.ts', import.meta.url), 'utf8');
const packageSource = readFileSync(new URL('../package.json', import.meta.url), 'utf8');
const styleSource = readFileSync(new URL('../src/style.css', import.meta.url), 'utf8');
const iconButtonSource = readFileSync(
  new URL('../src/components/base/IconButton.vue', import.meta.url),
  'utf8',
);

test('new provider template changes clear provider-specific draft fields', () => {
  const selectTemplateIndex = source.indexOf('function selectProviderTemplate');
  const resetDraftIndex = source.indexOf('resetProviderDraftFields()', selectTemplateIndex);

  expect(resetDraftIndex).toBeGreaterThan(selectTemplateIndex);
});

test('provider page does not expose category filter controls above the table', () => {
  expect(source).not.toContain('v-for="filter in filters"');
  expect(source).not.toContain('activeFilter');
});

test('provider page does not expose a manual bulk status refresh action', () => {
  expect(source).not.toContain('批量测试');
  expect(source).not.toContain('@click="pingAll"');
  expect(source).not.toContain('function pingAll');
});

test('provider title actions align with the bottom of the title description', () => {
  expect(source).toContain('<PageHeader title="供应商"');
  expect(styleSource).toContain('.page-titlebar');
  expect(styleSource).toContain('align-items: end');
  expect(styleSource).not.toContain('align-items: start');
});

test('built-in provider form does not expose an upstream protocol selector', () => {
  expect(source).not.toContain('上游协议');
  expect(source).toContain('selectedProviderProtocolOptions');
});

test('custom provider form exposes protocol checkboxes and selected protocol tags', () => {
  expect(source).toContain('支持协议');
  expect(source).toContain('type="checkbox"');
  expect(source).toContain('selectedCustomProtocolOptions');
});

test('provider and supported protocols occupy separate full-width form rows', () => {
  const providerLabelIndex = source.indexOf('v-model="form.provider_template"');
  const providerLabelOpenIndex = source.lastIndexOf('<label', providerLabelIndex);
  const providerLabelCloseIndex = source.indexOf('>', providerLabelOpenIndex);
  const providerLabelTag = source.slice(providerLabelOpenIndex, providerLabelCloseIndex);
  const supportedProtocolIndex = source.indexOf('支持协议');
  const supportedProtocolOpenIndex = source.lastIndexOf('<div', supportedProtocolIndex);
  const supportedProtocolCloseIndex = source.indexOf('>', supportedProtocolOpenIndex);
  const supportedProtocolTag = source.slice(
    supportedProtocolOpenIndex,
    supportedProtocolCloseIndex,
  );

  expect(providerLabelTag).toContain('sm:col-span-2');
  expect(supportedProtocolTag).toContain('sm:col-span-2');
});

test('provider table keeps name and protocol adaptive while other columns are fixed', () => {
  const tableIndex = source.indexOf('<DataTableShell');
  const theadIndex = source.indexOf('<thead', tableIndex);
  const tableStart = source.slice(tableIndex, theadIndex);

  expect(tableStart).toContain('<DataTableShell table-class="min-w-[60rem]">');
  expect(tableStart).toContain('<colgroup>');
  expect(tableStart).toContain('<!-- 名称列和协议列自适应 -->');
  expect(tableStart.match(/<col \/>/g)?.length).toBe(2);
  expect(tableStart).toContain('<col class="w-24" />');
  expect(tableStart).not.toContain('<col class="w-72" />');
  expect(tableStart).toContain('<col class="w-20" />');
  expect(tableStart).toContain('<col class="w-32" />');
  expect(tableStart.match(/<col class="w-24" \/>/g)?.length).toBe(2);
  expect(styleSource).toContain('.data-table');
  expect(styleSource).toContain('table-layout: auto');
});

test('provider protocol column stays on a single line', () => {
  const protocolIndex = source.indexOf('upstreamProtocolOptionsForProvider');
  const protocolCellOpenIndex = source.lastIndexOf('<td', protocolIndex);
  const protocolCellCloseIndex = source.indexOf('</td>', protocolIndex);
  const protocolCell = source.slice(protocolCellOpenIndex, protocolCellCloseIndex);

  expect(protocolCell).toContain('whitespace-nowrap');
  expect(protocolCell).toContain('flex flex-nowrap');
  expect(protocolCell).not.toContain('flex flex-wrap');
});

test('provider row actions use icon buttons with enough fixed width', () => {
  const rowClickIndex = source.indexOf('@click="selectProvider(provider.id)"');
  const rowIndex = source.lastIndexOf('<tr', rowClickIndex);
  const nextRowIndex = source.indexOf('</tr>', rowIndex);
  const rowSection = source.slice(rowIndex, nextRowIndex);

  expect(packageSource).toContain('"lucide-vue-next"');
  expect(source).toContain("from 'lucide-vue-next'");
  expect(source).toContain('Pencil');
  expect(source).toContain('Trash2');
  expect(rowSection).toContain('<IconButton label="编辑供应商"');
  expect(rowSection).toContain('<IconButton');
  expect(rowSection).toContain('label="删除供应商"');
  expect(iconButtonSource).toContain(':aria-label="label"');
  expect(rowSection).toContain('<Pencil');
  expect(rowSection).toContain('<Trash2');
  expect(styleSource).toContain('.icon-button');
  expect(styleSource).toContain('height: 2rem');
  expect(styleSource).toContain('width: 2rem');
  expect(rowSection).not.toContain('>编辑</button>');
  expect(rowSection).not.toContain('>删除</button>');
});

test('provider credentials put api key before default base url', () => {
  const apiKeyIndex = source.indexOf('API Key');
  const defaultBaseUrlIndex = source.indexOf('默认 Base URL');
  const supportedProtocolIndex = source.indexOf('支持协议');

  expect(apiKeyIndex).toBeGreaterThan(-1);
  expect(defaultBaseUrlIndex).toBeGreaterThan(apiKeyIndex);
  expect(supportedProtocolIndex).toBeGreaterThan(defaultBaseUrlIndex);
});

test('supported protocol rows expose optional per-protocol base url and test action', () => {
  expect(source).toContain('protocolBaseUrlRows');
  expect(source).toContain('v-model="form.protocol_base_urls[protocol.protocol]"');
  expect(source).toContain('留空使用默认 Base URL');
  expect(source).toContain('testProviderProtocol(protocol.protocol)');
  expect(source).toContain('aria-label="测试协议连接"');
  expect(source).not.toContain('max-h-40 gap-2 overflow-y-auto');
});

test('provider protocol test action uses lucide icons instead of css-drawn icons', () => {
  const buttonIndex = source.indexOf('aria-label="测试协议连接"');
  const buttonOpenIndex = source.lastIndexOf('<button', buttonIndex);
  const buttonCloseIndex = source.indexOf('</button>', buttonIndex);
  const buttonSection = source.slice(buttonOpenIndex, buttonCloseIndex);

  expect(packageSource).toContain('"lucide-vue-next"');
  expect(source).toContain(
    "import { Gauge, LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-vue-next'",
  );
  expect(buttonSection).toContain('<LoaderCircle');
  expect(buttonSection).toContain('<Gauge');
  expect(buttonSection).not.toContain('before:absolute');
  expect(buttonSection).not.toContain('after:absolute');
});

test('protocol base url rows use fixed-width text labels instead of tags', () => {
  const rowIndex = source.indexOf('v-for="protocol in protocolBaseUrlRows"');
  const nextSectionIndex = source.indexOf('</section>', rowIndex);
  const rowSection = source.slice(rowIndex, nextSectionIndex);

  expect(rowSection).toContain('sm:grid-cols-[9.5rem_minmax(0,1fr)_auto]');
  expect(rowSection).toContain('text-sm font-medium text-stone-600');
  expect(rowSection).not.toContain('protocolTagClass(protocol.protocol)');
  expect(rowSection).not.toContain('rounded-full border px-2 py-0.5');
});

test('built-in provider protocol base urls are prefilled from provider templates', () => {
  expect(source).toContain('urls[variant.protocol] = variant.baseUrl');
  expect(source).not.toContain('variant.baseUrl !== providerTemplate.baseUrl');
});

test('provider capabilities persist protocol-specific base url overrides', () => {
  expect(source).toContain('protocol_base_urls: protocolBaseUrlsFromForm()');
  expect(apiSource).toContain('export interface ProviderProtocolBaseUrls');
  expect(apiSource).toContain('protocol_base_urls?: ProviderProtocolBaseUrls');
});

test('frontend api declares provider protocol latency test contract', () => {
  expect(apiSource).toContain('export interface TestProviderProtocolRequest');
  expect(apiSource).toContain('export interface TestProviderProtocolResponse');
  expect(apiSource).toContain(
    "api.post<TestProviderProtocolResponse>('/configs/test-protocol', data)",
  );
  expect(apiSource).toContain(
    'api.post<TestProviderProtocolResponse>(`/configs/${providerId}/test-protocol`, data)',
  );
});

test('protocol test failure displays backend error before latency', () => {
  expect(source).toContain('if (!response.data.ok)');
  expect(source).toContain('response.data.error ??');
});

test('provider status ping uses admin provider ping instead of model generation', () => {
  const pingIndex = source.indexOf('async function pingProvider');
  const nextFunctionIndex = source.indexOf('\nfunction ', pingIndex + 1);
  const pingBody = source.slice(pingIndex, nextFunctionIndex);

  expect(apiSource).toContain('export interface PingProviderResponse');
  expect(apiSource).toContain('api.post<PingProviderResponse>(`/configs/${providerId}/ping`)');
  expect(pingBody).toContain('configApi.ping(provider.id)');
  expect(pingBody).not.toContain('modelsApi.list(provider.token)');
  expect(pingBody).not.toContain('configApi.testSavedProtocol');
  expect(pingBody).not.toContain('provider.models[0]?.model_name');
  expect(pingBody).not.toContain('首字');
});

test('provider status displays only status words instead of errors or latency', () => {
  const pingIndex = source.indexOf('async function pingProvider');
  const nextFunctionIndex = source.indexOf('\nfunction ', pingIndex + 1);
  const pingBody = source.slice(pingIndex, nextFunctionIndex);

  expect(pingBody).toContain("text: '检查中'");
  expect(pingBody).toContain("text: '可用'");
  expect(pingBody).toContain("text: '不可用'");
  expect(pingBody).not.toContain('response.data.error');
  expect(pingBody).not.toContain('latency_ms');
  expect(pingBody).not.toContain("type: 'error'");
  expect(source).toContain("?? '未检查'");
});

test('provider status refresh only pings providers that need a fresh check', () => {
  const loadIndex = source.indexOf('async function loadData');
  const nextFunctionIndex = source.indexOf('\nfunction ', loadIndex + 1);
  const loadBody = source.slice(loadIndex, nextFunctionIndex);

  expect(loadBody).toContain('const loaded = await load()');
  expect(loadBody).toContain('if (loaded)');
  expect(loadBody).toContain('providersNeedingPing');
  expect(loadBody).not.toContain('pingProviders(providers.value)');
  expect(source).toContain('function pingProviders(providersToPing: ProviderConfig[])');
});

test('provider list exposes distinct loading error empty and ready states', () => {
  expect(source).toContain('useManagementList<ProviderConfig>');
  expect(source).toContain('items: providers,');
  expect(source).toContain('loading,');
  expect(source).toContain('loadError,');
  expect(source).toContain('load,');
  expect(source).toContain('role="alert"');
  expect(source).toContain('@click="loadData"');
  expect(source).toContain('<RefreshCw');
  expect(source).toContain('v-else-if="!loadError && filteredProviders.length === 0"');
  expect(source).not.toContain('v-if="!loading && filteredProviders.length === 0"');
});

test('provider initialization clears only retired tokens before loading data', () => {
  const mountedIndex = source.indexOf('onMounted(() => {');
  const mountedEnd = source.indexOf('});', mountedIndex);
  const mountedBody = source.slice(mountedIndex, mountedEnd);

  expect(mountedBody).toContain('clearLegacyProviderTokens()');
  expect(mountedBody).toContain('void loadData()');
  expect(mountedBody.indexOf('clearLegacyProviderTokens()')).toBeLessThan(
    mountedBody.indexOf('void loadData()'),
  );
});

test('expanded provider models render as compact tags without enabled status', () => {
  const expandedIndex = source.indexOf('expandedProviderId === provider.id');
  const nextTemplateIndex = source.indexOf('</template>', expandedIndex);
  const expandedSection = source.slice(expandedIndex, nextTemplateIndex);

  expect(expandedSection).toContain('flex flex-wrap gap-2');
  expect(expandedSection).toContain('v-for="model in provider.models"');
  expect(expandedSection).toContain('rounded-full border border-stone-200 bg-white');
  expect(expandedSection).not.toContain('已启用');
  expect(expandedSection).not.toContain('上游模型');
});

test('provider model details expand from selecting the table row', () => {
  const rowClickIndex = source.indexOf('@click="selectProvider(provider.id)"');
  const rowIndex = source.lastIndexOf('<tr', rowClickIndex);
  const nextRowIndex = source.indexOf('</tr>', rowIndex);
  const rowSection = source.slice(rowIndex, nextRowIndex);

  expect(rowIndex).toBeGreaterThan(-1);
  expect(rowSection).toContain('selectable-row');
  expect(rowSection).toContain('@click="selectProvider(provider.id)"');
  expect(rowSection).toContain('@keydown.enter.prevent="selectProvider(provider.id)"');
  expect(rowSection).toContain('@keydown.space.prevent="selectProvider(provider.id)"');
  expect(rowSection).toContain('@click.stop="pingProvider(provider)"');
  expect(rowSection).toContain('@click.stop="openEditDrawer(provider)"');
  expect(rowSection).toContain('@click.stop="deleteProvider(provider)"');
});

test('selected provider row has a clear selected visual state', () => {
  const rowClickIndex = source.indexOf('@click="selectProvider(provider.id)"');
  const rowIndex = source.lastIndexOf('<tr', rowClickIndex);
  const rowOpenEndIndex = source.indexOf('>', rowClickIndex);
  const nextRowIndex = source.indexOf('</tr>', rowIndex);
  const rowSection = source.slice(rowIndex, nextRowIndex);
  const rowOpenTag = source.slice(rowIndex, rowOpenEndIndex);

  expect(rowSection).toContain(':aria-selected="expandedProviderId === provider.id"');
  expect(rowSection).toContain(':class=');
  expect(rowSection).toContain('expandedProviderId === provider.id');
  expect(rowSection).toContain('selected-row');
  expect(styleSource).toContain('.selected-row');
  expect(styleSource).toContain('var(--pr-color-brand-soft)');
  expect(styleSource).toContain('inset 3px 0 0 var(--pr-color-brand)');
  expect(rowOpenTag).not.toContain('focus:ring');
  expect(rowOpenTag).not.toContain('focus:outline');
});

test('provider row selection is not a toggle action', () => {
  const selectIndex = source.indexOf('function selectProvider(');
  const nextFunctionIndex = source.indexOf('\nasync function ', selectIndex + 1);
  const selectBody = source.slice(selectIndex, nextFunctionIndex);

  expect(selectIndex).toBeGreaterThan(-1);
  expect(selectBody).toContain('expandedProviderId.value = providerId');
  expect(selectBody).not.toContain('expandedProviderId.value === providerId ? null : providerId');
  expect(source).not.toContain('function toggleExpanded');
});

test('custom protocol multiselect trigger has fixed height and internal overflow handling', () => {
  expect(source).toContain('h-10 w-full');
  expect(source).toContain('overflow-hidden');
  expect(source).toContain('flex-nowrap');
  expect(source).toContain('overflow-x-auto');
});

test('custom protocol menu closes when clicking outside the dropdown', () => {
  expect(source).toContain('ref="customProtocolSelectRef"');
  expect(source).toContain(
    "document.addEventListener('pointerdown', handleCustomProtocolOutsideClick)",
  );
  expect(source).toContain(
    "document.removeEventListener('pointerdown', handleCustomProtocolOutsideClick)",
  );
  expect(source).toContain('function handleCustomProtocolOutsideClick');
});

test('disabled built-in protocol tags keep protocol colors', () => {
  expect(source).toContain(':class="protocolTagClass(protocol.protocol)"');
  expect(source).toContain('protocolTagClass,');
  expect(source).not.toContain('class="shrink-0 rounded-full bg-stone-100');
});

test('custom protocol changes keep key and model draft fields intact', () => {
  const toggleIndex = source.indexOf('function toggleCustomProtocol');
  const nextFunctionIndex = source.indexOf('\nfunction ', toggleIndex + 1);
  const toggleBody = source.slice(toggleIndex, nextFunctionIndex);

  expect(toggleIndex).toBeGreaterThan(-1);
  expect(toggleBody).not.toContain("form.value.api_key = ''");
  expect(toggleBody).not.toContain('form.value.models = []');
  expect(toggleBody).not.toContain("modelDraft.value = ''");
});

test('provider status action is disabled while busy and has a dynamic accessible label', () => {
  const pingClickIndex = source.indexOf('@click.stop="pingProvider(provider)"');
  const buttonStart = source.lastIndexOf('<button', pingClickIndex);
  const buttonEnd = source.indexOf('>', pingClickIndex);
  const buttonTag = source.slice(buttonStart, buttonEnd);

  expect(buttonTag).toContain(':disabled="isProviderPingBusy(provider.id)"');
  expect(buttonTag).toContain(':aria-label="providerPingAriaLabel(provider)"');
  expect(source).toContain('function providerPingAriaLabel(provider: ProviderConfig)');
});

test('custom protocol trigger controls a named checkbox group without menu semantics', () => {
  const triggerIndex = source.indexOf('@click="customProtocolMenuOpen = !customProtocolMenuOpen"');
  const triggerStart = source.lastIndexOf('<button', triggerIndex);
  const triggerEnd = source.indexOf('>', triggerIndex);
  const triggerTag = source.slice(triggerStart, triggerEnd);

  expect(triggerTag).toContain(':aria-expanded="customProtocolMenuOpen"');
  expect(triggerTag).toContain('aria-controls="provider-protocol-menu"');
  expect(triggerTag).not.toContain('aria-haspopup="menu"');
  expect(source).toContain('id="provider-protocol-menu"');
  expect(source).toContain('role="group"');
  expect(source).toContain('aria-label="支持协议"');
  expect(source).not.toContain('role="menu"');
  expect(source).not.toContain('role="menuitem"');
});

test('drawer async operations are scoped to the session that started them', () => {
  expect(source).toContain('const drawerSessions = createDrawerSessionCoordinator()');
  expect(source).toContain('function beginDrawerSession(');

  for (const functionName of ['saveProvider', 'discoverModels', 'testProviderProtocol']) {
    const functionIndex = source.indexOf(`async function ${functionName}`);
    const nextFunctionIndex = source.indexOf('\nfunction ', functionIndex + 1);
    const functionBody = source.slice(functionIndex, nextFunctionIndex);

    expect(functionBody).toContain('captureDrawerSession');
    expect(functionBody).toContain('isCurrentDrawerSession');
  }
});

test('provider save busy state is independent from drawer session changes', () => {
  const beginSessionIndex = source.indexOf('function beginDrawerSession');
  const beginSessionEnd = source.indexOf('\nfunction ', beginSessionIndex + 1);
  const beginSessionBody = source.slice(beginSessionIndex, beginSessionEnd);
  const saveIndex = source.indexOf('async function saveProvider');
  const saveEnd = source.indexOf('\nfunction ', saveIndex + 1);
  const saveBody = source.slice(saveIndex, saveEnd);

  expect(beginSessionBody).not.toContain('saving.value = false');
  expect(saveBody).toContain('drawerSessions.beginSave()');
  expect(saveBody).toContain('drawerSessions.finishSave(');
  expect(saveBody).toContain('drawerSessions.isSaving()');
});

test('provider deletion is deduplicated by id and reports management errors', () => {
  const deleteIndex = source.indexOf('async function deleteProvider');
  const nextFunctionIndex = source.indexOf('\nfunction ', deleteIndex + 1);
  const deleteBody = source.slice(deleteIndex, nextFunctionIndex);

  expect(source).toContain(':disabled="isProviderDeleteBusy(provider.id)"');
  expect(deleteBody).toContain('isProviderDeleteBusy(provider.id)');
  expect(deleteBody).toContain('try {');
  expect(deleteBody).toContain('managementErrorMessage(error)');
  expect(source).toContain('role="alert"');
  expect(source).toContain('{{ actionError }}');
});

test('legacy provider token cleanup tolerates missing or throwing local storage', () => {
  const cleanupIndex = source.indexOf('function clearLegacyProviderTokens');
  const nextFunctionIndex = source.indexOf('\nfunction ', cleanupIndex + 1);
  const cleanupBody = source.slice(cleanupIndex, nextFunctionIndex);

  expect(cleanupBody).toContain("typeof localStorage === 'undefined'");
  expect(cleanupBody).toContain('try {');
  expect(cleanupBody).toContain('clearStoredProviderTokens(localStorage)');
  expect(cleanupBody).toContain('catch');
});
