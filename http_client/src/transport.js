const MIME_EXTENSIONS = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'image/svg+xml': 'svg',
  'application/pdf': 'pdf',
  'application/json': 'json',
  'text/plain': 'txt',
  'text/html': 'html',
};

export function extractBinaryFile(dataTransfer, source = 'pasted') {
  if (!dataTransfer) return undefined;
  let file = dataTransfer.files?.[0];
  if (!file && dataTransfer.items) {
    for (const item of dataTransfer.items) {
      if (item.kind === 'file') {
        file = item.getAsFile();
        if (file) break;
      }
    }
  }
  if (file) {
    const pastedImage = source === 'pasted'
      && file.type.startsWith('image/')
      && (!file.name || /^image\.[a-z0-9]+$/i.test(file.name));
    if (pastedImage) {
      return new File([file], 'pasted image', {
        type: file.type,
        lastModified: file.lastModified || Date.now(),
      });
    }
    if (file.name) return file;
    const ext = MIME_EXTENSIONS[file.type] ?? 'bin';
    return new File([file], `${source}.${ext}`, {
      type: file.type || 'application/octet-stream',
      lastModified: file.lastModified || Date.now(),
    });
  }
  const text = dataTransfer.getData?.('text/plain');
  if (text) return new File([text], `${source} text`, { type: 'text/plain' });
  return undefined;
}

export function prepareRequestBody(format, text, binaryFile) {
  if (format === 'no_body') return undefined;
  if (format === 'text') return text;
  if (format === 'binary') return binaryFile ?? new Blob([]);
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
