import { useEffect, useRef, useState, type FormEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { ConnectionCandidate, ConnectionInput, EnvironmentView } from './contracts';

type Stage = { kind: 'edit' } | { kind: 'checking' }
  | { kind: 'ready' | 'saving'; candidate: ConnectionCandidate; input: ConnectionInput };

export function ManualConnections({ environments, savedIds }: { environments: EnvironmentView[]; savedIds: string[] }) {
  const [open, setOpen] = useState(false);
  return <div className="manual-connections">
    <button onClick={() => setOpen(true)} disabled={open}>Add environment</button>
    {open && <AddConnection onClose={() => setOpen(false)} />}
    {environments.filter(environment => savedIds.includes(environment.id)).map(environment =>
      <RemoveConnection key={environment.id} environment={environment} />)}
  </div>;
}

function AddConnection({ onClose }: { onClose: () => void }) {
  const [mode, setMode] = useState<'direct' | 'ssh'>('direct');
  const [address, setAddress] = useState('');
  const [hostname, setHostname] = useState('');
  const [username, setUsername] = useState('');
  const [port, setPort] = useState('');
  const [token, setToken] = useState('');
  const [stage, setStage] = useState<Stage>({ kind: 'edit' });
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  useEffect(() => () => { generation.current += 1; }, []);
  const busy = stage.kind === 'checking' || stage.kind === 'saving';

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setError(null);
    if (stage.kind === 'ready') {
      setStage({ ...stage, kind: 'saving' });
      try {
        await invoke('add_environment', { input: stage.input, environment: stage.candidate.environmentId, token });
        setToken(''); setAddress(''); onClose();
      } catch (failure) {
        setToken(''); setStage(stage); setError(String(failure));
      }
      return;
    }
    const numericPort = port.trim() === '' ? null : Number(port);
    if (mode === 'ssh' && numericPort !== null && (!Number.isInteger(numericPort) || numericPort < 1 || numericPort > 65535)) {
      setError('Enter an SSH port between 1 and 65535.'); return;
    }
    const input: ConnectionInput = mode === 'direct'
      ? { kind: 'direct', address: address.trim() }
      : { kind: 'ssh', hostname: hostname.trim(), username: username.trim() || null, port: numericPort };
    const current = ++generation.current;
    setStage({ kind: 'checking' });
    try {
      const candidate = await invoke<ConnectionCandidate>('probe_environment', { input });
      if (generation.current === current) setStage({ kind: 'ready', candidate, input });
    } catch (failure) {
      if (generation.current === current) { setStage({ kind: 'edit' }); setError(String(failure)); }
    }
  };

  return <form className="connection manual-connection" aria-label="Add environment" onSubmit={event => void submit(event)}>
    {stage.kind === 'edit' || stage.kind === 'checking' ? <>
      <label>Connection type<select value={mode} disabled={busy} onChange={event => {
        if (event.target.value === 'direct' || event.target.value === 'ssh') setMode(event.target.value);
      }}><option value="direct">Server address or pairing link</option><option value="ssh">SSH</option></select></label>
      {mode === 'direct' ? <label>Server address or pairing link<input autoFocus required autoComplete="off" spellCheck={false} value={address} disabled={busy} placeholder="https://server.example" onChange={event => setAddress(event.target.value)} /></label>
        : <>
          <label>SSH host or alias<input autoFocus required autoComplete="off" value={hostname} disabled={busy} onChange={event => setHostname(event.target.value)} /></label>
          <label>SSH user (optional)<input autoComplete="off" value={username} disabled={busy} onChange={event => setUsername(event.target.value)} /></label>
          <label>SSH port (optional)<input inputMode="numeric" value={port} disabled={busy} onChange={event => setPort(event.target.value)} /></label>
          <p className="notice">Uses your SSH configuration and agent. T3 Code must already be running on this host.</p>
        </>}
    </> : <>
      <p>Connect to <strong>{stage.candidate.label}</strong></p>
      <p className="connection-destination">{stage.candidate.destination}</p>
      <p className="notice">Environment: {stage.candidate.environmentId}</p>
      {stage.candidate.hasPairingKey ? <p className="notice">Pairing key included in the link.</p>
        : <label>{stage.candidate.label} pairing key<input autoFocus type="password" required autoComplete="off" value={token} disabled={busy} onChange={event => setToken(event.target.value)} /></label>}
      <p className="notice">Create a fresh pairing key in this environment. Bar requests read access only.</p>
    </>}
    {error && <p className="notice warning" role="alert">{error}</p>}
    <div className="settings-actions">
      <button type="submit" disabled={busy}>{stage.kind === 'checking' ? 'Checking…' : stage.kind === 'saving' ? 'Connecting…' : stage.kind === 'ready' ? 'Connect environment' : 'Check server'}</button>
      {stage.kind === 'ready' && <button type="button" className="secondary-button" onClick={() => { setToken(''); setError(null); setStage({ kind: 'edit' }); }}>Back</button>}
      <button type="button" className="secondary-button" disabled={stage.kind === 'saving'} onClick={() => { generation.current += 1; onClose(); }}>Cancel</button>
    </div>
  </form>;
}

function RemoveConnection({ environment }: { environment: EnvironmentView }) {
  const [confirming, setConfirming] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!confirming) return <button className="secondary-button" onClick={() => setConfirming(true)}>Remove saved connections for {environment.label}…</button>;
  return <div className="disconnect-confirmation" role="group" aria-label={`Remove saved connections for ${environment.label}`}>
    <p>Remove Bar’s saved connections and access for {environment.label}? Connections discovered from T3 Code are kept.</p>
    {error && <p className="notice warning" role="alert">{error}</p>}
    <div className="settings-actions"><button disabled={pending} onClick={() => setConfirming(false)}>Cancel</button>
      <button className="danger-button" disabled={pending} onClick={() => {
        setPending(true); setError(null);
        void invoke('remove_saved_environment', { environment: environment.id })
          .then(() => setConfirming(false)).catch(failure => setError(String(failure))).finally(() => setPending(false));
      }}>{pending ? 'Removing…' : 'Remove saved connections'}</button></div>
  </div>;
}
