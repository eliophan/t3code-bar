import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const require = createRequire(resolve('desktop/package.json'));
const ts = require('typescript');
const upstream = readFileSync('.local-reference/t3code-source/packages/shared/src/usageLimits.ts', 'utf8');
const server = readFileSync('.local-reference/t3code-source/packages/contracts/src/server.ts', 'utf8');
const file = ts.createSourceFile('server.ts', server, ts.ScriptTarget.Latest, true);
const available = file.statements.find(s => (ts.isFunctionDeclaration(s) && s.name?.text === 'isProviderAvailable') || (ts.isVariableStatement(s) && s.declarationList.declarations.some(d => d.name.getText(file) === 'isProviderAvailable')));
assert.ok(available);
function evaluate(source, imports = {}) {
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 }, reportDiagnostics: true });
  assert.equal(compiled.diagnostics.length, 0);
  const module = { exports: {} };
  new Function('exports', 'require', 'module', compiled.outputText)(module.exports, name => {
    assert.ok(Object.hasOwn(imports, name), name);
    return imports[name];
  }, module);
  return module.exports;
}
const model = ts.createSourceFile('model.ts', readFileSync('.local-reference/t3code-source/packages/contracts/src/model.ts', 'utf8'), ts.ScriptTarget.Latest, true);
const names = model.statements.find(s => ts.isVariableStatement(s) && s.declarationList.declarations.some(d => d.name.getText(model) === 'PROVIDER_DISPLAY_NAMES'));
assert.ok(names);
const contracts = evaluate('const ProviderDriverKind = { make: value => value }; const CODEX_DRIVER_KIND="codex", CLAUDE_DRIVER_KIND="claudeAgent", CURSOR_DRIVER_KIND="cursor", GROK_DRIVER_KIND="grok", FX_DRIVER_KIND="fx", KIRO_DRIVER_KIND="kiro", MUSE_DRIVER_KIND="muse", ACP_REGISTRY_DRIVER_KIND="acpRegistry", PI_DRIVER_KIND="pi", OPENCODE_DRIVER_KIND="opencode";\n' + available.getText(file) + '\n' + names.getText(model));
const limits = evaluate(upstream, { '@t3tools/contracts': contracts, 'effect/DateTime': {} });
const now = '2026-09-03T12:00:00Z';
const report = (usedPercent, checkedAt = now, extra = {}, windows) => ({ checkedAt, windows: windows ?? [{ id: 'slot', kind: 'session', label: 'Session', usedPercent }], ...extra });
const provider = (id, email, used, opts = {}) => ({ instanceId: id, driver: opts.driver ?? 'codex', enabled: true, installed: true, auth: { status: 'authenticated', ...(email && { email }), ...(opts.workspace && { workspaceId: opts.workspace }) }, usageLimits: report(used, opts.checkedAt, opts.fingerprint ? { credentialFingerprint: opts.fingerprint } : {}, opts.windows) });
const hub = (id, email, used, opts = {}) => ({ id: 'hub', label: 'Hub', accounts: [{ id, driver: opts.driver ?? 'codex', ...(email && { email }), ...(opts.workspace && { workspaceId: opts.workspace }), usageLimits: report(used, opts.checkedAt, opts.fingerprint ? { credentialFingerprint: opts.fingerprint } : {}) }] });
const env = (id, providers = [], sources = []) => ({ id, providers, sources });
const p = (id, email, used, opts) => provider(id, email, used, opts);
const cases = [
  { name: 'same account and workspace freshest', envs: [env('a',[p('a','USER@example.invalid',20,{workspace:'team',checkedAt:'2026-09-03T10:00:00Z'})]), env('b',[p('b',' user@example.invalid ',40,{workspace:'team'})])] },
  { name: 'independent and ambiguous workspaces', envs: [env('a',[p('a','u@example.invalid',20,{workspace:'one'})]),env('b',[p('b','u@example.invalid',40,{workspace:'two'})]),env('c',[p('c','u@example.invalid',60)])] },
  { name: 'unique workspace joins missing', envs: [env('a',[p('a','u@example.invalid',20,{workspace:'one'})]),env('b',[p('b','u@example.invalid',40)])] },
  { name: 'native and hub email', envs: [env('a',[p('a','u@example.invalid',20)], [hub('h','u@example.invalid',40,{checkedAt:'2026-09-03T12:01:00Z'})])] },
  { name: 'fingerprint fallback', envs: [env('a',[p('a',null,20,{fingerprint:'fp'})]),env('b',[],[hub('h',null,40,{fingerprint:'fp',checkedAt:'2026-09-03T12:01:00Z'})])] },
  { name: 'unknown native identities', envs: [env('a',[p('same',null,20)]),env('b',[p('same',null,40)])] },
  { name: 'unknown hub identities scoped to environment', divergence: 'Upstream treats hub source and account IDs as global. Bar scopes unidentified reports to their environment.', envs: [env('a',[],[hub('same',null,20)]),env('b',[],[hub('same',null,40)])] },
  { name: 'selection before ownership', selection: 'a', envs: [env('a',[p('a','u@example.invalid',20)]),env('b',[p('b','u@example.invalid',40,{checkedAt:'2026-09-03T12:01:00Z'})])] },
  { name: 'distinct accounts pool average', envs: [env('a',[p('a','one@example.invalid',20),p('b','two@example.invalid',60)]),env('b',[p('c','one@example.invalid',20)])] },
  { name: 'window kind and ids', envs: [env('a',[p('a','one@example.invalid',0,{windows:[{id:'first',kind:'session',label:'Session',usedPercent:20},{id:'second',kind:'session',label:'Other',usedPercent:30}]}),p('b','two@example.invalid',0,{windows:[{id:'different',kind:'session',label:'Session',usedPercent:60}]})]),env('b',[p('c','third@example.invalid',0,{driver:'cursor',windows:[{id:'autoPercentUsed',kind:'monthly',label:'Auto',usedPercent:10},{id:'apiPercentUsed',kind:'monthly',label:'API',usedPercent:30}]})])] },
];
for (const item of cases) {
  const selected = item.envs.filter(e => !item.selection || e.id === item.selection);
  const presentations = new Map(selected.map(e => [e.id, { entry: { target: { label: e.id } }, serverConfig: { providers: e.providers, usageLimitSources: e.sources } }]));
  const accounts = limits.collectLimitAccounts(presentations);
  item.upstream = { accounts: accounts.length, windows: accounts.map(a => a.limits.windows.map(w => w.usedPercent)).sort((a,b) => a[0]-b[0]), pools: limits.collectLimitPools(accounts, Date.parse(now)).flatMap(pool => pool.windows.map(w => ({driver:pool.driver,id:w.id,usedPercent:w.usedPercent}))).sort((a,b) => `${a.driver}:${a.id}`.localeCompare(`${b.driver}:${b.id}`)) };
}
writeFileSync('desktop/tests/fixtures/multi-environment/limits/parity.json', JSON.stringify({ upstreamSha256: createHash('sha256').update(upstream).digest('hex'), now, cases }, null, 2) + '\n');
console.log(`Generated ${cases.length} upstream limits cases`);
