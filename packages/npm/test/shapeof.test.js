import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

const consumer = process.env.SHAPEOF_PACKAGE;

async function entry() {
  if (!consumer) return new URL('../dist/shapeof.js', import.meta.url).href;
  const manifest = resolve(consumer, 'package.json');
  const name = Object.keys(JSON.parse(await readFile(manifest, 'utf8')).dependencies).find((d) => d === '@meritt/shapeof-wasm');
  return pathToFileURL(createRequire(manifest).resolve(name)).href;
}

const { ShapeOfError, load } = await import(await entry());

const engine = await load();
const { version } = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));

const corpusPath = process.env.SHAPEOF_CORPUS ?? fileURLToPath(new URL('../../../testdata/cases.json', import.meta.url));
const corpus = JSON.parse(await readFile(corpusPath, 'utf8'));

test('corpus format', () => {
  assert.equal(corpus.format, 1);
});

test('corpus cases', () => {
  assert.ok(corpus.cases.length > 0);
  for (const c of corpus.cases) {
    assert.deepEqual(engine.schema(c.input, c.target), c.want, c.name);
  }
});

test('reports the version of the package', () => {
  assert.equal(engine.version, version);
});

test('refuses unknown targets', () => {
  assert.throws(
    () => engine.schema('{}', 'gemini'),
    (error) => error instanceof ShapeOfError && error.code === 'invalid_argument'
  );
});

test('refuses a module from another build', async () => {
  await assert.rejects(
    load(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])),
    (error) => error instanceof ShapeOfError && error.code === 'version'
  );
});
