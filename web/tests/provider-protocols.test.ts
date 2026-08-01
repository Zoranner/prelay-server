import { expect, test } from 'bun:test';
import {
  PROVIDER_GROUPS,
  PROVIDER_TEMPLATE_GROUPS,
  providerLabel,
  providerProtocolValuesForTemplate,
  providerTemplateByValue,
  providerTemplateForProviderType,
} from '../src/utils/providers';
import {
  defaultProtocolValuesForProvider,
  normalizeProviderProtocolBaseUrls,
  protocolTagClass,
  protocolOptionsForProvider,
  upstreamProtocolOptionsForProvider,
} from '../src/utils/providerProtocols';

test('responses providers expose only supported downstream protocols by default', () => {
  expect(protocolOptionsForProvider('responses_compatible').map((item) => item.value)).toEqual([
    'responses',
    'anthropic_messages',
  ]);
  expect(defaultProtocolValuesForProvider('responses_compatible')).toEqual([
    'responses',
    'anthropic_messages',
  ]);
});

test('built-in providers use template capabilities instead of the saved protocol-specific type', () => {
  expect(protocolOptionsForProvider('kimi_coding_anthropic').map((item) => item.value)).toEqual([
    'responses',
    'chat_completions',
    'anthropic_messages',
  ]);
});

test('provider list protocol labels use upstream capabilities instead of downstream bridge options', () => {
  expect(
    upstreamProtocolOptionsForProvider('kimi_coding_anthropic').map((item) => item.value),
  ).toEqual(['openai', 'anthropic']);
  expect(upstreamProtocolOptionsForProvider('openai_compatible').map((item) => item.value)).toEqual(
    ['openai'],
  );
  expect(
    upstreamProtocolOptionsForProvider('openai_compatible', {
      upstream_protocols: ['anthropic'],
    }).map((item) => item.value),
  ).toEqual(['anthropic']);
});

test('protocol tags share the same visual class across pages', () => {
  expect(protocolTagClass('responses')).toBe('border-[#b7d8cf] bg-[#e8f4f0] text-[#176b5d]');
  expect(protocolTagClass('openai')).toBe('border-[#c7d6f4] bg-[#edf3ff] text-[#2f63d7]');
  expect(protocolTagClass('chat_completions')).toBe('border-[#c7d6f4] bg-[#edf3ff] text-[#2f63d7]');
  expect(protocolTagClass('anthropic')).toBe('border-[#e6c8aa] bg-[#f5ece0] text-[#8b5230]');
  expect(protocolTagClass('anthropic_messages')).toBe(
    'border-[#e6c8aa] bg-[#f5ece0] text-[#8b5230]',
  );
});

test('custom provider declarations override compatible provider type defaults', () => {
  expect(
    protocolOptionsForProvider('openai_compatible', {
      upstream_protocols: ['anthropic'],
    }).map((item) => item.value),
  ).toEqual(['responses', 'anthropic_messages']);
});

test('openai-compatible providers expose all bridged downstream protocols', () => {
  expect(protocolOptionsForProvider('zhipu_coding_openai').map((item) => item.value)).toEqual([
    'responses',
    'chat_completions',
    'anthropic_messages',
  ]);
});

test('provider templates hide protocol variants behind a provider first selection', () => {
  expect(providerTemplateForProviderType('kimi_coding_anthropic')?.value).toBe('kimi_code');
  expect(providerTemplateByValue('kimi_code')?.providerType).toBe('kimi_coding_anthropic');
  expect(providerProtocolValuesForTemplate('kimi_code')).toEqual(['openai', 'anthropic']);
});

test('api service providers expose officially supported provider protocol capabilities', () => {
  expect(providerProtocolValuesForTemplate('kimi')).toEqual(['openai']);
  expect(providerProtocolValuesForTemplate('deepseek')).toEqual(['openai', 'anthropic']);
  expect(providerProtocolValuesForTemplate('bailian')).toEqual([
    'openai',
    'anthropic',
    'responses',
  ]);
  expect(providerProtocolValuesForTemplate('bigmodel')).toEqual(['openai', 'anthropic']);
  expect(providerProtocolValuesForTemplate('minimax')).toEqual([
    'openai',
    'anthropic',
    'responses',
  ]);

  expect(providerTemplateByValue('deepseek')?.providerType).toBe('deepseek');
  expect(providerTemplateByValue('bailian')?.providerType).toBe('qwen');
  expect(providerTemplateByValue('bigmodel')?.providerType).toBe('zhipu');
  expect(providerTemplateByValue('minimax')?.providerType).toBe('minimax');
});

test('only custom provider templates expose user selectable protocol declarations', () => {
  expect(providerTemplateByValue('kimi_code')?.custom).toBe(false);
  expect(providerTemplateByValue('deepseek')?.custom).toBe(false);
  expect(providerTemplateByValue('custom')?.custom).toBe(true);
  expect(providerProtocolValuesForTemplate('custom')).toEqual(['openai', 'anthropic', 'responses']);
});

test('provider upstream protocols use chat completions first fixed order', () => {
  expect(providerProtocolValuesForTemplate('kimi_code')).toEqual(['openai', 'anthropic']);
  expect(providerProtocolValuesForTemplate('deepseek')).toEqual(['openai', 'anthropic']);
  expect(providerProtocolValuesForTemplate('bailian')).toEqual([
    'openai',
    'anthropic',
    'responses',
  ]);
  expect(
    upstreamProtocolOptionsForProvider('openai_compatible', {
      upstream_protocols: ['responses', 'anthropic', 'openai'],
    }).map((item) => item.value),
  ).toEqual(['openai', 'anthropic', 'responses']);
});

test('provider protocol base urls normalize null values from backend responses', () => {
  expect(
    normalizeProviderProtocolBaseUrls({
      responses: null,
      openai: ' https://chat.example/v1 ',
      anthropic: undefined,
    }),
  ).toEqual({
    responses: '',
    openai: 'https://chat.example/v1',
    anthropic: '',
  });
});

test('built-in provider names use common domestic official names only', () => {
  expect(PROVIDER_TEMPLATE_GROUPS).toEqual([
    {
      label: '套餐服务',
      options: [
        expect.objectContaining({ value: 'kimi_code', label: 'Kimi Code' }),
        expect.objectContaining({ value: 'bigmodel_coding_plan', label: 'GLM Coding Plan' }),
        expect.objectContaining({ value: 'minimax_token_plan', label: 'MiniMax Token Plan' }),
      ],
    },
    {
      label: 'API 服务',
      options: [
        expect.objectContaining({ value: 'kimi', label: 'Kimi' }),
        expect.objectContaining({ value: 'deepseek', label: 'DeepSeek' }),
        expect.objectContaining({ value: 'bailian', label: '阿里云百炼' }),
        expect.objectContaining({ value: 'bigmodel', label: '智谱AI开放平台' }),
        expect.objectContaining({ value: 'minimax', label: 'MiniMax' }),
      ],
    },
    {
      label: '其他服务',
      options: [expect.objectContaining({ value: 'custom', label: '自定义' })],
    },
  ]);

  const exposedProviderNames = PROVIDER_TEMPLATE_GROUPS.flatMap((group) =>
    group.options.map((option) => option.label),
  );
  expect(exposedProviderNames.join(' ')).not.toContain('Z.AI');
  expect(exposedProviderNames.join(' ')).not.toContain('OpenAI');
  expect(exposedProviderNames.join(' ')).not.toContain('Anthropic');
  expect(exposedProviderNames.join(' ')).not.toContain('API');
});

test('bailian is exposed only as an api service provider', () => {
  expect(providerTemplateByValue('bailian_coding_plan')).toBeUndefined();
  expect(providerTemplateByValue('bailian_token_plan')).toBeUndefined();
  expect(providerTemplateByValue('bailian')?.label).toBe('阿里云百炼');
});

test('provider labels stay normalized across protocol-specific provider types', () => {
  expect(PROVIDER_GROUPS).toEqual([
    {
      label: '套餐服务',
      options: [
        { value: 'kimi_coding_anthropic', label: 'Kimi Code' },
        { value: 'zhipu_coding', label: 'GLM Coding Plan' },
        { value: 'minimax_token', label: 'MiniMax Token Plan' },
      ],
    },
    {
      label: 'API 服务',
      options: [
        { value: 'kimi', label: 'Kimi' },
        { value: 'deepseek', label: 'DeepSeek' },
        { value: 'qwen', label: '阿里云百炼' },
        { value: 'zhipu', label: '智谱AI开放平台' },
        { value: 'minimax', label: 'MiniMax' },
      ],
    },
    {
      label: '其他服务',
      options: [{ value: 'openai_compatible', label: '自定义' }],
    },
  ]);

  expect(providerLabel('kimi_coding')).toBe('Kimi Code');
  expect(providerLabel('bailian_coding_anthropic')).toBe('百炼 Coding Plan');
  expect(providerLabel('bailian_token_anthropic')).toBe('百炼 Token Plan');
  expect(providerLabel('zhipu')).toBe('智谱AI开放平台');
  expect(providerLabel('zhipu_anthropic')).toBe('智谱AI开放平台');
  expect(providerLabel('zhipu_coding')).toBe('GLM Coding Plan');
  expect(providerLabel('minimax_token')).toBe('MiniMax Token Plan');
});
