type AudioWindow = Window & typeof globalThis & {
  webkitAudioContext?: typeof AudioContext;
};

interface ChimeTone {
  frequency: number;
  delay: number;
  duration: number;
  volume: number;
}

const CHIME_TONES: readonly ChimeTone[] = [
  { frequency: 783.99, delay: 0, duration: 0.22, volume: 0.045 },
  { frequency: 1_046.5, delay: 0.09, duration: 0.3, volume: 0.032 },
];
const MINIMUM_CHIME_INTERVAL_SECONDS = 1.25;

let audioContext: AudioContext | null = null;
let lastChimeAt = Number.NEGATIVE_INFINITY;

function widgetAudioContext(): AudioContext | null {
  if (audioContext) return audioContext;
  const AudioContextConstructor = window.AudioContext
    ?? (window as AudioWindow).webkitAudioContext;
  if (!AudioContextConstructor) return null;
  try {
    audioContext = new AudioContextConstructor();
  } catch {
    return null;
  }
  return audioContext;
}

export function supportsWidgetNotificationSound(): boolean {
  return Boolean(
    window.AudioContext
    ?? (window as AudioWindow).webkitAudioContext,
  );
}

export async function enableWidgetNotificationSound(): Promise<void> {
  const context = widgetAudioContext();
  if (context?.state === "suspended") {
    await context.resume().catch(() => undefined);
  }
}

export function playWidgetNotificationSound(): void {
  const context = widgetAudioContext();
  if (!context || context.state !== "running") return;

  const now = context.currentTime;
  if (now - lastChimeAt < MINIMUM_CHIME_INTERVAL_SECONDS) return;
  lastChimeAt = now;

  const output = context.createGain();
  output.gain.setValueAtTime(0.85, now);
  output.connect(context.destination);

  let stopAt = now;
  for (const tone of CHIME_TONES) {
    const startedAt = now + tone.delay;
    const endedAt = startedAt + tone.duration;
    const oscillator = context.createOscillator();
    const envelope = context.createGain();

    oscillator.type = "sine";
    oscillator.frequency.setValueAtTime(tone.frequency, startedAt);
    envelope.gain.setValueAtTime(0.0001, startedAt);
    envelope.gain.exponentialRampToValueAtTime(tone.volume, startedAt + 0.012);
    envelope.gain.exponentialRampToValueAtTime(0.0001, endedAt);
    oscillator.connect(envelope);
    envelope.connect(output);
    oscillator.start(startedAt);
    oscillator.stop(endedAt + 0.02);
    stopAt = Math.max(stopAt, endedAt);
  }

  window.setTimeout(() => output.disconnect(), Math.ceil((stopAt - now + 0.05) * 1_000));
}
