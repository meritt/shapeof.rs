# shapeof.rs

Строит JSON Schema 2020-12 по примерам: одному документу JSON, нестрогому JSON в том виде, в каком его копируют из кода, или JSON Lines. Схема подходит для структурированных ответов OpenAI и Anthropic или для обычной валидации.

## Установка

Rust:

```bash
cargo add shapeof-core
```

JavaScript:

```bash
pnpm add @meritt/shapeof-wasm
```

Swift:

```swift
.package(url: "https://github.com/meritt/shapeof.rs", exact: "0.1.0")
```

## Использование

Rust:

```rust
use shapeof_core::{Target, infer};

let outcome = infer(r#"{"id": 1, "name": "Ада"}"#, Target::OpenAi); // Outcome::Schema { text, notes }
```

JavaScript:

```js
import { load } from '@meritt/shapeof-wasm';

const shapeof = await load();
shapeof.schema('{"id": 1, "name": "Ада"}', 'openai');
// { schema: '{\n  "type": "object",\n  "properties": {…', notes: { rootObject: true, properties: 2, … } }
shapeof.schema('{"id" 1}', 'plain');
// { error: { reason: 'ожидалось двоеточие после имени свойства', line: 1, column: 7, source: '{"id" 1}' } }
```

Swift:

```swift
import ShapeOf

let inferred = try ShapeOf.schema(#"{"id": 1, "name": "Ада"}"#, for: .openai) // inferred.schema, inferred.notes
```

Цели: `openai`, `anthropic`, `plain`. Схема приходит текстом с отступами, `notes` считает, что пришлось обобщить.

Отказ: нечитаемый текст — причина, строка, столбец и текст строки; вложенность глубже 64 уровней — `depth`. JavaScript возвращает его в `{ error }`, Swift бросает `ShapeOfError`, Rust возвращает `Outcome::Invalid` или `Outcome::Deep`. Неверные аргументы дают `invalid_argument`.

## Автор

[Алексей Симоненко](https://simonenko.xyz)

## Лицензия

MIT. Смотрите файл `LICENSE`.
