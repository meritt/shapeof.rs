const VERSION = '@VERSION@';
const STATUS = ['ok', 'invalid_utf8', 'invalid_argument', 'internal'];
const TARGETS = ['openai', 'anthropic', 'plain'];
const OK = 0;
// shapeof_report on wasm32: status, then the report as (pointer, length).
const REPORT_SIZE = 12;

export class ShapeOfError extends Error {
  constructor(code) {
    super(code);
    this.name = 'ShapeOfError';
    this.code = code;
  }
}

export async function load(source = new URL('./shapeof.wasm', import.meta.url)) {
  const module = await compile(source);
  const { exports } = await WebAssembly.instantiate(module, {});
  return new ShapeOf(exports);
}

async function compile(source) {
  if (source instanceof WebAssembly.Module) return source;
  if (typeof source === 'string') source = new URL(source, import.meta.url);
  if (source instanceof URL) {
    if (source.protocol === 'file:') {
      const { readFile } = await import('node:fs/promises');
      return WebAssembly.compile(await readFile(source));
    }
    source = fetch(source);
  }
  if (source instanceof Promise || source instanceof Response) {
    return WebAssembly.compileStreaming(source);
  }
  return WebAssembly.compile(source);
}

const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });

class ShapeOf {
  #x;

  constructor(exports) {
    this.#x = exports;
    const version = typeof exports.shapeof_version === 'function' ? this.#cString(exports.shapeof_version()) : '';
    if (version !== VERSION) throw new ShapeOfError('version');
    this.version = VERSION;
  }

  schema(text, target) {
    const index = TARGETS.indexOf(target);
    if (typeof text !== 'string' || index < 0) throw new ShapeOfError('invalid_argument');
    // A lone surrogate has no UTF-8 form; it reads as U+FFFD, like TextEncoder writes it.
    const bytes = encoder.encode(text);
    const x = this.#x;
    const out = x.shapeof_alloc(REPORT_SIZE);
    if (out === 0) throw new ShapeOfError('internal');
    try {
      this.#with(bytes, (p, n) => x.shapeof_infer(p, n, index, out));
      // Read only after the call: memory may have grown and detached old views.
      const view = new DataView(x.memory.buffer, out, REPORT_SIZE);
      const status = view.getUint32(0, true);
      if (status !== OK) throw new ShapeOfError(STATUS[status] ?? 'internal');
      const report = new Uint8Array(x.memory.buffer, view.getUint32(4, true), view.getUint32(8, true));
      const result = JSON.parse(decoder.decode(report));
      x.shapeof_report_free(out);
      return result;
    } finally {
      x.shapeof_dealloc(out, REPORT_SIZE);
    }
  }

  #with(bytes, use) {
    const x = this.#x;
    const n = bytes.length;
    const p = x.shapeof_alloc(n);
    if (n > 0 && p === 0) throw new ShapeOfError('invalid_argument');
    try {
      new Uint8Array(x.memory.buffer, p, n).set(bytes);
      return use(p, n);
    } finally {
      x.shapeof_dealloc(p, n);
    }
  }

  #cString(p) {
    const memory = new Uint8Array(this.#x.memory.buffer);
    let end = p;
    while (memory[end] !== 0) end++;
    return decoder.decode(memory.subarray(p, end));
  }
}
