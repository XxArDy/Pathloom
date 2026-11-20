import { useEffect, useRef, useState } from 'react';
import i18n from './i18n';

/** Minimal shape of the Web Speech API recognizer (not in the TS DOM lib). */
interface Recognizer {
  lang: string;
  interimResults: boolean;
  continuous: boolean;
  onresult: ((e: { resultIndex: number; results: ArrayLike<ArrayLike<{ transcript: string }> & { isFinal: boolean }> }) => void) | null;
  onerror: ((e: { error: string }) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
}

type RecognizerCtor = new () => Recognizer;

const ctor = (): RecognizerCtor | undefined => {
  const w = window as unknown as { SpeechRecognition?: RecognizerCtor; webkitSpeechRecognition?: RecognizerCtor };
  return w.SpeechRecognition ?? w.webkitSpeechRecognition;
};

/** BCP 47 tag for the app language. */
export const speechLang = (lang: string) => (lang.startsWith('uk') ? 'uk-UA' : 'en-US');

/**
 * Voice dictation into a text field. Uses the browser speech recognizer when the WebView has one;
 * otherwise (or when it fails, e.g. WebView2 without a speech service) `fallback` turns on and the
 * caller shows the OS dictation hint (Win+H, Fn Fn).
 */
export function useDictation(onText: (text: string) => void) {
  const [listening, setListening] = useState(false);
  const [fallback, setFallback] = useState(false);
  const rec = useRef<Recognizer | null>(null);
  const cb = useRef(onText);
  cb.current = onText;
  useEffect(() => () => rec.current?.stop(), []);

  const toggle = () => {
    if (listening) { rec.current?.stop(); return; }
    const C = ctor();
    if (!C) { setFallback(true); return; }
    try {
      const r = new C();
      r.lang = speechLang(i18n.language);
      r.interimResults = false;
      r.continuous = true;
      r.onresult = (e) => {
        for (let i = e.resultIndex; i < e.results.length; i++) {
          if (e.results[i].isFinal) cb.current(e.results[i][0].transcript.trim());
        }
      };
      r.onerror = (e) => { if (e.error !== 'aborted' && e.error !== 'no-speech') setFallback(true); };
      r.onend = () => setListening(false);
      r.start();
      rec.current = r;
      setListening(true);
      setFallback(false);
    } catch {
      setFallback(true);
    }
  };
  return { listening, fallback, toggle, dismiss: () => setFallback(false) };
}

/** Appends dictated text to what is already typed, with one space between. */
export const appendText = (cur: string, add: string) => (!add ? cur : cur.trim() ? `${cur.trimEnd()} ${add}` : add);
