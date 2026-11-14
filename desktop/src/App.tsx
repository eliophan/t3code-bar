import { chartModel, nearestColumn } from './chart';
import { resolveProvider, uniqueProviders, statusIcons } from './providers';
import { limitAgo, limitReset, limitsStaleness } from './limits-format';
import { ProjectBadge } from './ProjectBadge';
import { EnvironmentMachineIcon } from './EnvironmentMachineIcon';
import { MiddleText } from './middle-text';
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ManualConnections } from './ManualConnections';
import { listen } from '@tauri-apps/api/event';
import type { AppSnapshot, ConnectionState, LimitGroup, Metric, Period, Preview, Screen, Settings, ThreadItem, UsageReport } from './contracts';
import { compact, currency, duration, providerStyles, relative, until } from './format';
import { costProviderDetail, footnotes, modelRows, sessions, slices, tokenProviderDetail, unpricedNote, usageProviderIcon, usageProviderLabel, visibleModels, modelToggleTitle, type Slice } from './usage-presentation';

function ProviderIcon({ kind }: { kind: string }) {
  const resolved = resolveProvider(kind);
  return resolved === 'unknown' ? <span className="provider-letter" title={kind}>{kind.slice(0, 2).toUpperCase()}</span> : <img className="provider-icon" src={`/icons/${resolved}.${resolved === 'antigravity' ? 'png' : 'svg'}`} alt={kind} />;
}
function providerRunLabel(providers: string[], modelName: string | null): string {
  const model = modelName?.trim();
  if (model) return `Run on ${model}`;
  const names = uniqueProviders(providers).join(', ');
  return names ? `Run on ${names}` : 'Run on OpenAI';
}

function ProviderIcons({ providers, modelName }: { providers: string[]; modelName: string | null }) {
  const label = providerRunLabel(providers, modelName);
  return <span className="providers" title={label} aria-label={label}>{uniqueProviders(providers).map(kind => <ProviderIcon key={kind} kind={kind === 'unknown' ? providers.find(p => resolveProvider(p) === 'unknown') ?? '?' : kind} />)}</span>;
}

function Notice({ children, warning = false }: { children: ReactNode; warning?: boolean }) {
  return <p className={`notice ${warning ? 'warning' : ''}`} role="status">{children}</p>;
}
function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return <div className="empty"><strong>{title}</strong>{children && <p>{children}</p>}</div>;
}

function Chevron({ right = false }: { right?: boolean }) {
  return <svg className={`chevron-icon${right ? ' right' : ''}`} viewBox="0 0 12 12" aria-hidden="true"><path d="m7.5 2-4 4 4 4" /></svg>;
}

export function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [screen, setScreen] = useState<Screen>('threads');
  const [connectionSettings, setConnectionSettings] = useState(false);
  const [metric, setMetric] = useState<Metric>('cost');
  const [period, setPeriod] = useState<Period>('24h');
  const [threadGeneration, setThreadGeneration] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [context, setContext] = useState<{ thread: ThreadItem } | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const act = useCallback(async (name: string, args?: Record<string, unknown>) => {
    try { setError(null); await invoke(name, args); }
    catch (failure) { setError(String(failure)); }
  }, []);

  useEffect(() => {
    let disposed = false;
    const initial = invoke<AppSnapshot>('get_snapshot').then(s => { if (!disposed) { setSnapshot(s); if (s.startupError) setError(s.startupError); } }).catch(e => { if (!disposed) setError(String(e)); });
    const events = listen<AppSnapshot>('bar-snapshot', e => { if (!disposed) setSnapshot(e.payload); });
    const opened = listen('bar-opened', () => { setScreen(current => current === 'settings' ? current : 'threads'); setContext(null); setThreadGeneration(generation => generation + 1); });
    const settings = listen('bar-settings', () => { setConnectionSettings(false); setScreen('settings'); });
    void initial;
    return () => { disposed = true; void events.then(stop => stop()); void opened.then(stop => stop()); void settings.then(stop => stop()); };
  }, []);
  useEffect(() => { void act('set_view', { screen, metric, period }); }, [screen, metric, period, act]);
  useEffect(() => { if (screen !== 'settings') setConnectionSettings(false); }, [screen]);
  useEffect(() => {
    const update = snapshot?.update;
    if (screen === 'settings' && !connectionSettings && update?.canInstall && update.offerId != null && update.version) {
      void invoke('update_prompt_shown', { offerId: update.offerId, version: update.version });
    }
  }, [screen, connectionSettings, snapshot?.update.offerId, snapshot?.update.version, snapshot?.update.canInstall]);
  useEffect(() => {
    if (!root.current) return;
    let timer: ReturnType<typeof setTimeout>;
    const observer = new ResizeObserver(entries => {
      clearTimeout(timer);
      timer = setTimeout(() => { void invoke('resize_panel', { height: Math.ceil(entries[0].contentRect.height) }).catch(() => {}); }, 40);
    });
    observer.observe(root.current);
    return () => { observer.disconnect(); clearTimeout(timer); };
  }, []);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      if (context) setContext(null);
      else if (screen === 'settings' && connectionSettings) setConnectionSettings(false);
      else if (screen !== 'threads') setScreen('threads');
      else void act('hide_panel');
    };
    window.addEventListener('keydown', key);
    return () => window.removeEventListener('keydown', key);
  }, [screen, connectionSettings, context, act]);

  const hasThreadAccess = snapshot !== null && (snapshot.localConnection.paired || snapshot.connection.paired || snapshot.environments.some(environment =>
    !environment.isLocal && (environment.status === 'online' || snapshot.threads.some(thread => thread.thread.environmentId === environment.id))));

  return <div ref={root} className="app" onClick={() => setContext(null)}>
    {error && <div className="error-banner" role="alert">{error}<button aria-label="Dismiss error" onClick={() => setError(null)}>×</button></div>}
    {!snapshot ? <Empty title={error ? 'Could not load T3 Code Bar' : 'Reading T3 Code data…'}><button onClick={() => { void invoke<AppSnapshot>('get_snapshot').then(setSnapshot).catch(e => setError(String(e))); }}>Retry</button></Empty>
    : screen === 'settings' ? <><header><button aria-label={connectionSettings ? 'Back to settings' : 'Back to threads'} onClick={() => connectionSettings ? setConnectionSettings(false) : setScreen('threads')}><Chevron /></button><strong>{connectionSettings ? 'Local connection' : 'Settings'}</strong></header>{connectionSettings ? <LocalConnectionSettings snapshot={snapshot} act={act} /> : <SettingsPage snapshot={snapshot} act={act} onManageConnection={() => setConnectionSettings(true)} />}</>
    : !hasThreadAccess ? <div className="onboarding"><h1>Connect this computer</h1><Notice>Pair with T3 Code on this computer to see your threads, replies and usage. Remote environments and T3 Connect sign-in are optional in Settings.</Notice><Connection snapshot={snapshot} act={act} local /><button className="subtle" onClick={() => setScreen('settings')}>Settings</button></div>
    : screen === 'threads' ? <>
      <EnvironmentPicker snapshot={snapshot} act={act} />
      <main key={threadGeneration} className="threads scroll">
        {snapshot.threadError && <Notice warning>{snapshot.threadError}</Notice>}
        {!snapshot.online && <Notice>T3 Code offline · showing saved threads</Notice>}

        {snapshot.loading ? <Empty title="Reading T3 Code data…" /> : snapshot.threads.length === 0 ? <Empty title="Your inbox is clear">No active threads. <button className="text-button" onClick={() => void act('new_thread')}>New thread</button></Empty> : snapshot.threads.map(thread => <ThreadRow key={JSON.stringify(thread.thread)} thread={thread} environment={snapshot.environments.find(environment => environment.id === thread.thread.environmentId)} now={Date.parse(snapshot.now)} online={snapshot.environments.find(environment => environment.id === thread.thread.environmentId)?.status === 'online'} revision={snapshot.revision} previews={snapshot.settings.showsPreviews} onOpen={() => void act('thread_action', { thread: thread.thread, action: 'open' })} onContext={() => setContext({ thread })} />)}
      </main><footer aria-label="Bar navigation"><button className="usage-link" aria-label="Usage" title="Usage" onClick={() => setScreen('usage')}><svg className="usage-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 21v-6M12 21V3M19 21V9" /></svg></button><button className="settings-button" aria-label="Settings" title="Settings" onClick={() => setScreen('settings')}><svg className="settings-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M9.67 4.14a2 2 0 0 1 4.66 0 2 2 0 0 0 2.88 1.66 2 2 0 0 1 2.33 4.04 2 2 0 0 0 0 3.32 2 2 0 0 1-2.33 4.04 2 2 0 0 0-2.88 1.66 2 2 0 0 1-4.66 0 2 2 0 0 0-2.88-1.66 2 2 0 0 1-2.33-4.04 2 2 0 0 0 0-3.32A2 2 0 0 1 6.79 5.8a2 2 0 0 0 2.88-1.66"/><circle cx="12" cy="12" r="3" /></svg></button></footer>
    </> : <>
      {!snapshot.connection.connected && <div className="disconnected-banner" title={snapshot.connection.message}><svg className="disconnected-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M1 9a16 16 0 0 1 22 0M5 13a10 10 0 0 1 14 0M9 17a4 4 0 0 1 6 0M3 3l18 18" /></svg><span>{['failed', 'restoring', 'credentialUnavailable'].includes(snapshot.connection.status) ? snapshot.connection.message : snapshot.connection.updatedAt ? `Disconnected · updated ${limitAgo(snapshot.connection.updatedAt, Date.parse(snapshot.now))}` : 'Disconnected · showing saved data'}</span></div>}
      <header><button aria-label="Back to threads" onClick={() => setScreen('threads')}><Chevron /></button><strong>Usage</strong>{metric !== 'limits' && <div className="segments periods" aria-label="Usage period">{(['24h', '7d', '30d', '90d'] as Period[]).map(p => <button key={p} aria-pressed={period === p} onClick={() => setPeriod(p)}>{p}</button>)}</div>}</header>
      <EnvironmentPicker snapshot={snapshot} act={act} />
      <div className="tab-bar"><nav className="segments tabs" aria-label="Usage metric">{(['cost', 'tokens', 'limits'] as Metric[]).map(m => <button key={m} aria-pressed={metric === m} onClick={() => setMetric(m)}>{m[0].toUpperCase() + m.slice(1)}</button>)}</nav></div>
      <main className="usage scroll">{metric === 'limits' ? <><Connection snapshot={snapshot} act={act} compact />{snapshot.limits ? <Limits groups={snapshot.limits.groups} snapshot={snapshot} act={act} /> : <Empty title="No limits data yet">Open T3 Code once so it can save provider status.</Empty>}</> : <>
        {snapshot.usageError && <Notice warning>{snapshot.usageError}</Notice>}
        {!snapshot.usage && snapshot.usageLoading ? <Empty title="Reading usage…" /> : !snapshot.usage ? <Empty title="No usage data yet">Open T3 Code’s Usage page to populate its saved scan cache.</Empty> : <div className={snapshot.usagePeriod !== period ? 'stale' : ''}><Usage key={metric} report={snapshot.usage} metric={metric} /></div>}
      </>}</main>
    </>}
    {context && <div role="menu" className="context-menu" onClick={e => e.stopPropagation()}>{[['open', 'Open in T3 Code'], ['browser', 'Open in Browser'], ['copy', 'Copy Thread Link']].map(([action, title]) => <button role="menuitem" key={action} onClick={() => { void act('thread_action', { thread: context.thread.thread, action }); setContext(null); }}>{title}</button>)}</div>}
  </div>;
}

function EnvironmentPicker({ snapshot, act }: { snapshot: AppSnapshot; act: Act }) {
  if (snapshot.environments.length < 2) return snapshot.environments.length === 0 ? <div className="environment-picker"><span>No environments found.</span><button className="text-button" onClick={() => void act('login_cloud')}>Sign in to T3 Code</button></div> : null;
  const selectedId = snapshot.settings.environmentSelection === 'all' ? '' : snapshot.settings.environmentSelection.one;
  return <div className="environment-picker" aria-label="Environment selector">
    <label htmlFor="environment-select">Environment</label>
    <select id="environment-select" value={selectedId} onChange={event => void act('set_environment_selection', { selection: event.target.value ? { one: event.target.value } : 'all' })}>
      <option value="">All environments</option>
      {snapshot.environments.map(environment => <option key={environment.id} value={environment.id}>{environment.label} · {environment.machine} · {environment.status}</option>)}
    </select>
  </div>;
}

type Act = (name: string, args?: Record<string, unknown>) => Promise<void>;
function SettingsPage({ snapshot, act, onManageConnection }: { snapshot: AppSnapshot; act: Act; onManageConnection: () => void }) {
  const [pending, setPending] = useState(false);
  const unavailable = snapshot.update.status === 'unavailable';
  const updateBusy = snapshot.update.busy;
  const change = async (key: Exclude<keyof Settings, 'environmentSelection'>, value: boolean) => {
    setPending(true);
    try { await act('set_setting', { key, value }); } finally { setPending(false); }
  };
  const toggle = (label: string, key: Exclude<keyof Settings, 'environmentSelection'>, checked: boolean, disabled = false, invert = false) => <label className="toggle"><span>{label}</span><input type="checkbox" role="switch" checked={checked} disabled={disabled || pending} onChange={e => void change(key, invert ? !e.target.checked : e.target.checked)} /></label>;
  return <main className="settings scroll">
    <section><h2>Local T3 Code connection</h2><Notice>{snapshot.localConnection.connected ? 'Connected to T3 Code' : snapshot.localConnection.message}</Notice><button onClick={onManageConnection}>Manage connection</button>
    </section>
    <section><h2>Environment</h2><EnvironmentSelectionOptions snapshot={snapshot} act={act} /><ManualConnections environments={snapshot.environments} savedIds={snapshot.manualEnvironmentIds} />
      <Notice>Optional: import saved environments instead of adding their addresses manually. This may ask to read T3 Code’s encrypted connection list using its Keychain or Secret Service entry. Importing only discovers environments; each still needs its own Bar pairing.</Notice>
      {toggle('Import environments from T3 Code', 'importsDesktopEnvironments', snapshot.settings.importsDesktopEnvironments)}
      <Notice>Turning import off stops discovery from that list. Your saved Bar pairing credentials are not deleted. Local pairing, manually added environments and T3 Connect work without this import.</Notice>
      {snapshot.environments.filter(environment => !environment.isLocal).map(environment => <EnvironmentAccess key={environment.id} environment={environment} cloudStatus={snapshot.cloudStatus} act={act} />)}
    </section>
    <section><h2>T3 Connect</h2><Notice>Sign in to find linked environments. Pair once with each environment to read through its public HTTPS tunnel. Tailscale is not required.</Notice>{snapshot.cloudError && <Notice>{snapshot.cloudError}</Notice>}{snapshot.cloudStatus === 'signedOut' && <button onClick={() => void act('begin_cloud_login')}>Sign in to T3 Connect</button>}{snapshot.cloudStatus === 'signingIn' && <button onClick={() => void act('cancel_cloud_login')}>Cancel</button>}{snapshot.cloudStatus === 'signedIn' && <button onClick={() => void act('disconnect_cloud')}>Sign out</button>}{snapshot.cloudStatus === 'recoveryRequired' && <><Notice>Saved T3 Connect credentials need recovery. Restore access to your system keyring, then retry sign out. Some saved credentials may remain until cleanup succeeds.</Notice><button onClick={() => void act('disconnect_cloud')}>Retry sign out</button></>}{snapshot.cloudStatus === 'unavailable' && <Notice>T3 Connect unavailable</Notice>}</section>
    <section><h2>Preferences</h2>
      {toggle('Notifications', 'notificationsMuted', !snapshot.settings.notificationsMuted, false, true)}
      {toggle('Hover previews', 'showsPreviews', snapshot.settings.showsPreviews)}
    </section>
    <section><h2>Updates</h2>
      {toggle('Check automatically', 'automaticallyChecksForUpdates', snapshot.settings.automaticallyChecksForUpdates, unavailable || updateBusy)}
      {toggle('Install without asking', 'installsWithoutAsking', snapshot.settings.installsWithoutAsking, unavailable || updateBusy || !snapshot.settings.automaticallyChecksForUpdates)}
      <Notice>{unavailable ? 'Updates unavailable in this build' : snapshot.update.message}</Notice>
      <button disabled={unavailable || updateBusy} onClick={() => void act(snapshot.update.canInstall ? 'install_update' : 'check_for_updates', snapshot.update.canInstall ? { offerId: snapshot.update.offerId, version: snapshot.update.version } : undefined)}>{updateBusy ? (snapshot.update.status === 'checking' ? 'Checking…' : 'Installing…') : snapshot.update.canInstall ? `Install ${snapshot.update.version} and restart` : 'Check for Updates…'}</button>
    </section>
    <div className="settings-bottom"><button onClick={() => void act('quit_app')}>Quit T3 Code Bar</button><span className="version">v{snapshot.version}</span></div>
  </main>;
}

function LocalConnectionSettings({ snapshot, act }: { snapshot: AppSnapshot; act: Act }) {
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);
  const [pending, setPending] = useState(false);
  return <main className="settings scroll"><section><Connection snapshot={snapshot} act={act} local />
    {snapshot.localConnection.paired && (confirmDisconnect ? <div className="disconnect-confirmation" role="group" aria-label="Confirm disconnect"><p>Disconnect and remove the saved pairing key? You will need a new key to connect again.</p><div className="settings-actions"><button autoFocus disabled={pending} onClick={() => setConfirmDisconnect(false)}>Cancel</button><button className="danger-button" disabled={pending} onClick={async () => { setPending(true); try { await act('disconnect'); setConfirmDisconnect(false); } finally { setPending(false); } }}>{pending ? 'Disconnecting…' : 'Disconnect and remove key'}</button></div></div> : <button className="secondary-button" onClick={() => setConfirmDisconnect(true)}>Disconnect…</button>)}
  </section></main>;
}

function EnvironmentSelectionOptions({ snapshot, act }: { snapshot: AppSnapshot; act: Act }) {
  const [pending, setPending] = useState(false);
  const selectedId = snapshot.settings.environmentSelection === 'all' ? '' : snapshot.settings.environmentSelection.one;
  const options = [
    { id: '', label: 'All environments' },
    ...snapshot.environments.map(environment => ({ id: environment.id, label: `${environment.label} · ${environment.status}` })),
    ...(selectedId && !snapshot.environments.some(environment => environment.id === selectedId) ? [{ id: selectedId, label: 'Selected environment unavailable' }] : []),
  ];
  return <fieldset className="environment-options" aria-label="Environment"><legend>Show</legend>{options.map(option => <label key={option.id}>
    <input type="radio" name="environmentSelection" value={option.id} checked={selectedId === option.id} disabled={pending} onChange={async () => {
      setPending(true);
      try { await act('set_environment_selection', { selection: option.id ? { one: option.id } : 'all' }); } finally { setPending(false); }
    }} /><span>{option.label}</span>
  </label>)}</fieldset>;
}

type EnvironmentViewModel = import('./contracts').EnvironmentView;

function EnvironmentAccess({ environment, cloudStatus, act }: { environment: EnvironmentViewModel; cloudStatus: AppSnapshot['cloudStatus']; act: Act }) {
  const needsPrompt = environment.status === 'needsAccess' || environment.status === 'permissionDenied'
    || (environment.status === 'unsupported' && (environment.access === 'cloud' || environment.access === 'directOrCloud'));
  if (environment.status === 'restoring') return <Notice>Waiting for the saved Local pairing. Complete any Keychain prompt for T3 Code Bar.</Notice>;
  if (environment.status === 'credentialUnavailable') return <div className="environment-access"><Notice warning>Saved access for {environment.label} is unavailable. Unlock your system credential store or allow T3 Code Bar in its Keychain prompt. Pairing has not been removed.</Notice>{!environment.isLocal && <button onClick={() => void act('retry_environment_access')}>Retry saved access for {environment.label}</button>}</div>;
  if (!needsPrompt && !environment.canForgetAccess) return null;
  return <div className="environment-access">
    {needsPrompt && <EnvironmentAccessPrompt environment={environment} cloudStatus={cloudStatus} act={act} />}
    {environment.canForgetAccess && <EnvironmentForget environment={environment} act={act} />}
  </div>;
}

function EnvironmentAccessPrompt({ environment, cloudStatus, act }: { environment: EnvironmentViewModel; cloudStatus: AppSnapshot['cloudStatus']; act: Act }) {
  const denied = environment.status === 'permissionDenied';
  const cloudHint = cloudStatus === 'signedOut' ? 'Sign in below to find its HTTPS address, then pair with this environment.' : cloudStatus === 'signingIn' ? 'Finish signing in or cancel below.' : 'Waiting for its HTTPS address. If unavailable, use Add environment with a server address or SSH host.';
  const denial = (route: string) => denied && <Notice warning>Bar could not authorize access to {environment.label}. {route}</Notice>;
  switch (environment.access) {
    case 'direct': return <>
      {denial('Enter a pairing key from this environment.')}
      <EnvironmentPairing environment={environment} act={act} />
    </>;
    case 'directOrCloud': return <>
      {denial('Enter a new pairing key from this environment.')}
      <EnvironmentPairing environment={environment} act={act} />
      <Notice>Sign in to T3 Connect to find a public HTTPS address for {environment.label}, then pair with that address.</Notice>
    </>;
    case 'cloud': return environment.status === 'unsupported'
      ? <Notice warning>T3 Connect has not provided a pairing address for {environment.label}. Use Add environment in Bar with a reachable HTTPS address or SSH host.</Notice>
      : denied
      ? <Notice warning>T3 Connect could not authorize {environment.label}. {cloudHint}</Notice>
      : <Notice>{environment.label}: {cloudHint}</Notice>;
    case 'unsupported': return null;
    default: { const _exhaustive: never = environment.access; return _exhaustive; }
  }
}

function EnvironmentForget({ environment, act }: { environment: EnvironmentViewModel; act: Act }) {
  const [confirming, setConfirming] = useState(false);
  const [pending, setPending] = useState(false);
  if (!confirming) return <button className="secondary-button" onClick={() => setConfirming(true)}>Forget access for {environment.label}…</button>;
  return <div className="disconnect-confirmation" role="group" aria-label={`Confirm forgetting access for ${environment.label}`}>
    <p>Remove the access Bar saved for {environment.label}? You will need a new pairing key to connect again. Other environments and T3 Connect are not affected.</p>
    <div className="settings-actions">
      <button autoFocus disabled={pending} onClick={() => setConfirming(false)}>Cancel</button>
      <button className="danger-button" disabled={pending} onClick={async () => {
        setPending(true);
        try { await act('forget_environment_access', { environment: environment.id }); setConfirming(false); } finally { setPending(false); }
      }}>{pending ? 'Removing…' : 'Forget access'}</button>
    </div>
  </div>;
}

function EnvironmentPairing({ environment, act }: { environment: EnvironmentViewModel; act: Act }) {
  const [token, setToken] = useState('');
  const [pending, setPending] = useState(false);
  return <form onSubmit={async event => {
    event.preventDefault();
    const input = token;
    setToken('');
    setPending(true);
    try { await act('pair_environment', { environment: environment.id, token: input }); } finally { setPending(false); }
  }}><label>{environment.label} pairing key <input type="password" value={token} autoComplete="off" maxLength={16384} onChange={event => setToken(event.target.value)} /></label><button disabled={pending || !token.trim()}>{pending ? 'Connecting…' : 'Connect'}</button></form>;
}

function Connection({ snapshot, act, compact: minimal = false, local = false }: { snapshot: AppSnapshot; act: Act; compact?: boolean; local?: boolean }) {
  const connection: ConnectionState = local ? snapshot.localConnection : snapshot.connection;
  const [token, setToken] = useState('');
  const [expanded, setExpanded] = useState(!minimal && !connection.paired);
  const [pending, setPending] = useState(false);
  const restoring = connection.status === 'restoring';
  const storageUnavailable = connection.status === 'credentialUnavailable';
  const busy = pending || restoring || ['connecting', 'retrying'].includes(connection.status);
  useEffect(() => { if (connection.connected) { setToken(''); setExpanded(false); } }, [connection.connected]);
  useEffect(() => { if (!connection.paired && !minimal) setExpanded(true); }, [connection.paired, minimal]);
  const connect = async (name: 'connect' | 'reconnect' | 'connect_from_clipboard') => {
    const input = token;
    setToken('');
    setPending(true);
    try { await act(name, name === 'connect' ? { token: input } : undefined); } finally { setPending(false); }
  };
  if (minimal && connection.connected) return null;
  return <div className="connection">
    <p className="connection-state" role="status"><i className={connection.connected ? 'connected' : busy ? 'connecting' : ''} aria-hidden="true" />{connection.connected ? 'Connected to T3 Code' : connection.message}</p>
    {!connection.connected && <>
      {storageUnavailable && <button disabled={pending} onClick={() => void connect('reconnect')}>Retry saved access</button>}
      {!storageUnavailable && !restoring && connection.paired && <div className="settings-actions"><button disabled={busy} onClick={() => void connect('reconnect')}>{busy ? 'Reconnecting…' : 'Reconnect'}</button><button className="text-button" disabled={busy} onClick={() => { setExpanded(!expanded); setToken(''); }}>{expanded ? 'Cancel' : 'Pair again…'}</button></div>}
      {!storageUnavailable && !restoring && !connection.paired && minimal && <button className="text-button" onClick={() => { setExpanded(!expanded); setToken(''); }}>{expanded ? 'Hide connection' : 'Connect T3 Code…'}</button>}
      {!storageUnavailable && !restoring && expanded && <form onSubmit={e => { e.preventDefault(); if (!busy && (local || token.trim())) void connect(token.trim() ? 'connect' : 'connect_from_clipboard'); }}>
        {local && <div id="local-pairing-help" className="pairing-help"><ol><li>In T3 Code, open <strong>Settings → Sharing &amp; access</strong>.</li><li>Under <strong>Authorized clients</strong>, choose <strong>Create link</strong> with read-only permissions.</li><li>Choose <strong>Copy link</strong>, then <strong>Paste and connect</strong> here.</li></ol><p>Or paste a link or use <strong>Copy code only</strong> below.</p><p>Clipboard access happens only on click. Bar saves its own access for reconnects.</p></div>}
        <label className="sr-only" htmlFor="access-token">Local pairing link or code</label><input id="access-token" type="password" value={token} disabled={busy} aria-describedby={local ? 'local-pairing-help' : undefined} autoComplete="off" spellCheck={false} placeholder="Paste pairing link or code" maxLength={16384} onChange={e => setToken(e.target.value)} />
        <div className="connection-actions"><button className="primary-button" type="submit" disabled={busy || (!local && !token.trim())}>{busy ? 'Connecting…' : local && !token.trim() ? 'Paste and connect' : 'Connect'}</button></div>
      </form>}
    </>}
  </div>;
}

function ThreadRow({ thread, environment, now, online, previews, revision, onOpen, onContext }: { thread: ThreadItem; environment: import('./contracts').EnvironmentView | undefined; now: number; online: boolean; previews: boolean; revision: number; onOpen: () => void; onContext: () => void }) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const generation = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const cancel = useCallback(() => { generation.current++; clearTimeout(timer.current); setPreview(null); }, []);
  useEffect(() => { cancel(); return () => { generation.current++; clearTimeout(timer.current); }; }, [previews, revision, cancel]);
  const hover = () => {
    if (!previews) return;
    const run = ++generation.current;
    clearTimeout(timer.current);
    timer.current = setTimeout(() => { void invoke<Preview | null>('get_preview', { thread: thread.thread, revision }).then(p => { if (run === generation.current) setPreview(p); }).catch(() => {}); }, 500);
  };
  const status = online ? thread.status : 'Settled';
  const suffix = status === 'Working' || status === 'Goal' ? duration(thread.activeRunStartedAt, now) : status === 'Limited' ? until(thread.usageLimitResetAt, now) : status === 'Waiting' ? duration(thread.latestRunCompletedAt, now) : null;
  const inFlight = ['Working', 'Goal', 'Approval', 'Input'].includes(status);
  const receded = inFlight || status === 'Settled';
  return <div className="thread-wrapper" data-in-flight={inFlight || undefined} data-receded={receded || undefined} data-status={status} onMouseEnter={hover} onMouseLeave={cancel}>
    <button className="thread-row" onClick={onOpen} onContextMenu={e => { e.preventDefault(); cancel(); onContext(); }} aria-label={`${thread.title}, in ${thread.projectTitle}, ${environment?.label ?? thread.thread.environmentId}${thread.branch ? `, on branch ${thread.branch}` : ''}${status !== 'Settled' ? `, ${status}` : ''}`}>
      <div className="project-line">{environment && !environment.isLocal && <EnvironmentMachineIcon kind={environment.machine} source={environment.label} />}<ProjectBadge title={thread.projectTitle} icon={thread.projectIcon} favicon={thread.projectFavicon} />{thread.isPinned && <svg className="pin" viewBox="0 0 24 24" aria-label="Pinned"><path fill="currentColor" d="M14 2 22 10 19 11 16 14 16 17 14 19 10 15 4 21 3 20 9 14 5 10 7 8 10 8 13 5Z" /></svg>}<span className="project-title">{thread.projectTitle}</span>{status === 'Settled' ? <time title="Last activity">{relative(thread.latestUserMessageAt ?? thread.updatedAt, now)}</time> : <span className={`status status-${status.toLowerCase()}`} title={status === 'Limited' ? `Usage limit reached${suffix ? `. Resets in ${suffix}` : ''}` : status === 'Done' && thread.latestRunCompletedAt ? `Done · ${new Date(thread.latestRunCompletedAt).toLocaleString()}` : status}><i style={{ maskImage: `url(/icons/Lucide-${statusIcons[status]}.svg)`, WebkitMaskImage: `url(/icons/Lucide-${statusIcons[status]}.svg)` }} />{status}{suffix && <small>{status === 'Limited' ? `· ${suffix}` : suffix}</small>}</span>}</div>
      <div className="title-line"><div className="thread-title">{thread.title}</div>{!thread.branch?.trim() && <ProviderIcons providers={thread.providers} modelName={thread.modelName} />}</div>
      {!!thread.branch?.trim() && <div className="branch-line"><span className="branch">{thread.branch}</span><ProviderIcons providers={thread.providers} modelName={thread.modelName} /></div>}
      {preview && <div className="preview" role="tooltip"><p>{preview.text}</p>{preview.updatedAt && <time>Last reply · {relative(preview.updatedAt, now) === 'now' ? 'just now' : `${relative(preview.updatedAt, now)} ago`}</time>}</div>}
    </button>
  </div>;
}

function Usage({ report, metric }: { report: UsageReport; metric: 'cost' | 'tokens' }) {
  const [showsAll, setShowsAll] = useState(false);
  if (!report.records) return <><Empty title="No usage in this period" /><UsageFootnotes report={report} /></>;
  const note = unpricedNote(report);
  const largest = Math.max(1, ...report.providers.map(p => p.processedTokens));
  const rows = modelRows(report, metric);
  const providers = <section className={`usage-providers ${metric}`}>
    {metric === 'tokens' && <h2>By provider</h2>}
    {report.providers.map(p => <div key={p.kind} className="usage-provider">
      <div className="usage-provider-heading">{metric === 'cost' && <i style={{ background: providerStyles[p.kind]?.color ?? '#8b8b8b' }} />}
        <ProviderIcon kind={usageProviderIcon(p.kind)} /><span className="usage-provider-name">{usageProviderLabel(p.kind)}</span>
        {metric === 'cost' && <small>{sessions(p.sessions)}</small>}
        <strong>{metric === 'cost' ? currency(p.costUsd) : compact(p.processedTokens)}</strong>
        {metric === 'tokens' && <small>· {tokenProviderDetail(p, report)}</small>}
      </div>
      {metric === 'cost' ? <p>{costProviderDetail(p, report)}</p> : <div className="usage-share"><span style={{ width: `${p.processedTokens / largest * 100}%` }} /></div>}
    </div>)}
  </section>;
  const totals = <section className="usage-totals"><h2>Totals</h2>{[['Processed tokens', compact(report.processedTokens)], ['Cached input', compact(report.tokens.cachedInput)], ['Uncached input', compact(report.tokens.uncachedInput)], ['Output', compact(report.tokens.output)], ['Cache savings', currency(report.cacheSavingsUsd)]].map(([label, value]) => <div key={label}><span>{label}</span><span>{value}</span></div>)}</section>;
  return <>
    <section className="hero">{metric === 'tokens' && <p>Processed tokens</p>}<h1>{metric === 'cost' ? currency(report.costUsd) : compact(report.processedTokens)}</h1><p>{sessions(report.sessions)}{metric === 'cost' && ' · API estimate'}{metric === 'cost' && note && <span title={note} aria-label={`Unpriced usage details: ${note}`} className="unpriced-info"><svg viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="4.75" /><path d="M6 5.3v3M6 3.5v.2" /></svg></span>}</p></section>
    {metric === 'cost' ? <>{providers}<Chart report={report} metric={metric} />{totals}<Breakdown title="Cost by type" parts={slices(report, 'cost')} /><Breakdown title="Cost by speed" parts={slices(report, 'speed')} aside={`Premium ${currency(report.costBySpeed.premium)}`} /></> : <><Breakdown title="Tokens by type" parts={slices(report, 'tokens')} />{totals}{providers}<Chart report={report} metric={metric} /></>}
    <section className="usage-models"><h2>Breakdown <small>Model</small></h2><div className="usage-model-list">{visibleModels(rows, showsAll).map(row => <div className="usage-model" key={`${row.kind}/${row.name}`}><span className="usage-model-rank">{row.rank}</span><div className="usage-model-content"><div className="usage-model-heading"><ProviderIcon kind={usageProviderIcon(row.kind)} /><MiddleText text={row.name} /><strong className={row.unpriced ? 'muted' : ''}>{row.costText}</strong></div><div className="usage-model-detail"><div>{row.barShare !== null && <span style={{ width: `${row.barShare * 100}%`, background: providerStyles[row.kind]?.color ?? '#8b8b8b' }} />}</div><span>{[row.shareText, row.tokensText].filter(Boolean).join(' · ')}</span></div></div></div>)}</div>{rows.length > 5 && <button className="usage-model-toggle" onClick={() => setShowsAll(!showsAll)}>{modelToggleTitle(rows.length, showsAll)}</button>}</section>
    <UsageFootnotes report={report} />
  </>;
}

function UsageFootnotes({ report }: { report: UsageReport }) {
  return <div className="usage-footnotes">{footnotes(report, Date.now()).map(note => <p key={note}>{note}</p>)}</div>;
}

function Chart({ report, metric }: { report: UsageReport; metric: 'cost' | 'tokens' }) {
  const model = chartModel(report, metric);
  const [hover, setHover] = useState<{ x: number; y: number; plotWidth: number } | null>(null);
  const hoverIndex = hover ? nearestColumn(hover.x / hover.plotWidth, model.columns.length) : null;
  const hoverColumn = hoverIndex === null ? null : model.columns[hoverIndex];
  const tooltip = useRef<HTMLDivElement>(null);
  const [tooltipSize, setTooltipSize] = useState({ width: 130, height: 80 });
  useLayoutEffect(() => { if (tooltip.current) setTooltipSize({ width: tooltip.current.offsetWidth, height: tooltip.current.offsetHeight }); }, [hoverIndex, report.providers.length]);
  const path = (curves: typeof model.series[number]['curves']) => curves.length ? `M ${curves[0].from.x} ${curves[0].from.y} ` + curves.map(c => `C ${c.control1.x} ${c.control1.y}, ${c.control2.x} ${c.control2.y}, ${c.to.x} ${c.to.y}`).join(' ') : '';
  const { width, height } = tooltipSize;
  return <section className="chart-section"><h2>{model.heading}</h2><div role="img" aria-label={`${model.heading} by provider, ${model.total} in total`}>
    <div className="chart-layout"><div className="chart-axis">{model.ticks.map((tick, i) => <span key={i} style={{ top: `${tick.y / 260 * 112 - 5.5}px` }}>{tick.label}</span>)}</div>
    <div className="chart-plot" onMouseMove={e => { const rect = e.currentTarget.getBoundingClientRect(); setHover({ x: e.clientX - rect.left, y: e.clientY - rect.top, plotWidth: rect.width }); }} onMouseLeave={() => setHover(null)}>
      <svg viewBox="0 0 960 260" preserveAspectRatio="none" aria-hidden="true">{model.ticks.map((tick, i) => <line key={i} x1="0" x2="960" y1={tick.y} y2={tick.y} stroke="#ffffff14" vectorEffect="non-scaling-stroke" />)}{model.series.map(s => { const line = path(s.curves), color = providerStyles[s.provider]?.color ?? '#8b8b8b'; return line && <g key={s.provider}><path d={`${line} L 960 260 L 0 260 Z`} fill={color} fillOpacity=".12" /><path d={line} fill="none" stroke={color} strokeWidth="2" strokeLinejoin="round" vectorEffect="non-scaling-stroke" /></g>; })}{hoverIndex !== null && <line x1={model.columns.length < 2 ? 0 : hoverIndex * 960 / (model.columns.length - 1)} x2={model.columns.length < 2 ? 0 : hoverIndex * 960 / (model.columns.length - 1)} y1="8" y2="260" stroke="#8b8b94" vectorEffect="non-scaling-stroke" />}</svg>
      {hover && hoverColumn && <div className="chart-tooltip" ref={tooltip} style={{ left: Math.max(0, Math.min(hover.x + 12 + width <= hover.plotWidth ? hover.x + 12 : hover.x - 12 - width, Math.max(0, hover.plotWidth - width))), top: Math.max(0, Math.min(hover.y + 12 + height <= 112 ? hover.y + 12 : hover.y - 12 - height, Math.max(0, 112 - height))) }}><strong>{hoverColumn.title}</strong>{hoverColumn.rows.map(row => <div key={row.provider}><span className="tooltip-provider"><ProviderIcon kind={usageProviderIcon(row.provider)} />{usageProviderLabel(row.provider)}</span><span>{row.text}</span></div>)}<div>Total<span>{hoverColumn.total}</span></div></div>}
    </div></div><div className="chart-labels">{model.labels.map((label, i) => <span key={i}>{label}</span>)}</div>
  </div></section>;
}

function Breakdown({ title, parts, aside }: { title: string; parts: Slice[]; aside?: string }) {
  if (!parts.length) return null;
  return <section className="usage-breakdown"><h2>{title}{aside && <small>{aside}</small>}</h2><div className="breakdown-bar">{parts.map(part => <span key={part.label} style={{ flexGrow: part.amount, opacity: part.opacity }} />)}</div><div className="breakdown-legend">{parts.map(part => <div key={part.label}><i style={{ opacity: part.opacity }} /><span>{part.label}</span><strong>{part.text}</strong></div>)}</div></section>;
}

function LimitBar({ left, color, even, refill = 0 }: { left: number; color: string; even?: number | null; refill?: number }) {
  return <div className="limit-track" aria-hidden="true"><span style={{ width: `${Math.max(0, Math.min(100, left))}%`, background: color }} />{refill > 0 && <i className="limit-refill" style={{ width: `${Math.max(0, Math.min(100 - left, refill))}%`, color }} />}{even != null && <i className="limit-pace" style={{ left: `clamp(3px, ${even}%, calc(100% - 3px))` }} />}</div>;
}
function Limits({ groups, snapshot, act }: { groups: LimitGroup[]; snapshot: AppSnapshot; act: Act }) {
  if (!snapshot.limits) return null;
  const stale = limitsStaleness(snapshot.limits.checkedAt, Date.parse(snapshot.now));
  return <>
    {snapshot.limits.readFailed && <Notice warning>Could not refresh limits. Showing the last read.</Notice>}
    {stale && <Notice warning>{stale}</Notice>}
    {groups.map(group => <section key={group.driver} className="limit-group"><h2><ProviderIcon kind={group.driver} />{group.label}</h2>{group.rows.map(row => {
      const color = providerStyles[group.driver]?.color ?? '#aaa';
      const pace = row.pace === 'ahead' ? 'Ahead of pace' : row.pace === 'under' ? 'Under pace' : 'On pace';
      return <article className="limit-card" key={row.id}>
        <div className="limit-heading"><strong>{row.label}</strong><span>{row.remainingPercent}% <small>left</small></span>{row.pace && <small title={pace} aria-label={pace}>{row.pace === 'ahead' ? '↗' : row.pace === 'under' ? '↘' : '='}</small>}</div>
        {row.members.length ? <div className="limit-members">{row.members.map((member, index) => {
          const reset = limitReset(member.resetAt, Date.parse(snapshot.now));
          const spoken = [`${member.label}: ${member.remainingPercent}% left`, reset, member.evenPaceLeft != null ? `${member.evenPaceLeft}% of the window left` : null, member.resetCredits ? `${member.resetCredits} reset ${member.resetCredits === 1 ? 'credit' : 'credits'} banked` : null].filter(Boolean).join(', ');
          return <div key={index} title={spoken} aria-label={spoken}>
            <div className="limit-member-heading">{row.members.length > 1 && <><strong>{member.label}</strong><span>{member.remainingPercent}%</span></>}{reset && <small>{reset.replace('resets in ', '↻ ')}</small>}</div>
            <LimitBar left={member.remainingPercent} color={color} even={member.evenPaceLeft} refill={member.resetAt ? member.refillPercent : 0} />
          </div>;
        })}</div> : <><LimitBar left={row.remainingPercent} color={color} />{row.resetAt && <small>{limitReset(row.resetAt, Date.parse(snapshot.now))}</small>}</>}
      </article>;
    })}</section>)}
    {snapshot.limits.links.map(link => <article className="limit-link-card" key={link.url}><strong>{link.label}</strong>{!!link.accounts?.length && <small>{link.accounts.join(', ')}</small>}{(link.url === 'https://chatgpt.com/#settings/Usage' || link.message) && <p>{link.url === 'https://chatgpt.com/#settings/Usage' ? 'View usage in ChatGPT with your connected account.' : link.message}</p>}{link.url.startsWith('https://') && <button className="limit-link" onClick={() => void act('open_limit_link', { url: link.url })}>Manage usage ↗</button>}</article>)}
    {snapshot.limits.notices.map(n => <Notice key={n} warning>{n}</Notice>)}
    {!!snapshot.limits.skippedFiles && <p className="footnote">{snapshot.limits.skippedFiles} provider {snapshot.limits.skippedFiles === 1 ? 'file' : 'files'} could not be read.</p>}
    <p className="footnote">Read from T3 Code's last check of each provider. This app never contacts a provider.</p>
  </>;
}
