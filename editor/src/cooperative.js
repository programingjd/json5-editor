// Yield through a browser task without nested timers accumulating minimum delays.
let channel;
const continuations = [];
export function yieldToBrowser() {
  if (!channel) {
    channel = new MessageChannel();
    channel.port1.onmessage = () => continuations.shift()?.();
  }
  return new Promise(resolve => {
    continuations.push(resolve);
    channel.port2.postMessage(null);
  });
}
