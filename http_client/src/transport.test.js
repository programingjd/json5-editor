import assert from 'node:assert/strict';
import test from 'node:test';
import { prepareRequestBody } from './transport.js';

test('form bodies preserve repeated fields and equals signs in values', () => {
  const fields = 'tag=one\r\ntag=two=three\n\n';
  assert.equal(prepareRequestBody('url_encoded', fields).toString(), 'tag=one&tag=two%3Dthree');
  assert.deepEqual(prepareRequestBody('multi_part', fields).getAll('tag'), ['one', 'two=three']);
  assert.throws(() => prepareRequestBody('url_encoded', 'missing equals'), /name=value/);
});

test('text, binary, and absent bodies keep their transport types', () => {
  const binary = new File([new Uint8Array([0, 255])], 'data.bin');
  assert.equal(prepareRequestBody('text', '  plain text\n'), '  plain text\n');
  assert.equal(prepareRequestBody('binary', '', binary), binary);
  const emptyBinary = prepareRequestBody('binary', '');
  assert.ok(emptyBinary instanceof Blob);
  assert.equal(emptyBinary.size, 0);
  assert.equal(prepareRequestBody('no_body', 'retained draft', binary), undefined);
});
