export type ViewKey = 'stats' | 'providers' | 'interfaces';

export interface AppRoute {
  label: string;
  value: ViewKey;
  path: string;
}

export const APP_ROUTES: AppRoute[] = [
  { label: '统计', value: 'stats', path: '/stats' },
  { label: '供应商', value: 'providers', path: '/providers' },
  { label: '接口', value: 'interfaces', path: '/interfaces' },
];

export function routeViewForPath(pathname: string): ViewKey {
  return APP_ROUTES.find((route) => route.path === pathname)?.value ?? 'stats';
}

export function pathForView(view: ViewKey): string {
  return APP_ROUTES.find((route) => route.value === view)?.path ?? '/stats';
}

export function defaultRoutePathForPath(pathname: string): string | null {
  return APP_ROUTES.some((route) => route.path === pathname) ? null : '/stats';
}
