const MEDIA_TYPE = 'application/vnd.http-client.proxy';
const encoder = new TextEncoder();
const utf8 = new TextDecoder();

function multipartHeaders(headers, contentType) {
  const lines = headers.split('\n');
  const index = lines.findIndex(line => /^content-type\s*:/i.test(line.trimStart()));
  if (index < 0) lines.push(`Content-Type: ${contentType}`);
  else if (/^content-type\s*:\s*multipart\/form-data\s*$/i.test(lines[index].trim())) {
    lines[index] = `Content-Type: ${contentType}`;
  }
  return lines.join('\n');
}

async function requestPacket({ body, headers, ...request }) {
  if (body instanceof FormData) {
    // Use the same browser-generated boundary in the content type and bytes.
    const multipart = new Response(body);
    headers = multipartHeaders(headers, multipart.headers.get('content-type'));
    body = await multipart.blob();
  } else if (body instanceof URLSearchParams) {
    body = body.toString();
  }
  const metadata = encoder.encode(JSON.stringify({ ...request, headers, has_body: body !== undefined }));
  const length = new DataView(new ArrayBuffer(4));
  length.setUint32(0, metadata.byteLength);
  return new Blob([length.buffer, metadata, ...(body === undefined ? [] : [body])], { type: MEDIA_TYPE });
}

function timelineEvents(events = [], request) {
  let { method, url } = request;
  return events.flatMap(({ label, detail, elapsed_ms }, index) => {
    if (label === 'Redirect') {
      // Older proxies record the redirect headers and destination as a single event.
      const redirect = /^(\d{3}) → (.+)$/.exec(detail);
      if (!redirect || events[index + 1]?.label !== 'Request start send') return [];
      const [, status, destination] = redirect;
      if ((['301', '302'].includes(status) && method === 'POST') ||
          (status === '303' && method !== 'HEAD')) method = 'GET';
      url = destination;
      return [{ label: `${status} response received`, elapsed_ms, side: 'response' }];
    }
    if (label === 'Request start send') label = `sending ${method} request to ${url}`;
    if (label === 'Request end send') label = 'request sent';
    if (label === 'Response start receive') label = 'receiving response';
    const side = label.startsWith('sending ') || label === 'request sent' ? 'request' : 'response';
    return [{ label, elapsed_ms, side }];
  });
}

async function readResponse(response, request) {
  if (!response.ok) {
    const detail = await response.json().catch(() => null);
    const error = new Error(detail?.error ?? `Proxy returned HTTP ${response.status}`);
    error.timeline = timelineEvents(detail?.timeline, request);
    throw error;
  }
  if (response.headers.get('content-type') !== MEDIA_TYPE) {
    throw new Error('Invalid proxy response. Open the HTTP client through the Salvo server.');
  }
  const packet = await response.blob();
  if (packet.size < 12) throw new Error('Incomplete proxy response');
  const length = new DataView(await packet.slice(0, 4).arrayBuffer()).getUint32(0);
  if (4 + length > packet.size - 8) throw new Error('Incomplete proxy response metadata');
  const metadata = JSON.parse(utf8.decode(await packet.slice(4, 4 + length).arrayBuffer()));
  const end = new DataView(await packet.slice(-8).arrayBuffer()).getBigUint64(0);
  const elapsed = Number(end) / 1000;
  metadata.elapsed_ms = Math.round(elapsed);
  metadata.timeline = timelineEvents(metadata.timeline, request);
  metadata.timeline.push({ label: `${metadata.status} response received`, elapsed_ms: elapsed, side: 'response' });
  const type = metadata.headers.find(([name]) => name === 'content-type')?.[1] ?? '';
  const mime = type.split(';', 1)[0].trim().toLowerCase();
  const body = packet.slice(4 + length, -8, mime);
  const format = mime.startsWith('text/') || /(?:\/|\+)xml$/.test(mime) || mime === 'application/javascript'
    ? 'text' : !mime || /(?:\/|\+)json5?$/.test(mime) ? 'json' : 'binary';
  if (format === 'binary') return { ...metadata, bytes: body.size, format, body };
  const charset = /;\s*charset\s*=\s*"?([^";\s]+)/i.exec(type)?.[1];
  let decoder = utf8;
  if (charset) {
    try { decoder = new TextDecoder(charset); } catch { /* Use UTF-8 for unknown encodings. */ }
  }
  return {
    ...metadata,
    bytes: body.size,
    format,
    body: decoder === utf8 ? await body.text() : decoder.decode(await body.arrayBuffer()),
  };
}

export function createProxyTransport() {
  return {
    async send(request) {
      const body = await requestPacket(request);
      let response;
      try {
        response = await fetch('/proxy', { method: 'POST', body, credentials: 'omit' });
      } catch (error) {
        throw new Error('Could not reach the local HTTP client proxy', { cause: error });
      }
      return readResponse(response, request);
    },
  };
}
