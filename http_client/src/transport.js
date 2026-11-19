export function prepareRequestBody(format, text, binaryFile) {
  if (format === 'no_body') return undefined;
  if (format === 'text') return text;
  if (format === 'binary') {
    if (!binaryFile) throw new Error('Choose a file for the binary request body');
    return binaryFile;
  }
  const entries = [];
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim()) continue;
    const equals = line.indexOf('=');
    if (equals < 1 || !line.slice(0, equals).trim()) {
      throw new Error('Enter one name=value pair per line in the request body');
    }
    entries.push([line.slice(0, equals).trim(), line.slice(equals + 1)]);
  }
  if (format === 'url_encoded') return new URLSearchParams(entries);
  if (format === 'multi_part') {
    const body = new FormData();
    for (const [name, value] of entries) body.append(name, value);
    return body;
  }
  throw new Error(`Unsupported request body format: ${format}`);
}

/**
 * A transport accepts a request with method, URL (including query), headers, body,
 * timeout_ms (zero disables the timeout), and follow_redirects.
 * It resolves to the status, timing, size, headers and body or rejects on failure.
 */
export function createPreviewTransport(response) {
  return {
    async send(request) {
      const url = URL.parse(request.url);
      if (!['http:', 'https:'].includes(url?.protocol)) {
        throw new Error('Enter an HTTP or HTTPS URL');
      }
      return response;
    },
  };
}
