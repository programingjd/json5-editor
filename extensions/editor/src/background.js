import { isJsonType, responseFilename } from './media-types.js';

const extension = globalThis.browser ?? globalThis.chrome;
const blockingHeaders = extension.runtime.getManifest().permissions.includes('webRequestBlocking');
const writes = new Map();
const keyFor = tabId => `json-response:${tabId}`;
const viewer = extension.runtime.getURL('index.html');
const withoutHash = url => url.split('#', 1)[0];
const navigationFilter = { urls: ['http://*/*', 'https://*/*'], types: ['main_frame'] };

function writeMetadata(tabId, metadata) {
  const key = keyFor(tabId);
  const write = (writes.get(key) ?? Promise.resolve()).catch(() => {}).then(() =>
    metadata ? extension.storage.session.set({ [key]: metadata }) : extension.storage.session.remove(key));
  writes.set(key, write);
  void write.finally(() => { if (writes.get(key) === write) writes.delete(key); }).catch(console.error);
}

extension.webRequest.onBeforeRequest.addListener(details => {
  if (details.tabId >= 0) {
    writeMetadata(details.tabId, null);
  }
}, navigationFilter);

// Chromium uses DNR; Firefox permits blocking response-header changes in MV3.
// Both paths observe the original MIME and change only document navigations.
extension.webRequest.onHeadersReceived.addListener(details => {
  if (details.tabId < 0) return;
  const header = name => details.responseHeaders?.find(item => item.name.toLowerCase() === name)?.value ?? '';
  const contentType = header('content-type');
  // Content-Length describes compressed bytes when Content-Encoding is set;
  // the editor receives decoded text, so that size cannot be its progress total.
  const length = header('content-length').trim();
  const encoding = header('content-encoding').trim().toLowerCase();
  const charset = /;\s*charset\s*=\s*"?([^;"\s]+)/i.exec(contentType)?.[1]?.toLowerCase();
  const totalBytes = (!charset || ['utf-8', 'utf8', 'us-ascii'].includes(charset))
    && (!encoding || encoding === 'identity') && /^\d+$/.test(length)
    && Number.isSafeInteger(Number(length)) ? Number(length) : undefined;
  const metadata = isJsonType(contentType) ? {
    url: details.url,
    contentType,
    name: responseFilename(details.url, contentType, header('content-disposition')),
    method: details.method,
    totalBytes,
  } : null;
  writeMetadata(details.tabId, metadata);
  if (blockingHeaders && metadata) {
    return {
      responseHeaders: [
        ...details.responseHeaders.filter(item =>
          !['content-type', 'content-disposition']
            .includes(item.name.toLowerCase())),
        { name: 'Content-Type', value: 'text/plain; charset=utf-8' },
      ],
    };
  }
}, navigationFilter, blockingHeaders ? ['blocking', 'responseHeaders'] : ['responseHeaders']);

function ownerFor(sender) {
  if (sender.id !== extension.runtime.id || sender.frameId !== 0 || !Number.isInteger(sender.tab?.id)
      || sender.tab.id < 0
      || !/^https?:\/\//.test(sender.url ?? '')) {
    return null;
  }
  return { tabId: sender.tab.id };
}

async function inspect(sender, contentType) {
  const owner = ownerFor(sender);
  // Navigation can retire a document while its initial probe is in flight.
  // Decline it without exposing another document's metadata or raising an error.
  if (!owner) return null;
  const key = keyFor(owner.tabId);
  await writes.get(key);
  const stored = (await extension.storage.session.get(key))[key];
  if (stored && withoutHash(stored.url) === withoutHash(sender.url)) return stored;
  // Service-worker/cache-produced JSON may not pass through webRequest/DNR.
  if (!isJsonType(contentType)) return null;
  return { url: sender.url, contentType, method: 'GET', name: responseFilename(sender.url, contentType) };
}

extension.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type !== 'json-response-inspect') return;
  const handle = async () => {
    return { response: await inspect(sender, message.contentType) };
  };
  handle().then(
    result => sendResponse({ ok: true, ...result }),
    error => sendResponse({ ok: false, error: error.message ?? String(error) }),
  );
  return true;
});

extension.tabs.onRemoved.addListener(tabId => {
  writeMetadata(tabId, null);
});
extension.runtime.onInstalled.addListener(() => {
  // Remove response bodies cached by the earlier redirect-based viewer.
  indexedDB.deleteDatabase('json5-editor-responses');
});
extension.action.onClicked.addListener(() => { void extension.tabs.create({ url: viewer }); });
