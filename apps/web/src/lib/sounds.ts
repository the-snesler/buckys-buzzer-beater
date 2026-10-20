/**
 * Sound effects, synthesized with the Web Audio API so there are no audio files to load.
 *
 * Browsers only let a page make sound after the user has interacted with it, so the audio
 * is unlocked on the first tap or key press. The display window is never clicked, so it
 * plays its sounds through the host window instead, see playOnStage.
 */

export type SoundName =
  /** A player joined the lobby */
  | "join"
  /** The board fills in at the start of a round */
  | "boardFill"
  /** A clue, or a Final Jeopardy response, is revealed */
  | "reveal"
  | "dailyDouble"
  /** A player won the buzz */
  | "buzzIn"
  | "correct"
  | "incorrect"
  /** Nobody buzzed in */
  | "timesUp"
  | "finalJeopardy"
  /** One of the last seconds of the Final Jeopardy countdown */
  | "tick"
  | "gameOver"
  /** This player's buzz was sent */
  | "buzz"
  /** This player pressed too early */
  | "lockout"
  /** It's this player's turn to pick a clue */
  | "yourPick";

/** Schedules a sound on `out`, starting at time `t` of the context */
type Voice = (ctx: BaseAudioContext, out: AudioNode, t: number) => void;

interface ToneOptions {
  type?: OscillatorType;
  freq: number;
  /** Glide to this frequency over the tone */
  to?: number;
  start: number;
  /** Seconds from the start until the tone has faded out */
  duration: number;
  gain?: number;
  attack?: number;
  /** Seconds of fade at the end. The tone fades over all of it after the attack by default. */
  release?: number;
  /** Cutoff of a lowpass filter, to soften square and sawtooth waves */
  lowpass?: number;
  vibrato?: { rate: number; depth: number };
}

function envelope(
  ctx: BaseAudioContext,
  start: number,
  duration: number,
  peak: number,
  attack: number,
  release: number
): GainNode {
  const env = ctx.createGain();
  env.gain.setValueAtTime(0.0001, start);
  env.gain.exponentialRampToValueAtTime(peak, start + attack);
  env.gain.setValueAtTime(peak, Math.max(start + attack, start + duration - release));
  env.gain.exponentialRampToValueAtTime(0.0001, start + duration);
  return env;
}

function tone(ctx: BaseAudioContext, out: AudioNode, o: ToneOptions) {
  const attack = o.attack ?? 0.005;
  const osc = ctx.createOscillator();
  osc.type = o.type ?? "sine";
  osc.frequency.setValueAtTime(o.freq, o.start);
  if (o.to !== undefined) {
    osc.frequency.exponentialRampToValueAtTime(o.to, o.start + o.duration);
  }
  if (o.vibrato) {
    const lfo = ctx.createOscillator();
    const depth = ctx.createGain();
    lfo.frequency.value = o.vibrato.rate;
    depth.gain.value = o.vibrato.depth;
    lfo.connect(depth).connect(osc.frequency);
    lfo.start(o.start);
    lfo.stop(o.start + o.duration + 0.05);
  }
  let node: AudioNode = osc;
  if (o.lowpass !== undefined) {
    const filter = ctx.createBiquadFilter();
    filter.type = "lowpass";
    filter.frequency.value = o.lowpass;
    node = node.connect(filter);
  }
  node
    .connect(envelope(ctx, o.start, o.duration, o.gain ?? 0.3, attack, o.release ?? o.duration - attack))
    .connect(out);
  osc.start(o.start);
  osc.stop(o.start + o.duration + 0.05);
}

interface NoiseOptions {
  start: number;
  duration: number;
  gain?: number;
  attack?: number;
  release?: number;
  /** Center of a bandpass filter, swept from `from` to `to` */
  from: number;
  to: number;
  q?: number;
}

function noise(ctx: BaseAudioContext, out: AudioNode, o: NoiseOptions) {
  const attack = o.attack ?? 0.01;
  const buffer = ctx.createBuffer(1, Math.ceil(ctx.sampleRate * o.duration), ctx.sampleRate);
  const data = buffer.getChannelData(0);
  for (let i = 0; i < data.length; i++) data[i] = Math.random() * 2 - 1;
  const source = ctx.createBufferSource();
  source.buffer = buffer;
  const filter = ctx.createBiquadFilter();
  filter.type = "bandpass";
  filter.Q.value = o.q ?? 1;
  filter.frequency.setValueAtTime(o.from, o.start);
  filter.frequency.exponentialRampToValueAtTime(o.to, o.start + o.duration);
  source
    .connect(filter)
    .connect(envelope(ctx, o.start, o.duration, o.gain ?? 0.3, attack, o.release ?? o.duration - attack))
    .connect(out);
  source.start(o.start);
}

// Note frequencies, in Hz
const G4 = 392;
const C5 = 523.25;
const D5 = 587.33;
const E5 = 659.25;
const G5 = 783.99;
const A5 = 880;
const B5 = 987.77;
const C6 = 1046.5;
const D6 = 1174.66;
const E6 = 1318.51;
const G6 = 1567.98;

const voices: Record<SoundName, Voice> = {
  join(ctx, out, t) {
    tone(ctx, out, { freq: 420, to: 900, start: t, duration: 0.14, gain: 0.7 });
  },

  boardFill(ctx, out, t) {
    // The tiles light up in a quick, scattered run, then the board settles on a chord
    const run = [C5, E5, G5, C6, G5, E5, A5, D6, B5, G5, E6, C6, A5, G6, D6, E6];
    run.forEach((freq, i) => {
      tone(ctx, out, { type: "triangle", freq, start: t + i * 0.055, duration: 0.14, gain: 0.2 });
    });
    const end = t + run.length * 0.055;
    for (const freq of [C5, E5, G5, C6]) {
      tone(ctx, out, { type: "triangle", freq, start: end, duration: 0.9, gain: 0.12, release: 0.6 });
    }
  },

  reveal(ctx, out, t) {
    noise(ctx, out, { start: t, duration: 0.35, from: 500, to: 5000, gain: 0.5, attack: 0.12, q: 2 });
    tone(ctx, out, { freq: 330, to: 660, start: t + 0.05, duration: 0.3, gain: 0.2, attack: 0.1 });
  },

  dailyDouble(ctx, out, t) {
    // Three wobbling laser zaps, each a little higher than the last
    [0, 1, 2].forEach((i) => {
      tone(ctx, out, {
        type: "sawtooth",
        freq: 1600 + i * 300,
        to: 180,
        start: t + i * 0.32,
        duration: 0.42,
        gain: 0.16,
        lowpass: 3500,
        vibrato: { rate: 28, depth: 90 },
      });
    });
    for (const freq of [D5, A5, D6]) {
      tone(ctx, out, { type: "triangle", freq, start: t + 1.0, duration: 0.8, gain: 0.12, release: 0.6 });
    }
  },

  buzzIn(ctx, out, t) {
    // A bell: a tone with quieter overtones, all ringing out
    tone(ctx, out, { freq: E6, start: t, duration: 0.9, gain: 0.55, attack: 0.002 });
    tone(ctx, out, { freq: E6 * 2, start: t, duration: 0.5, gain: 0.18, attack: 0.002 });
    tone(ctx, out, { freq: E6 * 3.01, start: t, duration: 0.25, gain: 0.09, attack: 0.002 });
  },

  correct(ctx, out, t) {
    [G5, B5, D6].forEach((freq, i) => {
      tone(ctx, out, { type: "triangle", freq, start: t + i * 0.07, duration: 0.2, gain: 0.22 });
    });
    tone(ctx, out, { type: "triangle", freq: G6, start: t + 0.21, duration: 0.6, gain: 0.22, release: 0.45 });
    tone(ctx, out, { freq: G6 * 2, start: t + 0.21, duration: 0.4, gain: 0.05 });
  },

  incorrect(ctx, out, t) {
    // Two detuned square waves beat against each other into a buzz
    for (const freq of [138, 146]) {
      tone(ctx, out, {
        type: "square",
        freq,
        start: t,
        duration: 0.45,
        gain: 0.14,
        lowpass: 900,
        release: 0.1,
      });
    }
  },

  timesUp(ctx, out, t) {
    [0, 1, 2].forEach((i) => {
      tone(ctx, out, {
        type: "square",
        freq: 466.16,
        start: t + i * 0.18,
        duration: 0.14,
        gain: 0.22,
        lowpass: 1400,
        release: 0.03,
      });
    });
  },

  finalJeopardy(ctx, out, t) {
    // A drum roll that swells into a big chord
    noise(ctx, out, { start: t, duration: 0.85, from: 120, to: 260, gain: 0.5, attack: 0.8, release: 0.05, q: 0.8 });
    for (const freq of [146.83, 220, 293.66, 369.99, 440]) {
      tone(ctx, out, {
        type: "sawtooth",
        freq,
        start: t + 0.85,
        duration: 1.6,
        gain: 0.07,
        lowpass: 1800,
        release: 1.2,
      });
    }
    noise(ctx, out, { start: t + 0.85, duration: 0.5, from: 4000, to: 3000, gain: 0.15, attack: 0.003 });
  },

  tick(ctx, out, t) {
    tone(ctx, out, { freq: 1900, start: t, duration: 0.05, gain: 0.55, attack: 0.001 });
    tone(ctx, out, { freq: 950, start: t, duration: 0.07, gain: 0.33, attack: 0.001 });
  },

  gameOver(ctx, out, t) {
    [G4, C5, E5].forEach((freq, i) => {
      tone(ctx, out, { type: "triangle", freq, start: t + i * 0.13, duration: 0.12, gain: 0.22, release: 0.04 });
      tone(ctx, out, { type: "square", freq, start: t + i * 0.13, duration: 0.12, gain: 0.05, lowpass: 2000, release: 0.04 });
    });
    for (const freq of [C5, E5, G5, C6]) {
      tone(ctx, out, { type: "triangle", freq, start: t + 0.39, duration: 1.4, gain: 0.13, release: 1 });
      tone(ctx, out, { type: "square", freq, start: t + 0.39, duration: 1.2, gain: 0.03, lowpass: 2000, release: 0.9 });
    }
  },

  buzz(ctx, out, t) {
    tone(ctx, out, { type: "square", freq: 300, to: 140, start: t, duration: 0.12, gain: 0.6, lowpass: 1500 });
    noise(ctx, out, { start: t, duration: 0.03, from: 3000, to: 2000, gain: 0.9, attack: 0.001 });
  },

  lockout(ctx, out, t) {
    tone(ctx, out, { freq: 180, to: 90, start: t, duration: 0.2, gain: 0.8 });
  },

  yourPick(ctx, out, t) {
    tone(ctx, out, { freq: A5, start: t, duration: 0.35, gain: 0.25, attack: 0.003 });
    tone(ctx, out, { freq: E6, start: t + 0.1, duration: 0.5, gain: 0.25, attack: 0.003 });
  },
};

/** Schedules a sound on any context, such as an OfflineAudioContext to render it */
export function renderSound(name: SoundName, ctx: BaseAudioContext, out: AudioNode, t = 0) {
  voices[name](ctx, out, t);
}

const MUTED_KEY = "sound_muted";

export function isMuted(): boolean {
  return localStorage.getItem(MUTED_KEY) === "1";
}

export function setMuted(muted: boolean) {
  if (muted) localStorage.setItem(MUTED_KEY, "1");
  else localStorage.removeItem(MUTED_KEY);
}

let ctx: AudioContext | null = null;
let output: AudioNode | null = null;

function context(): { ctx: AudioContext; output: AudioNode } | null {
  if (!ctx) {
    if (typeof AudioContext === "undefined") return null;
    ctx = new AudioContext();
    // Keeps sounds that overlap from clipping
    const compressor = ctx.createDynamicsCompressor();
    const master = ctx.createGain();
    master.gain.value = 0.8;
    master.connect(compressor).connect(ctx.destination);
    output = master;
  }
  if (ctx.state === "suspended") void ctx.resume();
  return { ctx, output: output! };
}

export function playSound(name: SoundName) {
  if (isMuted()) return;
  const audio = context();
  if (!audio) return;
  renderSound(name, audio.ctx, audio.output, audio.ctx.currentTime + 0.01);
}

// Mobile browsers only start audio from inside a tap, so start it on the first one
const unlock = () => {
  context();
  window.removeEventListener("pointerdown", unlock, true);
  window.removeEventListener("keydown", unlock, true);
};
window.addEventListener("pointerdown", unlock, true);
window.addEventListener("keydown", unlock, true);

/** Message the display sends the host window to play a sound */
export interface PlaySoundMessage {
  type: "playSound";
  sound: SoundName;
}

/**
 * Plays a sound for the room from the display window. The display is never clicked, so
 * the browser wouldn't let it make sound itself: the host window, which the host clicks,
 * plays it instead. Both windows are on the same machine, so the room hears it the same.
 */
export function playOnStage(name: SoundName) {
  const message: PlaySoundMessage = { type: "playSound", sound: name };
  window.opener?.postMessage(message, window.location.origin);
}
