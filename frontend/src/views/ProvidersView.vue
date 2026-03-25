<template>
  <div class="page">
    <!-- Header -->
    <div class="header">
      <div class="header-inner">
        <div>
          <h1 class="header-title">🔀 Provider Relay</h1>
          <p class="header-sub">大模型服务透传代理 — 让内网用户访问外网 AI 服务</p>
        </div>
      </div>
    </div>

    <div class="content">
      <!-- Proxy Usage Info -->
      <el-alert type="info" :closable="false" class="usage-alert">
        <template #default>
          <div class="usage-info">
            <strong>使用方法：</strong>
            在 AI 工具中将 Base URL 设置为
            <el-tag size="small" type="info" class="url-tag">{{ proxyUrl }}</el-tag>
            ，API Key 填写下方配置对应的 <strong>Token</strong>，即可通过代理访问外网大模型服务。
          </div>
        </template>
      </el-alert>

      <!-- Main Card -->
      <el-card class="main-card" shadow="never">
        <template #header>
          <div class="card-header">
            <span class="card-title">服务提供商配置</span>
            <el-button type="primary" :icon="Plus" @click="openCreateDialog">添加配置</el-button>
          </div>
        </template>

        <el-table :data="configs" v-loading="loading" empty-text="暂无配置，点击右上角添加">
          <el-table-column label="名称" prop="name" min-width="140" />

          <el-table-column label="提供商" width="120">
            <template #default="{ row }">
              <el-tag :type="tagType(row.provider_type)" size="small">
                {{ providerLabel(row.provider_type) }}
              </el-tag>
            </template>
          </el-table-column>

          <el-table-column label="Base URL" prop="base_url" min-width="220" show-overflow-tooltip />

          <el-table-column label="API Key" width="160">
            <template #default="{ row }">
              <code class="masked-key">{{ row.api_key_masked }}</code>
            </template>
          </el-table-column>

          <el-table-column label="内部 Token（粘贴到 AI 工具的 API Key 栏）" min-width="300">
            <template #default="{ row }">
              <div class="token-row">
                <el-input
                  :model-value="row.token"
                  readonly
                  size="small"
                  class="token-input"
                />
                <el-tooltip content="复制 Token" placement="top">
                  <el-button
                    size="small"
                    :icon="CopyDocument"
                    circle
                    @click="copyToken(row.token)"
                  />
                </el-tooltip>
              </div>
            </template>
          </el-table-column>

          <el-table-column label="操作" width="210" fixed="right">
            <template #default="{ row }">
              <el-button size="small" @click="openEditDialog(row)">编辑</el-button>
              <el-button
                size="small"
                type="warning"
                :loading="regeneratingId === row.id"
                @click="regenerateToken(row)"
              >
                刷新 Token
              </el-button>
              <el-button size="small" type="danger" @click="confirmDelete(row)">删除</el-button>
            </template>
          </el-table-column>
        </el-table>
      </el-card>
    </div>

    <!-- Add / Edit Dialog -->
    <el-dialog
      v-model="dialogVisible"
      :title="isEditing ? '编辑配置' : '添加配置'"
      width="540px"
      @closed="resetForm"
    >
      <el-form :model="form" :rules="rules" ref="formRef" label-width="110px">
        <el-form-item label="配置名称" prop="name">
          <el-input v-model="form.name" placeholder="例如：公司 OpenAI Key" />
        </el-form-item>

        <el-form-item label="提供商类型" prop="provider_type">
          <el-select v-model="form.provider_type" @change="onTypeChange" style="width: 100%">
            <el-option label="OpenAI" value="openai" />
            <el-option label="Anthropic Claude" value="anthropic" />
            <el-option label="Azure OpenAI" value="azure" />
            <el-option label="自定义 / 其他兼容接口" value="custom" />
          </el-select>
        </el-form-item>

        <el-form-item label="Base URL" prop="base_url">
          <el-input v-model="form.base_url" placeholder="https://api.openai.com" />
          <div class="form-hint">
            代理会将请求转发到 <code>{{ form.base_url.trim() || 'Base URL' }}/v1/chat/completions</code> 等路径
          </div>
        </el-form-item>

        <el-form-item label="API Key" prop="api_key">
          <el-input
            v-model="form.api_key"
            type="password"
            show-password
            :placeholder="isEditing ? '留空则保持不变' : '填写真实的 API Key'"
          />
        </el-form-item>
      </el-form>

      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" :loading="submitting" @click="submitForm">
          {{ isEditing ? '保存修改' : '创建配置' }}
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { FormInstance, FormRules } from 'element-plus'
import { Plus, CopyDocument } from '@element-plus/icons-vue'
import { configApi, type ProviderConfig } from '../api/index'

const configs = ref<ProviderConfig[]>([])
const loading = ref(false)
const dialogVisible = ref(false)
const isEditing = ref(false)
const editingId = ref('')
const submitting = ref(false)
const regeneratingId = ref('')
const formRef = ref<FormInstance>()

const proxyUrl = `${window.location.origin}/v1`

const defaultBaseUrls: Record<string, string> = {
  openai: 'https://api.openai.com',
  anthropic: 'https://api.anthropic.com',
  azure: 'https://YOUR-RESOURCE.openai.azure.com',
  custom: '',
}

const form = ref({
  name: '',
  provider_type: 'openai',
  base_url: 'https://api.openai.com',
  api_key: '',
})

const rules: FormRules = {
  name: [{ required: true, message: '请输入配置名称', trigger: 'blur' }],
  provider_type: [{ required: true, message: '请选择提供商类型', trigger: 'change' }],
  base_url: [
    { required: true, message: '请输入 Base URL', trigger: 'blur' },
    {
      validator: (_rule, value: string, callback) => {
        if (value && !value.startsWith('http')) {
          callback(new Error('Base URL 需以 http:// 或 https:// 开头'))
        } else {
          callback()
        }
      },
      trigger: 'blur',
    },
  ],
  api_key: [
    {
      validator: (_rule, value: string, callback) => {
        if (!isEditing.value && !value.trim()) {
          callback(new Error('请填写 API Key'))
        } else {
          callback()
        }
      },
      trigger: 'blur',
    },
  ],
}

function onTypeChange(type: string) {
  form.value.base_url = defaultBaseUrls[type] ?? ''
}

function tagType(type: string): '' | 'success' | 'warning' | 'info' | 'danger' {
  const map: Record<string, '' | 'success' | 'warning' | 'info' | 'danger'> = {
    openai: 'success',
    anthropic: 'warning',
    azure: 'info',
    custom: '',
  }
  return map[type] ?? ''
}

function providerLabel(type: string): string {
  const map: Record<string, string> = {
    openai: 'OpenAI',
    anthropic: 'Anthropic',
    azure: 'Azure',
    custom: '自定义',
  }
  return map[type] ?? type
}

async function loadConfigs() {
  loading.value = true
  try {
    const res = await configApi.list()
    configs.value = res.data
  } catch {
    ElMessage.error('加载配置失败，请检查服务是否运行')
  } finally {
    loading.value = false
  }
}

function openCreateDialog() {
  isEditing.value = false
  editingId.value = ''
  dialogVisible.value = true
}

function openEditDialog(config: ProviderConfig) {
  isEditing.value = true
  editingId.value = config.id
  form.value = {
    name: config.name,
    provider_type: config.provider_type,
    base_url: config.base_url,
    api_key: '',
  }
  dialogVisible.value = true
}

function resetForm() {
  form.value = { name: '', provider_type: 'openai', base_url: 'https://api.openai.com', api_key: '' }
  formRef.value?.clearValidate()
}

async function submitForm() {
  const valid = await formRef.value?.validate().catch(() => false)
  if (!valid) return

  submitting.value = true
  try {
    if (isEditing.value) {
      const payload: Record<string, string> = {
        name: form.value.name,
        provider_type: form.value.provider_type,
        base_url: form.value.base_url,
      }
      if (form.value.api_key.trim()) payload.api_key = form.value.api_key
      await configApi.update(editingId.value, payload)
      ElMessage.success('配置已更新')
    } else {
      await configApi.create(form.value)
      ElMessage.success('配置已创建')
    }
    dialogVisible.value = false
    await loadConfigs()
  } catch {
    ElMessage.error(isEditing.value ? '更新失败' : '创建失败')
  } finally {
    submitting.value = false
  }
}

async function regenerateToken(config: ProviderConfig) {
  try {
    await ElMessageBox.confirm(
      `刷新后，使用旧 Token 的连接将立即失效。确定刷新「${config.name}」的 Token 吗？`,
      '确认刷新',
      { type: 'warning', confirmButtonText: '确认刷新', cancelButtonText: '取消' }
    )
    regeneratingId.value = config.id
    const res = await configApi.regenerateToken(config.id)
    await loadConfigs()
    await copyToClipboard(res.data.token)
    ElMessage.success('Token 已刷新并复制到剪贴板')
  } catch (e) {
    if (e !== 'cancel') ElMessage.error('刷新失败')
  } finally {
    regeneratingId.value = ''
  }
}

async function confirmDelete(config: ProviderConfig) {
  try {
    await ElMessageBox.confirm(
      `确定要删除配置「${config.name}」吗？此操作不可撤销。`,
      '确认删除',
      { type: 'warning', confirmButtonText: '确认删除', cancelButtonText: '取消' }
    )
    await configApi.delete(config.id)
    ElMessage.success('已删除')
    await loadConfigs()
  } catch (e) {
    if (e !== 'cancel') ElMessage.error('删除失败')
  }
}

async function copyToken(token: string) {
  const ok = await copyToClipboard(token)
  if (ok) {
    ElMessage.success('Token 已复制到剪贴板')
  } else {
    ElMessage.warning('复制失败，请手动选中复制')
  }
}

async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    return false
  }
}

onMounted(loadConfigs)
</script>

<style scoped>
.page {
  min-height: 100vh;
}

.header {
  background: linear-gradient(135deg, #4f46e5 0%, #7c3aed 100%);
  padding: 28px 0;
  margin-bottom: 24px;
}

.header-inner {
  max-width: 1200px;
  margin: 0 auto;
  padding: 0 24px;
}

.header-title {
  margin: 0 0 6px;
  font-size: 26px;
  font-weight: 700;
  color: #fff;
}

.header-sub {
  margin: 0;
  color: rgba(255, 255, 255, 0.8);
  font-size: 14px;
}

.content {
  max-width: 1200px;
  margin: 0 auto;
  padding: 0 24px 40px;
}

.usage-alert {
  margin-bottom: 20px;
}

.usage-info {
  line-height: 1.8;
  font-size: 14px;
}

.url-tag {
  font-family: monospace;
  font-size: 13px;
  margin: 0 4px;
}

.main-card {
  border-radius: 8px;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.card-title {
  font-size: 16px;
  font-weight: 600;
  color: #1f2937;
}

.masked-key {
  font-family: monospace;
  font-size: 13px;
  color: #6b7280;
  background: #f3f4f6;
  padding: 2px 6px;
  border-radius: 4px;
}

.token-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.token-input {
  font-family: monospace;
  flex: 1;
}

.form-hint {
  font-size: 12px;
  color: #9ca3af;
  margin-top: 4px;
  line-height: 1.5;
}

.form-hint code {
  background: #f3f4f6;
  padding: 1px 4px;
  border-radius: 3px;
  font-size: 11px;
  color: #374151;
}
</style>
