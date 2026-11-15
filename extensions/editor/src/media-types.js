export const MEDIA_TYPES = [
  'application/json', 'application/json5', 'text/json', 'text/json5',
  'application/*+json', 'application/*+json5',
];

export function isJsonType(value) {
  const type = value?.split(';', 1)[0].trim().toLowerCase();
  return /^(?:application\/(?:json5?|[^\s/;]+\+json5?)|text\/json5?)$/.test(type ?? '');
}

export function responseRules() {
  return [{
    id: 1,
    priority: 1,
    action: {
      type: 'modifyHeaders',
      responseHeaders: [
        { header: 'content-type', operation: 'set', value: 'text/plain; charset=utf-8' },
        { header: 'content-disposition', operation: 'remove' },
      ],
    },
    condition: {
      regexFilter: '^https?://',
      resourceTypes: ['main_frame'],
      responseHeaders: [{
        header: 'content-type',
        values: MEDIA_TYPES.flatMap(type => [type, `${type};*`, `${type} ;*`]),
      }],
    },
  }];
}

export function responseFilename(url, type, disposition = '') {
  const encoded = /filename\*\s*=\s*UTF-8''([^;]+)/i.exec(disposition)?.[1];
  const plain = /filename\s*=\s*(?:"([^"]*)"|([^;]+))/i.exec(disposition);
  let name = encoded || plain?.[1] || plain?.[2] || new URL(url).pathname.split('/').at(-1) || 'response';
  try { name = decodeURIComponent(name.trim()); } catch { /* Keep malformed URL escapes literal. */ }
  name = name.split(/[\\/]/).at(-1).replace(/[\x00-\x1f\x7f]/g, '') || 'response';
  return /\.json5?$/i.test(name) ? name : `${name}.${/json5/i.test(type) ? 'json5' : 'json'}`;
}
