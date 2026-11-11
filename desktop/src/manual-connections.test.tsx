// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ManualConnections } from './ManualConnections';
import type { ConnectionCandidate, EnvironmentView } from './contracts';

const invoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
let root: Root;
let host: HTMLDivElement;
const candidate: ConnectionCandidate = { environmentId: 'remote', label: 'Remote Linux', hasPairingKey: false, destination: 'https://remote.example/' };
const environment: EnvironmentView = { id: 'remote', label: 'Remote Linux', isLocal: false, status: 'online', access: 'direct', machine: 'linux', canForgetAccess: true };

beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  invoke.mockReset();
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<ManualConnections environments={[environment]} savedIds={['remote']} />));
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.unstubAllGlobals(); });

async function click(text: string) {
  const button = [...host.querySelectorAll('button')].find(item => item.textContent === text);
  expect(button).toBeDefined();
  await act(async () => button!.click());
}
function fill(label: string, value: string) {
  const input = [...host.querySelectorAll('label')].find(item => item.textContent === label)?.querySelector('input');
  expect(input).toBeDefined();
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value);
    input!.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

it('checks server identity before pairing, clears a rejected key, and retries without losing the form', async () => {
  invoke.mockResolvedValue(candidate);
  await click('Add environment');
  fill('Server address or pairing link', 'https://remote.example');
  await click('Check server');
  expect(invoke).toHaveBeenLastCalledWith('probe_environment', { input: { kind: 'direct', address: 'https://remote.example' } });
  expect(host.textContent).toContain('Remote Linux');
  expect(host.textContent).toContain('https://remote.example/');
  fill('Remote Linux pairing key', 'ABCD2345EFGH');
  invoke.mockRejectedValueOnce('The pairing key was not accepted.');
  await click('Connect environment');
  expect(host.querySelector<HTMLInputElement>('input[type="password"]')?.value).toBe('');
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('not accepted');
  fill('Remote Linux pairing key', 'JKLM2345NPQR');
  invoke.mockResolvedValueOnce(null);
  await click('Connect environment');
  expect(invoke).toHaveBeenLastCalledWith('add_environment', {
    input: { kind: 'direct', address: 'https://remote.example' }, environment: 'remote', token: 'JKLM2345NPQR',
  });
  expect(host.querySelector('form')).toBeNull();
});

it('accepts a pasted pairing link without requiring a second key field', async () => {
  invoke.mockResolvedValue({ ...candidate, hasPairingKey: true });
  await click('Add environment');
  fill('Server address or pairing link', 'https://remote.example/pair#token=ABCD2345EFGH');
  await click('Check server');
  expect(host.textContent).toContain('Pairing key included');
  expect(host.querySelector('input[type="password"]')).toBeNull();
  await click('Connect environment');
  expect(invoke).toHaveBeenLastCalledWith('add_environment', {
    input: { kind: 'direct', address: 'https://remote.example/pair#token=ABCD2345EFGH' }, environment: 'remote', token: '',
  });
});

it('validates an SSH port and probes with the configured host and user', async () => {
  invoke.mockResolvedValue(candidate);
  await click('Add environment');
  const select = host.querySelector('select')!;
  act(() => { select.value = 'ssh'; select.dispatchEvent(new Event('change', { bubbles: true })); });
  fill('SSH host or alias', 'buildbox');
  fill('SSH user (optional)', 'developer');
  fill('SSH port (optional)', '0');
  await click('Check server');
  expect(invoke).not.toHaveBeenCalled();
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('between 1 and 65535');
  fill('SSH port (optional)', '2222');
  await click('Check server');
  expect(invoke).toHaveBeenLastCalledWith('probe_environment', {
    input: { kind: 'ssh', hostname: 'buildbox', username: 'developer', port: 2222 },
  });
});

it('ignores a delayed probe after cancellation and reopening', async () => {
  let resolve!: (value: ConnectionCandidate) => void;
  invoke.mockReturnValue(new Promise<ConnectionCandidate>(done => { resolve = done; }));
  await click('Add environment');
  fill('Server address or pairing link', 'https://old.example');
  await click('Check server');
  await click('Cancel');
  await click('Add environment');
  await act(async () => resolve(candidate));
  expect(host.textContent).not.toContain('Connect to');
  expect(host.querySelector('input[type="password"]')).toBeNull();
});

it('requires confirmation and removes only the selected Bar connection', async () => {
  invoke.mockResolvedValue(null);
  await click('Remove saved connections for Remote Linux…');
  expect(invoke).not.toHaveBeenCalled();
  await click('Cancel');
  expect(invoke).not.toHaveBeenCalled();
  await click('Remove saved connections for Remote Linux…');
  await click('Remove saved connections');
  expect(invoke).toHaveBeenCalledExactlyOnceWith('remove_saved_environment', { environment: 'remote' });
});
