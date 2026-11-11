import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';

const root = process.cwd();
const upstream = resolve(root, '.local-reference/t3code-source');
assert.equal(execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], {encoding:'utf8'}).trim(), '6647b0a761ff7a1287047a9bb5fd5b63772f456b');
const ts = createRequire(resolve(root, 'desktop/package.json'))('typescript');
const source = readFileSync(resolve(upstream, 'packages/shared/src/usageMerge.ts'), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 }, reportDiagnostics: true });
assert.equal(compiled.diagnostics.length, 0);
const module = { exports: {} };
new Function('exports', 'require', 'module', compiled.outputText)(module.exports, name => { assert.equal(name, '@t3tools/contracts'); return { USAGE_MERGE_COMPATIBLE_SINCE: 4 }; }, module);
const schemaRequire = createRequire(resolve(process.env.USAGE_PARITY_DEPS ?? upstream, 'package.json'));
const loadContract = (path, resolveImport) => {
  const code = ts.transpileModule(readFileSync(resolve(upstream, path), 'utf8'), {compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  const loaded = {exports:{}};
  new Function('exports','require','module',code)(loaded.exports, resolveImport, loaded);
  return loaded.exports;
};
const base = loadContract('packages/contracts/src/baseSchemas.ts', schemaRequire);
const contract = loadContract('packages/contracts/src/usage.ts', name => name === './baseSchemas.ts' ? base : schemaRequire(name));
const decodeSummary = schemaRequire('effect/Schema').decodeUnknownSync(contract.UsageSummary);
const bucket = (cost = 10, day = '2026-09-03', extra = {}) => ({ day, provider:'claude', model:'synthetic', sourcePath:'/synthetic', totals:{uncachedInputTokens:10,cachedInputTokens:20,cacheCreationTokens:5,outputTokens:15,reasoningTokens:7}, costUsd:cost,cacheSavingsUsd:2,costSource:'modelPriced',records:2,unpricedRecords:0,sessions:1,...extra });
const env = (id, opts = {}) => ({environmentId:id,label:id,summary:{contractVersion:opts.version ?? 6,readAt:opts.readAt ?? '2026-09-03T10:00:00Z',timeZone:'UTC',sinceDay:'2026-09-01',untilDay:'2026-09-30',buckets:opts.buckets ?? [bucket(opts.cost)],sources:opts.sources ?? [{fingerprint:{hostId:opts.host ?? 'host',provider:'claude',resolvedHomePath:'/synthetic',volumeId:opts.volume ?? 'volume'},status:opts.status ?? 'ok',scannedFiles:1,skippedFiles:0,malformedRecords:0,distinctSessions:opts.sessions ?? 1,message:null}],pricing:{status:'fresh',source:'synthetic',fetchedAt:null,knownModels:1},scanDurationMs:1,...(opts.filter ? {modelFilter:opts.filter} : {})}});
const cases = [
 ['malformed bucket',[env('a',{buckets:[bucket(),{provider:'claude',model:'bad'}]})]],
 ['unknown source',[env('a',{sources:[...env('a').summary.sources,{...env('a').summary.sources[0],status:'new-status'}]})]],
 ['negative tokens',[env('a',{buckets:[bucket(),bucket(99,'2026-09-03',{totals:{...bucket().totals,uncachedInputTokens:-100}})]})]],
 ['unknown provider',[env('a',{buckets:[bucket(),bucket(99,'2026-09-03',{provider:'future-provider'})]})]],
 ['invalid day',[env('a',{buckets:[bucket(),bucket(99,'not-a-day')]})]],
 ['null optional',[env('a',{buckets:[bucket(),bucket(99,'2026-09-03',{fastCostUsd:null})]})]],
 ['fractional tokens',[env('a',{buckets:[bucket(),bucket(99,'2026-09-03',{totals:{...bucket().totals,outputTokens:1.5}})]})]],
 ['case tie',[env('A',{cost:20}),env('a',{cost:10})]],
 ['punctuation tie',[env('env-a',{cost:20}),env('env_a',{cost:10})]],
 ['unicode tie',[env('é',{cost:20}),env('e',{cost:10})]],
 ['empty',[]], ['shared',[env('a'),env('b')]], ['independent',[env('a'),env('b',{host:'host-b',cost:4})]],
 ['volume',[env('a'),env('b',{volume:'other',cost:4})]], ['newest',[env('a'),env('b',{readAt:'2026-09-03T11:00:00Z',cost:14})]],
 ['tie',[env('b',{cost:20}),env('a')]], ['complete',[env('a'),env('b',{status:'partial',readAt:'2026-09-03T11:00:00Z',cost:99})]],
 ['supplement',[env('a'),env('b',{status:'partial',readAt:'2026-09-03T11:00:00Z',sessions:3,buckets:[bucket(99),bucket(4,'2026-09-04')]})]],
 ['filter mismatch',[env('a'),env('b',{status:'partial',readAt:'2026-09-03T11:00:00Z',filter:{include:[{model:'x'}]},buckets:[bucket(4,'2026-09-04')]})]],
 ['failed priority',[env('a',{status:'failed',readAt:'2026-09-03T12:00:00Z'}),env('b',{status:'partial',cost:4})]],
 ['missing',[env('a',{status:'missing'}),env('b',{cost:4})]], ['old',[env('a',{version:3}),env('b',{host:'b',version:4})]], ['future',[env('a',{version:7})]],
 ['one',[env('a'),env('b',{readAt:'2026-09-03T11:00:00Z',cost:14})],'a'],
 ['quality',[env('a',{buckets:[bucket(10,'2026-09-03',{costSource:'providerReported',categoryCostUsd:{input:1,cacheRead:2,cacheWrite:1,output:3},fastCostUsd:2,ultrafastCostUsd:3,speedPremiumUsd:1,unpricedRecords:1,hourStart:'2026-09-03T10:00:00Z'})]})]],
 ['model',[env('a',{buckets:[bucket(10),bucket(5,'2026-09-03',{model:'other'})]})]],
 ['matching filter supplement',[env('a',{filter:{include:[{model:'x'}]}}),env('b',{status:'partial',filter:{include:[{model:'x'}]},readAt:'2026-09-03T11:00:00Z',buckets:[bucket(4,'2026-09-04')]})]],
 ['partial ordering',[env('a'),env('b',{status:'partial',readAt:'2026-09-03T11:00:00Z',buckets:[bucket(4,'2026-09-04')]}),env('c',{status:'partial',readAt:'2026-09-03T12:00:00Z',buckets:[bucket(5,'2026-09-04'),bucket(6,'2026-09-05')]})]],
 ['pathless',[env('a',{buckets:[bucket(10,'2026-09-03',{sourcePath:undefined})]}),env('b',{buckets:[bucket(20,'2026-09-03',{sourcePath:undefined})]})]],
 ['sessions without buckets',[env('a',{buckets:[],sessions:4})]],
 ['older partial',[env('a'),env('b',{status:'partial',readAt:'2026-09-03T09:00:00Z',buckets:[bucket(4,'2026-09-04')]})]],
];
const project = r => ({costUsd:r.costUsd,tokens:{uncachedInput:r.uncachedInputTokens,cachedInput:r.cachedInputTokens,cacheCreation:r.cacheCreationTokens,output:r.outputTokens,reasoning:r.reasoningTokens,processed:r.totalTokens},sessions:r.sessions,records:r.records,unpricedRecords:r.models.reduce((n,m)=>n+m.unpricedRecords,0),costByType:{input:r.categoryCost.input,cacheRead:r.categoryCost.cacheRead,cacheWrite:r.categoryCost.cacheWrite,output:r.categoryCost.output,other:r.categoryCost.unsplit},costBySpeed:r.speedCost,cacheSavingsUsd:r.costQuality.cacheSavingsUsd,costQuality:r.costQuality,models:r.models.map(m=>({kind:m.provider,name:m.model,costUsd:m.costUsd,records:m.records,unpricedRecords:m.unpricedRecords,tokens:{uncachedInput:m.tokens.uncachedInputTokens,cachedInput:m.tokens.cachedInputTokens,cacheCreation:m.tokens.cacheCreationTokens,output:m.tokens.outputTokens,reasoning:m.tokens.reasoningTokens,processed:m.totalTokens}})),duplicateSources:r.duplicateSources,contributingEnvironments:r.contributingEnvironments,contractMismatches:r.contractMismatches});
const corpus = cases.map(([name,environments,one]) => {
 const normalized = environments.map(e => ({...e,summary:decodeSummary(e.summary)}));
 const filter = JSON.stringify(normalized[0]?.summary.modelFilter ?? null);
 const matching = normalized.filter(e => JSON.stringify(e.summary.modelFilter ?? null) === filter);
 return {name,environments,selection:one ? {one} : 'all',expected:project(module.exports.mergeUsage(one ? matching.filter(e=>e.environmentId===one) : matching,6))};
});
writeFileSync(resolve(root,'desktop/tests/fixtures/multi-environment/usage/parity.json'), JSON.stringify(corpus,null,2)+'\n');
console.log(`Generated ${corpus.length} cases from upstream 6647b0a761ff7a1287047a9bb5fd5b63772f456b`);
