import { expect, test } from 'bun:test';
import * as apiModule from '../src/api';
import { managementErrorMessage } from '../src/utils/apiErrors';

type ReadAdminToken = (storage: Pick<Storage, 'getItem'> | null) => string | null;

test('management api 401 explains how to fix the admin credential', () => {
  expect(
    managementErrorMessage({
      isAxiosError: true,
      response: { status: 401, data: { error: 'unauthorized' } },
    }),
  ).toBe('管理凭据无效或缺失，请检查 ADMIN_TOKEN。');
});

test('management api network failure explains that the service cannot be reached', () => {
  expect(managementErrorMessage({ isAxiosError: true, request: {} })).toBe(
    '无法连接管理服务，请检查服务状态后重试。',
  );
});

test('management api uses a backend error string when available', () => {
  expect(
    managementErrorMessage({
      isAxiosError: true,
      response: { status: 500, data: { error: '数据库暂不可用' } },
    }),
  ).toBe('数据库暂不可用');
});

test('management api falls back to a stable loading error', () => {
  expect(
    managementErrorMessage({
      isAxiosError: true,
      response: { status: 500, data: {} },
    }),
  ).toBe('加载失败，请稍后重试。');
});

test('admin token reader is available to isolate storage access', () => {
  expect(adminTokenReader()).toBeFunction();
});

test('admin token reader trims a stored credential', () => {
  const readAdminToken = adminTokenReader();
  expect(readAdminToken).toBeFunction();
  if (!readAdminToken) {
    return;
  }

  expect(readAdminToken({ getItem: () => '  admin-secret  ' })).toBe('admin-secret');
});

test('admin token reader tolerates unavailable storage', () => {
  const readAdminToken = adminTokenReader();
  expect(readAdminToken).toBeFunction();
  if (!readAdminToken) {
    return;
  }

  expect(readAdminToken(null)).toBeNull();
});

test('admin token reader tolerates storage access errors', () => {
  const readAdminToken = adminTokenReader();
  expect(readAdminToken).toBeFunction();
  if (!readAdminToken) {
    return;
  }

  expect(
    readAdminToken({
      getItem() {
        throw new Error('storage blocked');
      },
    }),
  ).toBeNull();
});

function adminTokenReader(): ReadAdminToken | undefined {
  return (apiModule as unknown as { readAdminToken?: ReadAdminToken }).readAdminToken;
}
