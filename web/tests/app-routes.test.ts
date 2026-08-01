import { expect, test } from 'bun:test';
import {
  APP_ROUTES,
  defaultRoutePathForPath,
  pathForView,
  routeViewForPath,
} from '../src/utils/appRoutes';

test('stats providers and interfaces use three top-level routes', () => {
  expect(APP_ROUTES).toEqual([
    { label: '统计', value: 'stats', path: '/stats' },
    { label: '供应商', value: 'providers', path: '/providers' },
    { label: '接口', value: 'interfaces', path: '/interfaces' },
  ]);
});

test('app routes map browser paths to views', () => {
  expect(routeViewForPath('/stats')).toBe('stats');
  expect(routeViewForPath('/providers')).toBe('providers');
  expect(routeViewForPath('/interfaces')).toBe('interfaces');
  expect(routeViewForPath('/')).toBe('stats');
  expect(routeViewForPath('/unknown')).toBe('stats');
});

test('app routes redirect missing routes to the default stats path', () => {
  expect(defaultRoutePathForPath('/stats')).toBeNull();
  expect(defaultRoutePathForPath('/providers')).toBeNull();
  expect(defaultRoutePathForPath('/interfaces')).toBeNull();
  expect(defaultRoutePathForPath('/')).toBe('/stats');
  expect(defaultRoutePathForPath('/unknown')).toBe('/stats');
});

test('app routes map views to browser paths', () => {
  expect(pathForView('stats')).toBe('/stats');
  expect(pathForView('providers')).toBe('/providers');
  expect(pathForView('interfaces')).toBe('/interfaces');
});
