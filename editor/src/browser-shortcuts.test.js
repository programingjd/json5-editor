import { afterEach, mock, test } from 'node:test';
import assert from 'node:assert/strict';
import { preserveNativeContextMenu } from './browser-shortcuts.js';

class FakeCanvas {}

let restoreGlobals = () => {};
afterEach(() => {
  restoreGlobals();
  restoreGlobals = () => {};
});

function setupDom() {
  const previousWindow = globalThis.window;
  const previousCanvas = globalThis.HTMLCanvasElement;
  const listeners = [];
  globalThis.HTMLCanvasElement = FakeCanvas;
  globalThis.window = {
    addEventListener(type, listener, options) {
      listeners.push({ type, listener, capture: !!options?.capture });
    },
    removeEventListener(type, listener, options) {
      const capture = !!options?.capture;
      const index = listeners.findIndex(
        entry => entry.type === type && entry.listener === listener && entry.capture === capture,
      );
      if (index >= 0) listeners.splice(index, 1);
    },
  };
  restoreGlobals = () => {
    globalThis.window = previousWindow;
    globalThis.HTMLCanvasElement = previousCanvas;
  };
  return {
    listeners,
    dispatch(type, event = {}) {
      let stopped = false;
      const dispatched = {
        ...event,
        stopPropagation() { stopped = true; },
      };
      for (const entry of [...listeners]) {
        if (entry.type === type && entry.capture) entry.listener(dispatched);
      }
      return stopped;
    },
  };
}

test('allows the native context menu on the canvas when no custom menu was claimed', () => {
  const { dispatch } = setupDom();
  const consume = mock.fn(() => false);
  const stop = preserveNativeContextMenu(consume);
  const stopped = dispatch('contextmenu', { target: new FakeCanvas(), button: 2, pointerType: 'mouse' });
  assert.equal(stopped, true);
  assert.equal(consume.mock.callCount(), 1);
  stop();
});

test('lets GPUI suppress the native context menu when a custom menu was claimed', () => {
  const { dispatch } = setupDom();
  const consume = mock.fn(() => true);
  const stop = preserveNativeContextMenu(consume);
  const stopped = dispatch('contextmenu', { target: new FakeCanvas(), button: 2, pointerType: 'mouse' });
  assert.equal(stopped, false);
  assert.equal(consume.mock.callCount(), 1);
  stop();
});

test('keeps GPUI suppression for touch long-press contextmenu events', () => {
  const { dispatch } = setupDom();
  const consume = mock.fn(() => false);
  const stop = preserveNativeContextMenu(consume);
  const stopped = dispatch('contextmenu', { target: new FakeCanvas(), button: 0, pointerType: 'touch' });
  assert.equal(stopped, false);
  assert.equal(consume.mock.callCount(), 0);
  stop();
});

test('keeps GPUI suppression for legacy MouseEvent touch long-press without blocking keyboard menus', () => {
  const { dispatch } = setupDom();
  const consume = mock.fn(() => false);
  const stop = preserveNativeContextMenu(consume);
  const canvas = new FakeCanvas();
  dispatch('pointerdown', { target: canvas, pointerType: 'touch' });
  assert.equal(dispatch('contextmenu', { target: canvas, button: 0 }), false);
  assert.equal(consume.mock.callCount(), 0);
  dispatch('keydown', { key: 'ContextMenu' });
  assert.equal(dispatch('contextmenu', { target: canvas, button: 0 }), true);
  assert.equal(consume.mock.callCount(), 0);
  stop();
});

test('allows non-secondary-button contextmenu events without consuming a right-click claim', () => {
  const { dispatch } = setupDom();
  const consume = mock.fn(() => true);
  const stop = preserveNativeContextMenu(consume);
  const stopped = dispatch('contextmenu', { target: new FakeCanvas(), button: 0, pointerType: '' });
  assert.equal(stopped, true);
  assert.equal(consume.mock.callCount(), 0);
  stop();
});

test('ignores contextmenu events outside the canvas and unregisters on cleanup', () => {
  const { dispatch, listeners } = setupDom();
  const consume = mock.fn(() => false);
  const stop = preserveNativeContextMenu(consume);
  assert.equal(dispatch('contextmenu', { target: {}, button: 2, pointerType: 'mouse' }), false);
  assert.equal(consume.mock.callCount(), 0);
  stop();
  assert.equal(listeners.length, 0);
});
