import english from './locales/en.js';
import { createTranslator } from './i18n-core.js';

const KEY = 'valanium.onboarding.language';
export function getLanguage() {
  const stored = localStorage.getItem(KEY);
  return stored === 'ru' || stored === 'en' ? stored : navigator.language.toLowerCase().startsWith('ru') ? 'ru' : 'en';
}
export const getLocale = () => getLanguage() === 'en' ? 'en-US' : 'ru-RU';
export const { tr, trTemplate } = createTranslator(english, getLanguage);
export function setLanguage(language) {
  if (!['ru', 'en'].includes(language)) return;
  localStorage.setItem(KEY, language);
  location.reload();
}

// Capture static chrome before the core can populate conversations or recovery fields.
// Do not observe the message list: user text and seed words are never translated.
document.documentElement.lang = getLanguage();
const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
for (let node = walker.nextNode(); node; node = walker.nextNode()) {
  if (node.parentElement?.closest('script,style,textarea,code,pre,[data-no-translate]')) continue;
  if (/[А-Яа-яЁё]/.test(node.nodeValue)) node.nodeValue = tr(node.nodeValue);
}
document.querySelectorAll('[title],[placeholder],[aria-label],img[alt]').forEach(element => {
  for (const name of ['title', 'placeholder', 'aria-label', 'alt']) {
    const value = element.getAttribute(name);
    if (value) element.setAttribute(name, tr(value));
  }
});
const choice = document.getElementById('ui-language');
if (choice) {
  choice.value = getLanguage();
  choice.addEventListener('change', () => setLanguage(choice.value));
}
