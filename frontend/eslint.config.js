import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import pluginVue from 'eslint-plugin-vue';
import prettierConfig from 'eslint-config-prettier';
import globals from 'globals';

export default tseslint.config(
  // 忽略构建产物
  { ignores: ['../static/**', 'dist/**', 'node_modules/**'] },

  // JS 基础规则
  js.configs.recommended,

  // TypeScript 规则
  ...tseslint.configs.recommended,

  // Vue 3 规则
  ...pluginVue.configs['flat/recommended'],

  // 关闭与 Prettier 冲突的格式化规则（必须放最后）
  prettierConfig,

  // 全局配置
  {
    files: ['**/*.{ts,vue}'],
    languageOptions: {
      globals: globals.browser,
      parserOptions: {
        parser: tseslint.parser,
        extraFileExtensions: ['.vue'],
      },
    },
    rules: {
      // 单文件组件允许单词组件名（内部工具无需严格命名）
      'vue/multi-word-component-names': 'off',
      // 允许在 setup script 中不使用 defineOptions
      'vue/require-default-prop': 'off',
      // TypeScript 相关放宽
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
    },
  },
);
