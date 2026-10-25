import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import init, { DocumentProcessor } from './worker-wasm/gpui_json5_processor.js';

await init({ module_or_path: await readFile(new URL('./worker-wasm/gpui_json5_processor_bg.wasm', import.meta.url)) });

function edit(view, at, oldLength, replacement) {
  const point = (offset) => {
    const prefix = view.slice(0, offset).split('\n');
    return [prefix.length - 1, new TextEncoder().encode(prefix.at(-1)).length];
  };
  const start = point(at);
  const oldEnd = point(at + oldLength);
  const parts = replacement.split('\n');
  const newEnd = [start[0] + parts.length - 1,
    (parts.length === 1 ? start[1] : 0) + new TextEncoder().encode(parts.at(-1)).length];
  return { start: new TextEncoder().encode(view.slice(0, at)).length,
    old_end: new TextEncoder().encode(view.slice(0, at + oldLength)).length,
    replacement, start_row: start[0], start_column: start[1],
    old_end_row: oldEnd[0], old_end_column: oldEnd[1], new_end_row: newEnd[0], new_end_column: newEnd[1] };
}

test('worker highlights unicode keys, valid escapes and diagnoses invalid escapes', () => {
  const processor = new DocumentProcessor();
  const result = processor.open("{ 'é': 'a\\n', 'a\\u0062': true, bad: '\\q', hex: 0xdecaf }");
  const view = new TextEncoder().encode(processor.view());
  const runs = result.highlights();
  const tokens = [];
  let previousEnd = 0;
  for (let index = 0; index < runs.length; index += 3) {
    const [start, end, semantic] = runs.slice(index, index + 3);
    assert.ok(start >= previousEnd);
    assert.ok(end <= view.length);
    tokens.push([new TextDecoder().decode(view.slice(start, end)), semantic]);
    previousEnd = end;
  }
  assert.ok(tokens.some(token => token[0] === "'é'" && token[1] === 1));
  assert.ok(tokens.some(token => token[0] === '\\u0062' && token[1] === 9));
  assert.ok(tokens.some(token => token[0] === '\\n' && token[1] === 9));
  assert.ok(tokens.some(token => token[0] === "'\\q'" && token[1] === 2));
  assert.ok(JSON.parse(result.diagnostics()).some((entry) => entry.code === 'json5/invalid-escape'));
  result.free();
  processor.free();
});

test('worker edits and undo preserve the original source whitespace on save', () => {
  const original = '{"a":[1,2],\n    "b": 3}';
  const processor = new DocumentProcessor();
  processor.open(original).free();
  let view = processor.view();
  const at = view.indexOf('3');
  processor.apply_edits(JSON.stringify([edit(view, at, 1, '4')])).free();
  assert.equal(processor.save(false), original.replace('3', '4'));
  assert.equal(processor.save(true), '{"a":[1,2],"b":4}');
  view = processor.view();
  processor.apply_edits(JSON.stringify([edit(view, at, 1, '3')])).free();
  assert.equal(processor.save(false), original);
  processor.free();
});

test('incremental syntax diagnostics recover after undoing an invalid edit', () => {
  const processor = new DocumentProcessor();
  processor.open('{a: 1, b: 2}').free();
  let view = processor.view();
  const at = view.indexOf(',');
  let result = processor.apply_edits(JSON.stringify([edit(view, at, 1, '')]));
  assert.ok(JSON.parse(result.diagnostics()).length > 0);
  result.free();
  view = processor.view();
  result = processor.apply_edits(JSON.stringify([edit(view, at, 0, ',')]));
  assert.deepEqual(JSON.parse(result.diagnostics()), []);
  result.free();
  processor.free();
});
