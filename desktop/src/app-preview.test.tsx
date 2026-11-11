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
  now: '2026-11-06T09:08:00Z',
  threads: [{
    id: 'thread-1', projectId: 'project-1', projectTitle: 'Fixture', title: 'Review thread', status: 'Settled',
    isPinned: false, providers: ['codex'], branch: null, modelName: null, prNumber: null,
    updatedAt: '2026-11-06T09:07:00Z', latestUserMessageAt: '2026-11-06T09:07:00Z', latestRunCompletedAt: null,
    activeRunStartedAt: null, usageLimitResetAt: null, projectIcon: null, projectFavicon: null,
  }],
  online: true, threadError: null, loading: false,
  connection: { status: 'connected', message: 'Connected', connected: true, paired: true, hasData: true, port: 0, updatedAt: null },
  usage: null, usageError: null, usageLoading: false, usagePeriod: '24h', limits: null,
  settings: { notificationsMuted: false, showsPreviews, automaticallyChecksForUpdates: false, installsWithoutAsking: false },
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
    expect(tauri.invoke).not.toHaveBeenCalledWith('get_preview', { id: 'thread-1' });
    act(() => vi.advanceTimersByTime(1));
    await act(async () => { await Promise.resolve(); });
    expect(tauri.invoke).toHaveBeenCalledWith('get_preview', { id: 'thread-1' });
    expect(host.querySelector('[role="tooltip"]')?.textContent).toContain(preview.text);
  });

  it('cancels a pending hover fetch when the pointer leaves', async () => {
    await renderSnapshot(snapshot(true));
    const row = threadRow();
    act(() => row.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => row.dispatchEvent(new MouseEvent('mouseout', { bubbles: true, relatedTarget: document.body })));
    act(() => vi.advanceTimersByTime(500));
    expect(tauri.invoke).not.toHaveBeenCalledWith('get_preview', { id: 'thread-1' });
  });

  it('ignores a delayed preview result after the panel reopens', async () => {
    await renderSnapshot(snapshot(true));
    const row = threadRow();
    let resolvePreview!: (value: typeof preview) => void;
    tauri.invoke.mockImplementation((name: string) => name === 'get_preview' ? new Promise(resolve => { resolvePreview = resolve; }) : Promise.resolve(null));

    act(() => row.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => vi.advanceTimersByTime(500));
    expect(tauri.invoke).toHaveBeenCalledWith('get_preview', { id: 'thread-1' });
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

  it('does not fetch previews when the setting is disabled', async () => {
    await renderSnapshot(snapshot(false));
    act(() => threadRow().dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => vi.advanceTimersByTime(500));
    expect(tauri.invoke).not.toHaveBeenCalledWith('get_preview', { id: 'thread-1' });
  });

  it('cancels a pending preview when the setting is disabled while hovering', async () => {
    await renderSnapshot(snapshot(true));
    act(() => threadRow().dispatchEvent(new MouseEvent('mouseover', { bubbles: true })));
    act(() => tauri.listeners.get('bar-snapshot')?.({ payload: snapshot(false) }));
    act(() => vi.advanceTimersByTime(500));
    expect(tauri.invoke).not.toHaveBeenCalledWith('get_preview', { id: 'thread-1' });
    expect(host.querySelector('[role="tooltip"]')).toBeNull();
  });
});

describe('new-install pairing', () => {
  it('requires a pairing key even when T3 Code already has cached data', async () => {
    const value = snapshot(true);
    value.connection = { ...value.connection, connected: false, paired: false, status: 'cacheOnly' };
    await renderSnapshot(value);
    expect(host.querySelector('.onboarding')).not.toBeNull();
    expect(host.querySelector('.thread-row')).toBeNull();
    expect(host.querySelector('input[type="password"]')?.getAttribute('placeholder')).toBe('Pairing key');
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
    await renderSnapshot(value);
    expect(host.querySelector('.onboarding')).toBeNull();
    expect(host.querySelector('.thread-row')).not.toBeNull();
    expect(host.textContent).toContain('T3 Code offline');
  });

});

describe('settings state and actions', () => {
  async function openSettings(value = snapshot(true)) {
    await renderSnapshot(value);
    act(() => tauri.listeners.get('bar-settings')?.());
  }
  function button(label: string) {
    const found = [...host.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label);
    expect(found).toBeDefined();
    return found!;
  }
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
