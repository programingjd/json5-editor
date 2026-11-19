const body = JSON.stringify({
  took: 44,
  timed_out: false,
  total: 5,
  matches: [
    { id: 'A-104', name: 'Pocket guide', category: 'books', available: true },
    { id: 'A-135', name: 'Trail map', category: 'books', available: true },
  ],
  facets: {
    category: { books: 3, maps: 2 },
    available: { true: 4, false: 1 },
  },
});

export const preview = {
  url: 'https://api.example.com/catalog/_search',
  request: JSON.stringify({
    query: {
      bool: {
        filter: [
          { term: { available: true } },
          { terms: { category: ['books', 'maps'] } },
        ],
        must: [
          { match: { name: 'guide' } },
        ],
      },
    },
    sort: [
      { updated_at: 'desc' },
    ],
    size: 25,
  }, null, 2),
  headers: 'Content-Type: application/json\nAccept: application/json',
  response: {
    preview: true,
    status: 200,
    status_text: 'OK',
    elapsed_ms: 81,
    bytes: new TextEncoder().encode(body).byteLength,
    headers: [['content-type', 'application/json; charset=utf-8'], ['cache-control', 'no-cache']],
    timeline: [
      { label: 'sending POST request to https://api.example.com/catalog/_search', elapsed_ms: 0, side: 'request' },
      { label: 'request sent', elapsed_ms: 2, side: 'request' },
      { label: 'receiving response', elapsed_ms: 79, side: 'response' },
      { label: '200 response received', elapsed_ms: 81, side: 'response' },
    ],
    body,
  },
};
