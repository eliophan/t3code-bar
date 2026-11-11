// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppSnapshot } from './contracts';
import { App } from './App';

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (event?: unknown) => void>(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: tauri.invoke }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, callback: (event?: unknown) => void) => {
    tauri.listeners.set(name, callback);
    return Promise.resolve(() => tauri.listeners.delete(name));
  }),
}));

const preview = { text: 'A fresh assistant reply', updatedAt: '2026-11-06T09:07:00Z' };
const snapshot = (showsPreviews: boolean): AppSnapshot => ({
  revision: 0, environments: [], manualEnvironmentIds: [], cloudStatus: 'signedOut', cloudError: null,
  now: '2026-11-06T09:08:00Z',
  threads: [{
    thread: { environmentId: 'fixture-environment', threadId: 'thread-1' }, projectId: 'project-1', projectTitle: 'Fixture', title: 'Review thread', status: 'Settled',
    isPinned: false, providers: ['codex'], branch: null, modelName: null, prNumber: null,
    updatedAt: '2026-11-06T09:07:00Z', latestUserMessageAt: '2026-11-06T09:07:00Z', latestRunCompletedAt: null,
    activeRunStartedAt: null, usageLimitResetAt: null, projectIcon: null, projectFavicon: null,
  }],
  online: true, threadError: null, loading: false,
  localConnection: { status: 'connected', message: 'Connected', connected: true, paired: true, hasData: true, port: 0, updatedAt: null },
  connection: { status: 'connected', message: 'Connected', connected: true, paired: true, hasData: true, port: 0, updatedAt: null },
  usage: null, usageError: null, usageLoading: false, usagePeriod: '24h', limits: null,
  settings: { environmentSelection: 'all', notificationsMuted: false, showsPreviews, automaticallyChecksForUpdates: false, installsWithoutAsking: false },
  update: { status: 'idle', message: '', version: null, offerId: null, canInstall: false, busy: false }, version: 'test',
});

let root: Root;
let host: HTMLDivElement;

async function renderSnapshot(value: AppSnapshot) {
  tauri.invoke.mockImplementation((name: string) => name === 'get_snapshot' ? Promise.resolve(value) : Promise.resolve(null));
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => { root.render(<App />); });
}

function threadRow() {
  const row = host.querySelector<HTMLButtonElement>('.thread-row');
  expect(row).not.toBeNull();
  return row!;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  tauri.invoke.mockReset();
  tauri.listeners.clear();
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    disconnect() {}
  });
});

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('thread preview lifecycle', () => {
  it('exposes the provider/model as the Run on row metadata', async () => {
    const value = snapshot(true);
    value.threads[0] = { ...value.threads[0], modelName: 'gpt-5.6-luna', providers: ['codex'] };
    await renderSnapshot(value);
    const providers = host.querySelector<HTMLElement>('.providers');
    expect(providers?.getAttribute('title')).toBe('Run on gpt-5.6-luna');
    expect(providers?.getAttribute('aria-label')).toBe('Run on gpt-5.6-luna');
  });

  it('shows a recoverable startup error while keeping the app usable', async () => {
    await renderSnapshot({ ...snapshot(true), startupError: 'Settings could not be loaded. Using defaults for this session.' });
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('Settings could not be loaded');
    expect(threadRow()).not.toBeNull();
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Dismiss error"]')!.click());
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });
  it('waits 500ms before fetching and renders the completed preview', async () => {
    await renderSnapshot(snapshot(true));
    const row = threadRow();
    tauri.invoke.mockImplementation((name: string) => name === 'get_preview' ? Promise.resolve(preview) : Promise.resolve(null));

    act(() => row.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => vi.advanceTimersByTime(499));
    expect(tauri.invoke).not.toHaveBeenCalledWith('get_preview', { thread: { environmentId: 'fixture-environment', threadId: 'thread-1' }, revision: 0 });
    act(() => vi.advanceTimersByTime(1));
    await act(async () => { await Promise.resolve(); });
    expect(tauri.invoke).toHaveBeenCalledWith('get_preview', { thread: { environmentId: 'fixture-environment', threadId: 'thread-1' }, revision: 0 });
    expect(host.querySelector('[role="tooltip"]')?.textContent).toContain(preview.text);
  });

  it('cancels a pending hover fetch when the pointer leaves', async () => {
    await renderSnapshot(snapshot(true));
    const row = threadRow();
    act(() => row.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => row.dispatchEvent(new MouseEvent('mouseout', { bubbles: true, relatedTarget: document.body })));
    act(() => vi.advanceTimersByTime(500));
    expect(tauri.invoke).not.toHaveBeenCalledWith('get_preview', { thread: { environmentId: 'fixture-environment', threadId: 'thread-1' }, revision: 0 });
  });

  it('ignores a delayed preview result after the panel reopens', async () => {
    await renderSnapshot(snapshot(true));
    const row = threadRow();
    let resolvePreview!: (value: typeof preview) => void;
    tauri.invoke.mockImplementation((name: string) => name === 'get_preview' ? new Promise(resolve => { resolvePreview = resolve; }) : Promise.resolve(null));

    act(() => row.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => vi.advanceTimersByTime(500));
    expect(tauri.invoke).toHaveBeenCalledWith('get_preview', { thread: { environmentId: 'fixture-environment', threadId: 'thread-1' }, revision: 0 });
    act(() => tauri.listeners.get('bar-opened')?.());
    await act(async () => { resolvePreview(preview); await Promise.resolve(); });
    expect(host.querySelector('[role="tooltip"]')).toBeNull();
  });

  it('clears an already visible preview when the panel reopens', async () => {
    await renderSnapshot(snapshot(true));
    tauri.invoke.mockImplementation((name: string) => name === 'get_preview' ? Promise.resolve(preview) : Promise.resolve(null));
    act(() => threadRow().dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => vi.advanceTimersByTime(500));
    await act(async () => { await Promise.resolve(); });
    expect(host.querySelector('[role="tooltip"]')?.textContent).toContain(preview.text);

    act(() => tauri.listeners.get('bar-opened')?.());
    expect(host.querySelector('[role="tooltip"]')).toBeNull();
  });

  it('ignores a delayed preview after the pointer leaves', async () => {
    await renderSnapshot(snapshot(true));
    const row = threadRow();
    let resolvePreview!: (value: typeof preview) => void;
    tauri.invoke.mockImplementation((name: string) => name === 'get_preview' ? new Promise(resolve => { resolvePreview = resolve; }) : Promise.resolve(null));
    act(() => row.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => vi.advanceTimersByTime(500));
    act(() => row.dispatchEvent(new MouseEvent('mouseout', { bubbles: true, relatedTarget: document.body })));
    await act(async () => { resolvePreview(preview); await Promise.resolve(); });
    expect(host.querySelector('[role="tooltip"]')).toBeNull();
  });

  it('keeps equal thread IDs in different environments distinct', async () => {
    const value = snapshot(true);
    value.threads.push({ ...value.threads[0], thread: { environmentId: 'other-environment', threadId: 'thread-1' }, title: 'Other thread' });
    await renderSnapshot(value);
    const rows = host.querySelectorAll<HTMLButtonElement>('.thread-row');
    expect(rows).toHaveLength(2);
    act(() => rows[1].click());
    expect(tauri.invoke).toHaveBeenCalledWith('thread_action', { thread: { environmentId: 'other-environment', threadId: 'thread-1' }, action: 'open' });
  });

  it('discards a preview when the selection revision changes', async () => {
    await renderSnapshot(snapshot(true));
    let resolvePreview!: (value: typeof preview) => void;
    tauri.invoke.mockImplementation((name: string) => name === 'get_preview' ? new Promise(resolve => { resolvePreview = resolve; }) : Promise.resolve(null));
    act(() => threadRow().dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => vi.advanceTimersByTime(500));
    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: { ...snapshot(true), revision: 1 } }));
    await act(async () => { resolvePreview(preview); await Promise.resolve(); });
    expect(host.querySelector('[role="tooltip"]')).toBeNull();
  });

  it('does not fetch previews when the setting is disabled', async () => {
    await renderSnapshot(snapshot(false));
    act(() => threadRow().dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => vi.advanceTimersByTime(500));
    expect(tauri.invoke).not.toHaveBeenCalledWith('get_preview', { thread: { environmentId: 'fixture-environment', threadId: 'thread-1' }, revision: 0 });
  });

  it('cancels a pending preview when the setting is disabled while hovering', async () => {
    await renderSnapshot(snapshot(true));
    act(() => threadRow().dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: snapshot(false) }));
    act(() => vi.advanceTimersByTime(500));
    expect(tauri.invoke).not.toHaveBeenCalledWith('get_preview', { thread: { environmentId: 'fixture-environment', threadId: 'thread-1' }, revision: 0 });
    expect(host.querySelector('[role="tooltip"]')).toBeNull();
  });
});

describe('new-install pairing', () => {
  it('requires a pairing key even when T3 Code already has cached data', async () => {
    const value = snapshot(true);
    value.connection = { ...value.connection, connected: false, paired: false, status: 'cacheOnly' };
    value.localConnection = { ...value.connection };
    await renderSnapshot(value);
    expect(host.querySelector('.onboarding')).not.toBeNull();
    expect(host.querySelector('.thread-row')).toBeNull();
    expect(host.querySelector('input[type="password"]')?.getAttribute('placeholder')).toBe('Local pairing key');
    expect(host.textContent).not.toContain('Auto-Generate');

    const input = host.querySelector<HTMLInputElement>('input[type="password"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'synthetic-pairing-key');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => { host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); });
    expect(tauri.invoke).toHaveBeenCalledWith('connect', { token: 'synthetic-pairing-key' });
    expect(input.value).toBe('');

    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: snapshot(true) }));
    expect(host.querySelector('.onboarding')).toBeNull();
    expect(host.querySelector('.thread-row')).not.toBeNull();

    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: value }));
    expect(host.querySelector('.onboarding')).not.toBeNull();
    expect(host.querySelector('.thread-row')).toBeNull();
  });

  it('keeps saved threads available to a paired installation while T3 Code is offline', async () => {
    const value = snapshot(true);
    value.online = false;
    value.connection = { ...value.connection, connected: false, paired: true, status: 'offline' };
    value.localConnection = { ...value.connection };
    await renderSnapshot(value);
    expect(host.querySelector('.onboarding')).toBeNull();
    expect(host.querySelector('.thread-row')).not.toBeNull();
    expect(host.textContent).toContain('T3 Code offline');
  });
});

describe('settings state and actions', () => {
  it('keeps an environment form and its failure visible when the panel reopens', async () => {
    await openSettings();
    act(() => button('Add environment').click());
    const input = host.querySelector<HTMLInputElement>('input[placeholder="https://server.example"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'https://remote.example');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    let rejectProbe!: (error: string) => void;
    tauri.invoke.mockImplementation((name: string) => name === 'probe_environment'
      ? new Promise((_resolve, reject) => { rejectProbe = reject; }) : Promise.resolve(null));
    await act(async () => button('Check server').click());
    act(() => tauri.listeners.get('bar-opened')?.());
    await act(async () => rejectProbe('Could not reach the server.'));
    expect(host.querySelector('form[aria-label="Add environment"]')).not.toBeNull();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('Could not reach the server');
    expect(host.querySelector<HTMLInputElement>('input[placeholder="https://server.example"]')?.value).toBe('https://remote.example');
  });

  async function openSettings(value = snapshot(true)) {
    await renderSnapshot(value);
    act(() => tauri.listeners.get('bar-settings')?.());
  }
  function button(label: string) {
    const found = [...host.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label);
    expect(found).toBeDefined();
    return found!;
  }
  it('offers the cloud action for each session status', async () => {
    await openSettings();
    await act(async () => button('Sign in to T3 Connect').click());
    expect(tauri.invoke).toHaveBeenCalledWith('begin_cloud_login', undefined);
    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: { ...snapshot(true), cloudStatus: 'signingIn' } }));
    await act(async () => button('Cancel').click());
    expect(tauri.invoke).toHaveBeenCalledWith('cancel_cloud_login', undefined);
    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: { ...snapshot(true), cloudStatus: 'signedIn' } }));
    await act(async () => button('Sign out').click());
    expect(tauri.invoke).toHaveBeenCalledWith('disconnect_cloud', undefined);
    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: { ...snapshot(true), cloudStatus: 'recoveryRequired' } }));
    expect(host.textContent).toContain('Some saved credentials may remain');
    await act(async () => button('Retry sign out').click());
    expect(tauri.invoke).toHaveBeenLastCalledWith('disconnect_cloud', undefined);
  });
  it('hides pairing while connected and requires confirmation before removing the key', async () => {
    await openSettings();
    expect(host.querySelector('input[type="password"]')).toBeNull();
    expect(host.textContent).toContain('Connected to T3 Code');
    act(() => button('Disconnect…').click());
    expect(tauri.invoke).not.toHaveBeenCalledWith('disconnect', undefined);
    act(() => button('Cancel').click());
    expect(host.querySelector('[aria-label="Confirm disconnect"]')).toBeNull();
    act(() => button('Disconnect…').click());
    await act(async () => button('Disconnect and remove key').click());
    expect(tauri.invoke).toHaveBeenCalledWith('disconnect', undefined);
  });
  it('maps Notifications enabled to the inverse muted preference', async () => {
    await openSettings();
    const notifications = [...host.querySelectorAll<HTMLLabelElement>('label')].find(label => label.textContent === 'Notifications')!.querySelector<HTMLInputElement>('input')!;
    expect(notifications.checked).toBe(true);
    await act(async () => notifications.click());
    expect(tauri.invoke).toHaveBeenCalledWith('set_setting', { key: 'notificationsMuted', value: true });
  });
  it('offers saved reconnect and opens pairing only when requested while offline', async () => {
    const value = snapshot(true);
    value.connection = { ...value.connection, connected: false, status: 'offline', message: 'Open T3 Code to connect.' };
    value.localConnection = { ...value.connection };
    await openSettings(value);
    expect(host.querySelector('input[type="password"]')).toBeNull();
    await act(async () => button('Reconnect').click());
    expect(tauri.invoke).toHaveBeenCalledWith('reconnect', undefined);
    act(() => button('Pair again…').click());
    expect(host.querySelector('input[type="password"]')).not.toBeNull();
    act(() => button('Cancel').click());
    expect(host.querySelector('input[type="password"]')).toBeNull();
  });
  it('disables unavailable updates and dependent automatic install controls', async () => {
    const value = snapshot(true);
    value.update.status = 'unavailable';
    await openSettings(value);
    expect(button('Check for Updates…').disabled).toBe(true);
    expect(host.textContent).toContain('Updates unavailable in this build');
    expect([...host.querySelectorAll<HTMLInputElement>('[role="switch"]')].slice(2).every(input => input.disabled)).toBe(true);
  });
});

describe('environment sources', () => {
  it('shows independently paired remote threads without requiring Local pairing', async () => {
    const value = snapshot(false);
    value.connection = { ...value.connection, connected: false, paired: false, status: 'disconnected' };
    value.localConnection = { ...value.connection };
    value.environments = [{ id: 'remote', label: 'Independent', machine: 'linux', status: 'online', access: 'direct', isLocal: false, canForgetAccess: true }];
    value.threads = [{ ...value.threads[0], thread: { environmentId: 'remote', threadId: 'one' }, title: 'Independent thread' }];
    await renderSnapshot(value);
    expect(threadRow().textContent).toContain('Independent thread');
    expect(host.querySelector('.onboarding')).toBeNull();
    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: { ...value, environments: [{ ...value.environments[0], status: 'offline' }] } }));
    expect(threadRow().textContent).toContain('Independent thread');
  });

  it('shows the environment icon only for threads owned by another machine', async () => {
    const value = snapshot(false);
    value.environments = [
      { id: 'here', label: 'This machine', isLocal: true, machine: 'linux', status: 'online', access: 'direct', canForgetAccess: false },
      { id: 'away', label: 'Other machine', isLocal: false, machine: 'laptop', status: 'online', access: 'direct', canForgetAccess: true },
    ];
    value.threads = [
      { ...value.threads[0], thread: { environmentId: 'here', threadId: 'same' }, title: 'Local thread' },
      { ...value.threads[0], thread: { environmentId: 'away', threadId: 'same' }, title: 'Remote thread' },
    ];
    await renderSnapshot(value);
    const rows = host.querySelectorAll<HTMLButtonElement>('.thread-row');
    expect(rows[0].querySelector('.environment-icon')).toBeNull();
    expect(rows[1].querySelector('.environment-icon')?.getAttribute('aria-label')).toBe('Other machine: Laptop');
  });

  it('shows each source icon and label for threads with the same ID', async () => {
    const value = snapshot(false);
    value.environments = [
      { id: 'a', label: 'Studio', machine: 'mac-mini', status: 'online', access: 'direct', isLocal: false, canForgetAccess: false },
      { id: 'b', label: 'Linux box', machine: 'linux', status: 'offline', access: 'direct', isLocal: false, canForgetAccess: false },
    ];
    value.threads = [
      { ...value.threads[0], thread: { environmentId: 'a', threadId: 'same' }, title: 'First' },
      { ...value.threads[0], thread: { environmentId: 'b', threadId: 'same' }, title: 'Second' },
    ];
    await renderSnapshot(value);
    const rows = host.querySelectorAll<HTMLButtonElement>('.thread-row');
    expect(rows).toHaveLength(2);
    expect(rows[0].getAttribute('aria-label')).toContain('Studio');
    expect(rows[0].querySelector('[role="img"]')?.getAttribute('aria-label')).toBe('Studio: Mini PC');
    expect(rows[1].getAttribute('aria-label')).toContain('Linux box');
    expect(rows[1].querySelector('[role="img"]')?.getAttribute('aria-label')).toBe('Linux box: Linux/WSL');
  });

  it('identifies the local pairing target while a remote environment is selected', async () => {
    const value = snapshot(false);
    value.connection = { ...value.connection, connected: false, paired: false, status: 'disconnected' };
    value.localConnection = { ...value.connection };
    value.settings.environmentSelection = { one: 'remote' };
    value.environments = [{ id: 'remote', label: 'Remote Mac', machine: 'laptop', status: 'needsAccess', access: 'direct', isLocal: false, canForgetAccess: false }];
    await renderSnapshot(value);
    act(() => [...host.querySelectorAll('button')].find(button => button.textContent === 'Settings')!.click());
    expect(host.textContent).toContain('Local T3 Code connection');
    expect(host.querySelector('label[for="access-token"]')?.textContent).toBe('Local pairing key');
    expect(host.textContent).toContain('Remote Mac pairing key');
  });

  it('routes needsAccess by route capability without leaking pairing forms', async () => {
    const value = snapshot(false);
    value.cloudStatus = 'signedOut';
    value.environments = [
      { id: 'r', label: 'Relay box', machine: 'cloud', status: 'needsAccess', access: 'cloud', isLocal: false, canForgetAccess: false },
      { id: 'd', label: 'Direct box', machine: 'linux', status: 'needsAccess', access: 'direct', isLocal: false, canForgetAccess: false },
      { id: 'm', label: 'Mixed box', machine: 'server', status: 'needsAccess', access: 'directOrCloud', isLocal: false, canForgetAccess: false },
      { id: 's', label: 'Ssh box', machine: 'server', status: 'needsAccess', access: 'unsupported', isLocal: false, canForgetAccess: false },
    ];
    await renderSnapshot(value);
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Settings"]')!.click());
    const forms = [...host.querySelectorAll('form')].map(form => form.textContent);
    expect(forms).toHaveLength(2);
    expect(forms.join('|')).toContain('Direct box pairing key');
    expect(forms.join('|')).toContain('Mixed box pairing key');
    expect(forms.join('|')).not.toContain('Relay box pairing key');
    expect(forms.join('|')).not.toContain('Ssh box pairing key');
    expect(host.textContent).toContain('Relay box: Sign in below to find its HTTPS address, then pair with this environment.');
    expect(host.textContent).not.toContain('Ssh box needs');
    expect([...host.querySelectorAll('button')].some(b => b.textContent === 'Sign in to T3 Connect')).toBe(true);
  });

  it('explains an unsupported Relay login without asking the user to sign out', async () => {
    const value = snapshot(false);
    value.cloudStatus = 'signedIn';
    value.environments = [{ id: 'r', label: 'Relay box', machine: 'cloud', status: 'unsupported', access: 'cloud', isLocal: false, canForgetAccess: false }];
    await renderSnapshot(value);
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Settings"]')!.click());
    expect(host.textContent).toContain('T3 Connect has not provided a pairing address for Relay box. Use Add environment in Bar with a reachable HTTPS address or SSH host.');
    expect(host.textContent).not.toContain('Sign out and sign in again');
  });

  it('offers new access with a clear notice when saved access was denied', async () => {
    const value = snapshot(false);
    value.environments = [
      { id: 'd', label: 'Direct box', machine: 'linux', status: 'permissionDenied', access: 'direct', isLocal: false, canForgetAccess: true },
      { id: 'm', label: 'Mixed box', machine: 'server', status: 'permissionDenied', access: 'directOrCloud', isLocal: false, canForgetAccess: true },
      { id: 'r', label: 'Relay box', machine: 'cloud', status: 'permissionDenied', access: 'cloud', isLocal: false, canForgetAccess: false },
      { id: 'o', label: 'Online box', machine: 'server', status: 'online', access: 'direct', isLocal: false, canForgetAccess: false },
    ];
    value.cloudStatus = 'signedIn';
    await renderSnapshot(value);
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Settings"]')!.click());
    const text = host.textContent ?? '';
    expect(text).toContain('Bar could not authorize access to Direct box. Enter a pairing key from this environment.');
    expect(text).toContain('Bar could not authorize access to Mixed box. Enter a new pairing key from this environment.');
    expect(text).toContain('T3 Connect could not authorize Relay box. Waiting for its HTTPS address. If unavailable, use Add environment with a server address or SSH host.');
    const forms = [...host.querySelectorAll('form')].map(form => form.textContent).join('|');
    expect(forms).toContain('Direct box pairing key');
    expect(forms).toContain('Mixed box pairing key');
    expect(forms).not.toContain('Relay box pairing key');
    expect(forms).not.toContain('Online box pairing key');
    const input = host.querySelector<HTMLInputElement>('form input[type="password"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'ABCD2345EFGH');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => { host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); });
    expect(tauri.invoke).toHaveBeenCalledWith('pair_environment', { environment: 'd', token: 'ABCD2345EFGH' });
  });

  it('forgets one environment after confirmation without using the primary disconnect', async () => {
    const value = snapshot(false);
    value.environments = [
      { id: 'a', label: 'Studio', machine: 'mac-mini', status: 'online', access: 'direct', isLocal: false, canForgetAccess: true },
      { id: 'b', label: 'Linux box', machine: 'linux', status: 'offline', access: 'directOrCloud', isLocal: false, canForgetAccess: true },
      { id: 'local', label: 'Local', machine: 'linux', status: 'online', access: 'direct', isLocal: true, canForgetAccess: false },
    ];
    await renderSnapshot(value);
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Settings"]')!.click());
    const open = [...host.querySelectorAll('button')].filter(b => b.textContent?.startsWith('Forget access for'));
    expect(open.map(b => b.textContent)).toEqual(['Forget access for Studio…', 'Forget access for Linux box…']);
    act(() => open[1].click());
    const group = host.querySelector('[aria-label="Confirm forgetting access for Linux box"]')!;
    expect(group.textContent).toContain('Other environments and T3 Connect are not affected');
    await act(async () => [...group.querySelectorAll('button')].find(b => b.textContent === 'Forget access')!.click());
    expect(tauri.invoke).toHaveBeenCalledWith('forget_environment_access', { environment: 'b' });
    expect(tauri.invoke).not.toHaveBeenCalledWith('disconnect', expect.anything());
    expect(tauri.invoke.mock.calls.some(([name]) => name === 'disconnect')).toBe(false);
  });

  it('shows Cancel for a Relay-only source while cloud sign-in is running', async () => {
    const value = snapshot(false);
    value.cloudStatus = 'signingIn';
    value.environments = [{ id: 'r', label: 'Relay box', machine: 'cloud', status: 'needsAccess', access: 'cloud', isLocal: false, canForgetAccess: false }];
    await renderSnapshot(value);
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Settings"]')!.click());
    expect(host.querySelectorAll('form')).toHaveLength(0);
    expect(host.textContent).toContain('Finish signing in or cancel below.');
    expect([...host.querySelectorAll('button')].some(b => b.textContent === 'Cancel')).toBe(true);
  });

  it('selects one environment and returns to All through inline controls', async () => {
    const value = snapshot(false);
    value.environments = [{ id: 'remote', label: 'Remote', machine: 'linux', status: 'online', access: 'direct', isLocal: false, canForgetAccess: false }];
    await renderSnapshot(value);
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Settings"]')!.click());
    const group = host.querySelector('[aria-label="Environment"]');
    await act(async () => group?.querySelector<HTMLInputElement>('input[value="remote"]')?.click());
    expect(tauri.invoke).toHaveBeenCalledWith('set_environment_selection', { selection: { one: 'remote' } });
    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: { ...value, settings: { ...value.settings, environmentSelection: { one: 'remote' } } } }));
    await act(async () => group?.querySelector<HTMLInputElement>('input[value=""]')?.click());
    expect(tauri.invoke).toHaveBeenLastCalledWith('set_environment_selection', { selection: 'all' });
  });

  it('keeps a missing selected environment in the selector', async () => {
    const value = snapshot(false);
    value.settings.environmentSelection = { one: 'missing' };
    value.environments = [{ id: 'local', label: 'Local', machine: 'linux', status: 'offline', access: 'direct', isLocal: true, canForgetAccess: false }];
    await renderSnapshot(value);
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Settings"]')!.click());
    const group = host.querySelector('[aria-label="Environment"]');
    const selected = group?.querySelector<HTMLInputElement>('input:checked');
    expect(selected?.value).toBe('missing');
    expect(selected?.parentElement?.textContent).toBe('Selected environment unavailable');
    expect(group?.textContent).toContain('Local · offline');
    expect(tauri.invoke).not.toHaveBeenCalledWith('set_environment_selection', expect.anything());
  });
});
