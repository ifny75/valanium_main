'use client';
import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import english from '../locales/en.json';

type Language = 'ru' | 'en';
const dictionary: Record<string, string> = english;
const normalize = (value: string) => value.replace(/\s+/g, ' ').trim();
const escapeRegex = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const templates = Object.entries(dictionary).filter(([key]) => /\{\d+\}/.test(key)).map(([key, value]) => {
  const slots: string[] = [];
  const pattern = key.split(/(\{\d+\})/).map(part => /^\{\d+\}$/.test(part) ? (slots.push(part), '(.+?)') : escapeRegex(part)).join('');
  return { regex: new RegExp(`^${pattern}$`), value, slots };
});
export function translate(value: string, language: Language): string {
  if (language === 'ru') return value;
  const key = normalize(value);
  let result = dictionary[key];
  if (!result) for (const template of templates) {
    const match = key.match(template.regex);
    if (!match) continue;
    result = template.value;
    template.slots.forEach((slot, i) => { result = result.replaceAll(slot, match[i + 1]); });
    break;
  }
  return result ? value.match(/^\s*/)?.[0] + result + value.match(/\s*$/)?.[0] : value;
}
const LanguageContext = createContext<{ language: Language; setLanguage: (language: Language) => void }>({ language: 'ru', setLanguage() {} });
export const useLanguage = () => useContext(LanguageContext);
export const displayLocale = () => typeof document !== 'undefined' && document.documentElement.lang === 'en' ? 'en-US' : 'ru-RU';

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<Language>('ru');
  useEffect(() => {
    let saved: string | null = null;
    try { saved = localStorage.getItem('valanium.language'); } catch {}
    setLanguageState(saved === 'ru' || saved === 'en' ? saved : navigator.language.toLowerCase().startsWith('ru') ? 'ru' : 'en');
  }, []);
  const setLanguage = (value: Language) => {
    try { localStorage.setItem('valanium.language', value); } catch {}
    document.cookie = `valanium_language=${value};path=/;max-age=31536000;SameSite=Lax`;
    setLanguageState(value);
  };
  useEffect(() => {
    document.documentElement.lang = language;
    const texts = new WeakMap<Node, { source: string; rendered: string }>();
    const attributes = new WeakMap<Element, Map<string, { source: string; rendered: string }>>();
    const excluded = 'script,style,textarea,input,code,pre,[data-no-translate],.animated-heading';
    // Keep original UI text across language changes; never touch form values or marked user content.
    const records = (window as Window & { __valaniumTextRecords?: typeof texts; __valaniumAttrRecords?: typeof attributes });
    const textRecords = records.__valaniumTextRecords ??= texts;
    const attributeRecords = records.__valaniumAttrRecords ??= attributes;
    function update() {
      const walker = document.createTreeWalker(document.documentElement, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const parent = node.parentElement;
        if (!parent || parent.closest(excluded)) continue;
        const current = node.nodeValue ?? '';
        let record = textRecords.get(node);
        if (!record || current !== record.rendered) record = { source: current, rendered: current };
        const next = translate(record.source, language);
        record.rendered = next; textRecords.set(node, record);
        if (current !== next) node.nodeValue = next;
      }
      document.querySelectorAll('[title],[placeholder],[aria-label],img[alt]').forEach(element => {
        if (element.closest('[data-no-translate],.animated-heading')) return;
        const values = attributeRecords.get(element) ?? new Map();
        for (const name of ['title', 'placeholder', 'aria-label', 'alt']) {
          const current = element.getAttribute(name);
          if (current === null) continue;
          let record = values.get(name);
          if (!record || current !== record.rendered) record = { source: current, rendered: current };
          const next = translate(record.source, language);
          record.rendered = next; values.set(name, record);
          if (current !== next) element.setAttribute(name, next);
        }
        attributeRecords.set(element, values);
      });
    }
    let frame = 0;
    const observer = new MutationObserver(() => {
      if (!frame) frame = requestAnimationFrame(() => { frame = 0; update(); });
    });
    update();
    observer.observe(document.documentElement, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ['title', 'placeholder', 'aria-label', 'alt'] });
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  }, [language]);
  return <LanguageContext.Provider value={{ language, setLanguage }}>{children}</LanguageContext.Provider>;
}

export function LanguageSwitcher() {
  const { language, setLanguage } = useLanguage();
  return <select data-no-translate className="language-switcher" aria-label={language === 'en' ? 'Interface language' : 'Язык интерфейса'}
    value={language} onChange={event => setLanguage(event.target.value as Language)}>
    <option value="ru">RU</option><option value="en">EN</option>
  </select>;
}
