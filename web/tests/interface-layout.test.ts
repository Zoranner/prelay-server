import { readFileSync } from 'node:fs';
import { expect, test } from 'bun:test';

const source = readFileSync(new URL('../src/views/InterfacesView.vue', import.meta.url), 'utf8');
const packageSource = readFileSync(new URL('../package.json', import.meta.url), 'utf8');
const styleSource = readFileSync(new URL('../src/style.css', import.meta.url), 'utf8');
const copyButtonSource = readFileSync(
  new URL('../src/components/base/CopyButton.vue', import.meta.url),
  'utf8',
);
const iconButtonSource = readFileSync(
  new URL('../src/components/base/IconButton.vue', import.meta.url),
  'utf8',
);

test('interface model add action stays with the model form instead of the drawer footer', () => {
  const addModelIndex = source.indexOf('添加模型');
  const footerIndex = source.indexOf('<template #footer>');
  const modelNameIndex = source.indexOf('对外模型名');

  expect(addModelIndex).toBeGreaterThan(modelNameIndex);
  expect(addModelIndex).toBeLessThan(footerIndex);
});

test('interface page does not expose protocol selection because every interface supports all protocols', () => {
  expect(source).not.toContain('protocolFilters');
  expect(source).not.toContain('activeProtocol');
  expect(source).not.toContain('v-model="form.protocol"');
  expect(source).not.toContain('protocolTagClass(item.protocol)');
});

test('interface table does not expose a misleading default model column', () => {
  expect(source).not.toContain('默认模型');
  expect(source).not.toContain('item.models[0]?.model_name');
});

test('interface table keeps only the name column adaptive', () => {
  const tableIndex = source.indexOf('<DataTableShell');
  const theadIndex = source.indexOf('<thead', tableIndex);
  const tableStart = source.slice(tableIndex, theadIndex);

  expect(tableStart).toContain('<DataTableShell table-class="min-w-[48rem]">');
  expect(tableStart).toContain('<colgroup>');
  expect(tableStart).toContain('<!-- 接口名称列自适应 -->');
  expect(tableStart.match(/<col \/>/g)?.length).toBe(1);
  expect(tableStart).toContain('<col class="w-48" />');
  expect(tableStart).toContain('<col class="w-20" />');
  expect(tableStart).toContain('<col class="w-24" />');
  expect(styleSource).toContain('.data-table');
  expect(styleSource).toContain('table-layout: auto');
});

test('interface rows expand from selecting the table row', () => {
  const rowClickIndex = source.indexOf('@click="selectInterface(item.id)"');
  const rowIndex = source.lastIndexOf('<tr', rowClickIndex);
  const nextRowIndex = source.indexOf('</tr>', rowIndex);
  const rowSection = source.slice(rowIndex, nextRowIndex);

  expect(rowIndex).toBeGreaterThan(-1);
  expect(rowSection).toContain('selectable-row');
  expect(rowSection).toContain('@click="selectInterface(item.id)"');
  expect(rowSection).toContain('@keydown.enter.prevent="selectInterface(item.id)"');
  expect(rowSection).toContain('@keydown.space.prevent="selectInterface(item.id)"');
  expect(rowSection).toContain('@click.stop="openEditDrawer(item)"');
  expect(rowSection).toContain('@click.stop="deleteInterface(item)"');
  expect(rowSection).not.toContain('@click="toggleExpanded(item.id)"');
});

test('selected interface row matches provider selected visual state', () => {
  const rowClickIndex = source.indexOf('@click="selectInterface(item.id)"');
  const rowIndex = source.lastIndexOf('<tr', rowClickIndex);
  const rowOpenEndIndex = source.indexOf('>', rowClickIndex);
  const nextRowIndex = source.indexOf('</tr>', rowIndex);
  const rowSection = source.slice(rowIndex, nextRowIndex);
  const rowOpenTag = source.slice(rowIndex, rowOpenEndIndex);

  expect(rowSection).toContain(':aria-selected="expandedInterfaceId === item.id"');
  expect(rowSection).toContain(':class=');
  expect(rowSection).toContain('expandedInterfaceId === item.id');
  expect(rowSection).toContain('selected-row');
  expect(styleSource).toContain('.selected-row');
  expect(styleSource).toContain('var(--pr-color-brand-soft)');
  expect(styleSource).toContain('inset 3px 0 0 var(--pr-color-brand)');
  expect(rowOpenTag).not.toContain('focus:ring');
  expect(rowOpenTag).not.toContain('focus:outline');
});

test('interface row selection is not a toggle action', () => {
  const selectIndex = source.indexOf('function selectInterface(');
  const nextFunctionIndex = source.indexOf('\nasync function ', selectIndex + 1);
  const selectBody = source.slice(selectIndex, nextFunctionIndex);

  expect(selectIndex).toBeGreaterThan(-1);
  expect(selectBody).toContain('expandedInterfaceId.value = interfaceId');
  expect(selectBody).not.toContain(
    'expandedInterfaceId.value === interfaceId ? null : interfaceId',
  );
  expect(source).not.toContain('function toggleExpanded');
});

test('interface row actions use icon buttons with enough fixed width', () => {
  const rowClickIndex = source.indexOf('@click="selectInterface(item.id)"');
  const rowIndex = source.lastIndexOf('<tr', rowClickIndex);
  const nextRowIndex = source.indexOf('</tr>', rowIndex);
  const rowSection = source.slice(rowIndex, nextRowIndex);

  expect(packageSource).toContain('"lucide-vue-next"');
  expect(source).toContain("from 'lucide-vue-next'");
  expect(source).toContain('Pencil');
  expect(source).toContain('Trash2');
  expect(rowSection).toContain('<IconButton');
  expect(rowSection).toContain('label="编辑接口"');
  expect(rowSection).toContain('label="删除接口"');
  expect(iconButtonSource).toContain(':aria-label="label"');
  expect(rowSection).toContain('<Pencil');
  expect(rowSection).toContain('<Trash2');
  expect(styleSource).toContain('.icon-button');
  expect(styleSource).toContain('height: 2rem');
  expect(styleSource).toContain('width: 2rem');
  expect(rowSection).not.toContain('>编辑</button>');
  expect(rowSection).not.toContain('>删除</button>');
});

test('interface table api token supports copying with an icon button', () => {
  const tokenIndex = source.indexOf('{{ maskToken(item.token) }}');
  const tokenCellIndex = source.lastIndexOf('<td', tokenIndex);
  const tokenCellEndIndex = source.indexOf('</td>', tokenIndex);
  const tokenCell = source.slice(tokenCellIndex, tokenCellEndIndex);

  expect(source).toContain('Copy');
  expect(tokenCell).toContain('{{ maskToken(item.token) }}');
  expect(tokenCell).toContain('<CopyButton');
  expect(tokenCell).toContain('label="复制 API Token"');
  expect(tokenCell).toContain('@click.stop="copyInterfaceToken(item.token)"');
  expect(copyButtonSource).toContain('<Copy');
  expect(source).toContain('function copyInterfaceToken');
});

test('expanded interface models render as copyable model-name tags instead of list rows', () => {
  const expandedIndex = source.indexOf('expandedInterfaceId === item.id');
  const nextTemplateIndex = source.indexOf('</template>', expandedIndex);
  const expandedSection = source.slice(expandedIndex, nextTemplateIndex);

  expect(expandedSection).toContain('flex flex-wrap gap-2');
  expect(expandedSection).toContain('v-for="model in item.models"');
  expect(expandedSection).toContain('copy-tag');
  expect(styleSource).toContain('.copy-tag');
  expect(expandedSection).toContain('@click="copyInterfaceModelName(model.model_name)"');
  expect(expandedSection).toContain('aria-label="复制模型名"');
  expect(expandedSection).toContain('<Copy');
  expect(expandedSection).not.toContain('aria-label="删除接口模型"');
  expect(expandedSection).not.toContain('<X');
  expect(expandedSection).not.toContain('grid gap-2');
  expect(expandedSection).not.toContain('>删除</button>');
  expect(expandedSection).not.toContain('providerForModel(model)');
  expect(expandedSection).not.toContain('model.upstream_model');
  expect(source).toContain('function copyInterfaceModelName');
});

test('interface drawer model collection stays editable as a list while table details use tags', () => {
  const titleIndex = source.indexOf('<h4 class="font-semibold text-stone-800">模型列表</h4>');
  const addFormIndex = source.indexOf('border-t border-stone-100', titleIndex);
  const modelSection = source.slice(titleIndex, addFormIndex);

  expect(packageSource).toContain('"lucide-vue-next"');
  expect(modelSection).toContain('grid gap-2');
  expect(modelSection).toContain('v-for="model in form.models"');
  expect(modelSection).toContain('flex items-center justify-between gap-3 rounded-lg');
  expect(modelSection).toContain('@click="removeModelFromForm(model)"');
  expect(modelSection).toContain('删除');
  expect(source).not.toContain('function deleteInterfaceModel');
  expect(source).toContain('function removeModelFromForm');
  expect(modelSection).not.toContain('rounded-full border border-stone-200 bg-white');
});

test('interface save uses one aggregate request and does not call legacy model CRUD endpoints', () => {
  const saveIndex = source.indexOf('async function saveInterface');
  const nextFunctionIndex = source.indexOf('\nfunction ', saveIndex + 1);
  const saveBody = source.slice(saveIndex, nextFunctionIndex);

  expect(saveBody).toContain('persistInterface(configApi, command)');
  expect(saveBody).toContain('models: interfaceModelsFromForm()');
  expect(source).not.toContain('configApi.createInterfaceModel(');
  expect(source).not.toContain('configApi.deleteInterfaceModel(');
});

test('interface list exposes distinct loading error empty and ready states with retry', () => {
  expect(source).toContain('useManagementList<ProviderConfig>');
  expect(source).toContain('useManagementList<InterfaceResponse>');
  expect(source).toContain('managementErrorMessage');
  expect(source).toContain('v-if="loading"');
  expect(source).toContain('v-else-if="loadError"');
  expect(source).toContain('role="alert"');
  expect(source).toContain('@click="loadData"');
  expect(source).toContain('<RefreshCw');
  expect(source).toContain('v-else-if="filteredInterfaces.length === 0"');
  expect(source).not.toContain('v-if="!loading && filteredInterfaces.length === 0"');
});

test('provider and interface loads use independent latest-request state', () => {
  expect(source).toContain('loadProviders,');
  expect(source).toContain('loadInterfaces,');
  expect(source).toContain('Promise.all([loadProviders(), loadInterfaces()])');
});

test('successful interface save closes the drawer and refreshes while failure keeps it open', () => {
  const saveIndex = source.indexOf('async function saveInterface');
  const nextFunctionIndex = source.indexOf('\nfunction ', saveIndex + 1);
  const saveBody = source.slice(saveIndex, nextFunctionIndex);
  const catchIndex = saveBody.indexOf('} catch (error) {');

  expect(saveBody).toContain('await persistInterface(configApi, command)');
  expect(saveBody).toContain('drawerOpen.value = false');
  expect(saveBody).toContain('await loadData()');
  expect(saveBody.indexOf('drawerOpen.value = false')).toBeGreaterThan(catchIndex);
  expect(saveBody.slice(catchIndex)).toContain('managementErrorMessage(error)');
});

test('interface deletion is deduplicated by id and reports management errors', () => {
  const deleteIndex = source.indexOf('async function deleteInterface');
  const nextFunctionIndex = source.indexOf('\nfunction ', deleteIndex + 1);
  const deleteBody = source.slice(deleteIndex, nextFunctionIndex);

  expect(source).toContain(':disabled="isInterfaceDeleteBusy(item.id)"');
  expect(deleteBody).toContain('isInterfaceDeleteBusy(item.id)');
  expect(deleteBody).toContain('try {');
  expect(deleteBody).toContain('managementErrorMessage(error)');
  expect(source).toContain('{{ actionError }}');
});
