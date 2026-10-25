// The loading overlay covers the editor. Keep a bounded initial preview behind
// it; the complete response is retained and analyzed by the document worker.
export class StreamPreviewBudget {
  constructor() {
    this.remaining = 512 * 1024;
  }

  take(text) {
    if (!this.remaining) return '';
    let end = Math.min(text.length, this.remaining);
    if (end < text.length) {
      const last = text.charCodeAt(end - 1);
      if (last >= 0xd800 && last <= 0xdbff) end--;
    }
    const result = text.slice(0, end);
    this.remaining = end < text.length ? 0 : this.remaining - end;
    return result;
  }
}
