import assert from 'node:assert/strict';
import test from 'node:test';
import { extractBinaryFile, prepareRequestBody } from './transport.js';

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

test('extractBinaryFile accepts dropped files, unnamed blobs, and plain text', async () => {
  const file = new File([new Uint8Array([1, 2, 3])], 'payload.bin', { type: 'application/octet-stream' });
  assert.equal(extractBinaryFile({ files: [file] }, 'dropped'), file);

  const unnamed = new File([new Uint8Array([9, 8])], '', { type: 'image/png' });
  const pastedImage = extractBinaryFile({ items: [{ kind: 'file', getAsFile: () => unnamed }] }, 'pasted');
  assert.equal(pastedImage.name, 'pasted image');
  assert.equal(pastedImage.size, 2);

  const syntheticImage = new File([new Uint8Array([7, 6])], 'image.png', { type: 'image/png' });
  const pastedSyntheticImage = extractBinaryFile({ files: [syntheticImage] }, 'pasted');
  assert.equal(pastedSyntheticImage.name, 'pasted image');
  assert.equal(pastedSyntheticImage.size, 2);

  const droppedImage = extractBinaryFile({ files: [syntheticImage] }, 'dropped');
  assert.equal(droppedImage, syntheticImage);

  const pastedText = extractBinaryFile({ files: [], getData: type => (type === 'text/plain' ? 'hello' : '') }, 'pasted');
  assert.equal(pastedText.name, 'pasted text');
  assert.equal(await pastedText.text(), 'hello');

  assert.equal(extractBinaryFile({ files: [], getData: () => '' }, 'pasted'), undefined);
});
