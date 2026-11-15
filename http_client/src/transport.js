/**
 * A transport accepts a request with method, URL, parameters, headers and body.
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
