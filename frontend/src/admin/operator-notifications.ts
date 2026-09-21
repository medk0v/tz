import { productNamespace } from "../product-edition";

export type BrowserNotificationPermission = NotificationPermission | "unsupported";

export const OPERATOR_NOTIFICATION_SOUNDS = [
  "glass_chime",
  "pearl_chime",
  "warm_keys",
  "droplet_chime",
  "dawn_chime",
  "bell",
  "double_bell",
  "urgent_bell",
  "soft_chime",
  "crystal_ping",
  "rising_chime",
  "deep_bell",
] as const;
export type OperatorNotificationSound = typeof OPERATOR_NOTIFICATION_SOUNDS[number];
export const DEFAULT_OPERATOR_NOTIFICATION_SOUND: OperatorNotificationSound = "glass_chime";

interface BellStrike {
  frequency: number;
  delay: number;
  duration: number;
  volume: number;
}

interface ChimeTone {
  frequency: number;
  delay: number;
  duration: number;
  volume: number;
}

const IN_CHAT_CHIME_TONES: readonly ChimeTone[] = [
  { frequency: 783.99, delay: 0, duration: 0.22, volume: 0.045 },
  { frequency: 1_046.5, delay: 0.09, duration: 0.3, volume: 0.032 },
];
const MINIMUM_IN_CHAT_CHIME_INTERVAL_SECONDS = 1.25;

const BELL_PARTIALS = [
  { ratio: 1, volume: 0.5, decay: 1 },
  { ratio: 2.01, volume: 0.23, decay: 0.72 },
  { ratio: 2.68, volume: 0.14, decay: 0.58 },
  { ratio: 3.92, volume: 0.08, decay: 0.44 },
] as const;

const GLASS_PARTIALS = [
  { ratio: 1, volume: 0.64, decay: 1 },
  { ratio: 2, volume: 0.12, decay: 0.65 },
  { ratio: 3, volume: 0.04, decay: 0.4 },
] as const;

const BELL_PATTERNS: Record<OperatorNotificationSound, readonly BellStrike[]> = {
  glass_chime: [
    { frequency: 880, delay: 0, duration: 0.6, volume: 0.5 },
    { frequency: 1_318.51, delay: 0.16, duration: 0.6, volume: 0.42 },
    { frequency: 1_108.73, delay: 0.32, duration: 0.95, volume: 0.46 },
  ],
  pearl_chime: [
    { frequency: 1_174.66, delay: 0, duration: 0.65, volume: 0.44 },
    { frequency: 880, delay: 0.22, duration: 1.05, volume: 0.48 },
  ],
  warm_keys: [
    { frequency: 523.25, delay: 0, duration: 0.36, volume: 0.48 },
    { frequency: 659.25, delay: 0.13, duration: 0.36, volume: 0.44 },
    { frequency: 783.99, delay: 0.26, duration: 0.42, volume: 0.42 },
    { frequency: 659.25, delay: 0.43, duration: 0.65, volume: 0.46 },
  ],
  droplet_chime: [
    { frequency: 1_568, delay: 0, duration: 0.28, volume: 0.36 },
    { frequency: 1_174.66, delay: 0.12, duration: 0.34, volume: 0.4 },
    { frequency: 1_568, delay: 0.36, duration: 0.6, volume: 0.32 },
  ],
  dawn_chime: [
    { frequency: 587.33, delay: 0, duration: 0.85, volume: 0.42 },
    { frequency: 880, delay: 0.26, duration: 0.85, volume: 0.4 },
    { frequency: 1_174.66, delay: 0.52, duration: 1.1, volume: 0.38 },
  ],
  bell: [
    { frequency: 1_046.5, delay: 0, duration: 1.15, volume: 0.92 },
  ],
  double_bell: [
    { frequency: 987.77, delay: 0, duration: 0.9, volume: 0.86 },
    { frequency: 1_318.51, delay: 0.28, duration: 1.15, volume: 0.94 },
  ],
  urgent_bell: [
    { frequency: 1_174.66, delay: 0, duration: 0.72, volume: 0.88 },
    { frequency: 880, delay: 0.22, duration: 0.72, volume: 0.84 },
    { frequency: 1_174.66, delay: 0.44, duration: 0.9, volume: 0.92 },
  ],
  soft_chime: [
    { frequency: 659.25, delay: 0, duration: 0.72, volume: 0.42 },
    { frequency: 783.99, delay: 0.16, duration: 0.9, volume: 0.5 },
  ],
  crystal_ping: [
    { frequency: 1_318.51, delay: 0, duration: 0.78, volume: 0.54 },
  ],
  rising_chime: [
    { frequency: 523.25, delay: 0, duration: 0.52, volume: 0.45 },
    { frequency: 659.25, delay: 0.14, duration: 0.62, volume: 0.5 },
    { frequency: 783.99, delay: 0.28, duration: 0.82, volume: 0.56 },
  ],
  deep_bell: [
    { frequency: 392, delay: 0, duration: 1.3, volume: 0.62 },
  ],
};

type AudioWindow = Window & typeof globalThis & {
  webkitAudioContext?: typeof AudioContext;
};

let audioContext: AudioContext | null = null;
let lastInChatChimeAt = Number.NEGATIVE_INFINITY;

function notificationAudioContext(): AudioContext | null {
  if (audioContext) return audioContext;
  const AudioContextConstructor = window.AudioContext
    ?? (window as AudioWindow).webkitAudioContext;
  if (!AudioContextConstructor) return null;
  audioContext = new AudioContextConstructor();
  return audioContext;
}

export function enableOperatorNotifications(): Promise<BrowserNotificationPermission> {
  if (typeof Notification === "undefined") {
    return Promise.resolve("unsupported");
  }
  return Notification.permission === "default"
    ? Notification.requestPermission()
    : Promise.resolve(Notification.permission);
}

export async function enableOperatorNotificationSound(): Promise<void> {
  const context = notificationAudioContext();
  if (context?.state === "suspended") {
    await context.resume().catch(() => undefined);
  }
}

export function currentBrowserNotificationPermission(): BrowserNotificationPermission {
  return typeof Notification === "undefined" ? "unsupported" : Notification.permission;
}

export function isOperatorNotificationSound(value: string | null): value is OperatorNotificationSound {
  return OPERATOR_NOTIFICATION_SOUNDS.includes(value as OperatorNotificationSound);
}

function scheduleBellStrike(
  context: AudioContext,
  output: AudioNode,
  startedAt: number,
  strike: BellStrike,
  partials: readonly { ratio: number; volume: number; decay: number }[] = BELL_PARTIALS,
): number {
  const strikeStart = startedAt + strike.delay;

  for (const partial of partials) {
    const oscillator = context.createOscillator();
    const envelope = context.createGain();
    const partialDuration = strike.duration * partial.decay;
    const strikeEnd = strikeStart + partialDuration;

    oscillator.type = "sine";
    oscillator.frequency.setValueAtTime(strike.frequency * partial.ratio, strikeStart);
    envelope.gain.setValueAtTime(0.0001, strikeStart);
    envelope.gain.exponentialRampToValueAtTime(
      strike.volume * partial.volume,
      strikeStart + 0.008,
    );
    envelope.gain.exponentialRampToValueAtTime(0.0001, strikeEnd);
    oscillator.connect(envelope);
    envelope.connect(output);
    oscillator.start(strikeStart);
    oscillator.stop(strikeEnd + 0.025);
  }

  return strikeStart + strike.duration;
}

export function playOperatorNotificationSound(
  sound: OperatorNotificationSound = DEFAULT_OPERATOR_NOTIFICATION_SOUND,
): void {
  const context = notificationAudioContext();
  if (!context || context.state !== "running") return;
  const now = context.currentTime;
  const output = context.createGain();
  const compressor = context.createDynamicsCompressor();

  output.gain.setValueAtTime(0.95, now);
  compressor.threshold.setValueAtTime(-8, now);
  compressor.knee.setValueAtTime(6, now);
  compressor.ratio.setValueAtTime(4, now);
  compressor.attack.setValueAtTime(0.002, now);
  compressor.release.setValueAtTime(0.2, now);
  output.connect(compressor);
  compressor.connect(context.destination);

  const partials = sound === "glass_chime"
    || sound === "pearl_chime"
    || sound === "warm_keys"
    || sound === "droplet_chime"
    || sound === "dawn_chime"
    ? GLASS_PARTIALS
    : BELL_PARTIALS;
  const stopAt = BELL_PATTERNS[sound].reduce(
    (latest, strike) => Math.max(latest, scheduleBellStrike(context, output, now, strike, partials)),
    now,
  );

  window.setTimeout(() => {
    output.disconnect();
    compressor.disconnect();
  }, Math.ceil((stopAt - now + 0.1) * 1_000));
}

export function playOperatorInChatSound(): void {
  const context = notificationAudioContext();
  if (!context || context.state !== "running") return;
  const now = context.currentTime;
  if (now - lastInChatChimeAt < MINIMUM_IN_CHAT_CHIME_INTERVAL_SECONDS) return;
  lastInChatChimeAt = now;

  const output = context.createGain();
  output.gain.setValueAtTime(0.85, now);
  output.connect(context.destination);

  let stopAt = now;
  for (const tone of IN_CHAT_CHIME_TONES) {
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

export function showOperatorBrowserNotification(title: string, body: string): void {
  if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
  const notification = new Notification(title, {
    body,
    tag: `${productNamespace}-${Date.now()}`,
  });
  notification.onclick = () => {
    window.focus();
    notification.close();
  };
}
