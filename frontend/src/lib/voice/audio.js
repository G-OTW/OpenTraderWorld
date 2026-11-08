/**
 * Microphone capture for push-to-talk.
 *
 * The microphone is opened on press and every track is stopped on release: nothing listens
 * between two presses, and the browser's recording indicator says so. Samples are taken as
 * raw PCM (no MediaRecorder codec, which differs per browser) and handed back as a 16 kHz
 * mono WAV, the one format every speech server accepts.
 */

const TARGET_RATE = 16_000;

/** Why the microphone cannot be used here, or '' when it can. */
export function micBlocker() {
  if (typeof window === 'undefined') return 'unsupported';
  if (!window.isSecureContext) return 'insecure';
  if (!navigator.mediaDevices?.getUserMedia) return 'unsupported';
  return '';
}

/** The browser's own recognizer, when it has one (Chrome, Edge, Safari). */
export function browserRecognizer() {
  if (typeof window === 'undefined') return null;
  return window.SpeechRecognition ?? window.webkitSpeechRecognition ?? null;
}

/**
 * Start recording. Resolves once the microphone is live, to a handle whose `stop()`
 * resolves to a WAV Blob and whose `cancel()` drops everything. `onLevel(0..1)` is called
 * about twenty times a second for the meter.
 */
export async function startRecording({ onLevel = () => {} } = {}) {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true }
  });
  const Ctx = window.AudioContext ?? window.webkitAudioContext;
  const ctx = new Ctx();
  const source = ctx.createMediaStreamSource(stream);
  // ScriptProcessor is deprecated but runs everywhere without a worklet module to serve.
  const node = ctx.createScriptProcessor(4096, 1, 1);
  const chunks = [];
  let samples = 0;
  node.onaudioprocess = (e) => {
    const data = e.inputBuffer.getChannelData(0);
    chunks.push(new Float32Array(data));
    samples += data.length;
    let sum = 0;
    for (let i = 0; i < data.length; i++) sum += data[i] * data[i];
    onLevel(Math.min(1, Math.sqrt(sum / data.length) * 4));
  };
  source.connect(node);
  // A processor only runs while connected to the destination; it writes silence there.
  node.connect(ctx.destination);
  const started = performance.now();

  function release() {
    node.onaudioprocess = null;
    try {
      source.disconnect();
      node.disconnect();
    } catch {
      /* already torn down */
    }
    stream.getTracks().forEach((tr) => tr.stop());
    ctx.close().catch(() => {});
  }

  return {
    get ms() {
      return performance.now() - started;
    },
    async stop() {
      const rate = ctx.sampleRate;
      release();
      const pcm = new Float32Array(samples);
      let at = 0;
      for (const c of chunks) {
        pcm.set(c, at);
        at += c.length;
      }
      return encodeWav(await resample(pcm, rate, TARGET_RATE), TARGET_RATE);
    },
    cancel: release
  };
}

async function resample(pcm, from, to) {
  if (from === to || !pcm.length) return pcm;
  const frames = Math.max(1, Math.round((pcm.length * to) / from));
  const Offline = window.OfflineAudioContext ?? window.webkitOfflineAudioContext;
  const off = new Offline(1, frames, to);
  const buf = off.createBuffer(1, pcm.length, from);
  buf.copyToChannel(pcm, 0);
  const src = off.createBufferSource();
  src.buffer = buf;
  src.connect(off.destination);
  src.start();
  const out = await off.startRendering();
  return out.getChannelData(0);
}

function encodeWav(pcm, rate) {
  const bytes = new ArrayBuffer(44 + pcm.length * 2);
  const v = new DataView(bytes);
  const str = (o, s) => [...s].forEach((ch, i) => v.setUint8(o + i, ch.charCodeAt(0)));
  str(0, 'RIFF');
  v.setUint32(4, 36 + pcm.length * 2, true);
  str(8, 'WAVEfmt ');
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true); // PCM
  v.setUint16(22, 1, true); // mono
  v.setUint32(24, rate, true);
  v.setUint32(28, rate * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  str(36, 'data');
  v.setUint32(40, pcm.length * 2, true);
  for (let i = 0; i < pcm.length; i++) {
    const s = Math.max(-1, Math.min(1, pcm[i]));
    v.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return new Blob([bytes], { type: 'audio/wav' });
}

/** Read text aloud with the browser's own voices. Resolves when done (or at once if mute). */
export function speak(text, lang = '') {
  return new Promise((resolve) => {
    if (typeof speechSynthesis === 'undefined' || !text) return resolve();
    const u = new SpeechSynthesisUtterance(text);
    if (lang) u.lang = lang;
    u.onend = u.onerror = () => resolve();
    speechSynthesis.cancel();
    speechSynthesis.speak(u);
  });
}
