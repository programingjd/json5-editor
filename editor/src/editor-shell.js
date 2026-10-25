// Shared loading/error surfaces. Install before GPUI registers input handlers.
export function mountEditorShell() {
  const template = document.createElement('template');
  template.innerHTML = `
    <div id="error"></div>
    <div id="parser-busy" role="status" aria-live="off" hidden>
      <span class="processing-spinner" aria-hidden="true"></span>
      <span class="visually-hidden">Parsing document</span>
    </div>
    <section id="document-loading" role="dialog" aria-modal="true" aria-labelledby="loading-title" tabindex="-1" hidden>
      <div class="loading-panel">
        <div class="loading-heading">
          <span class="processing-spinner" aria-hidden="true"></span>
          <h1 id="loading-title"></h1>
        </div>
        <div id="loading-progress" hidden>
          <p id="loading-size"></p>
          <div id="loading-progress-bar" role="progressbar" aria-label="File loading" aria-valuemin="0">
            <div class="loading-progress-fill"></div>
          </div>
        </div>
        <div class="loading-actions">
          <button id="abort-loading" type="button">Abort</button>
        </div>
      </div>
    </section>
`;
  document.body.append(template.content);
}
