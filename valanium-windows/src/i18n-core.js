export function createTranslator(english, getLanguage) {
  const normalize = value => value.replace(/\s+/g, ' ').trim();
  function tr(value) {
    if (getLanguage() === 'ru') return value;
    const result = english[normalize(value)];
    if (result) return value.match(/^\s*/)[0] + result + value.match(/\s*$/)[0];
    if (/<\/?[a-z]/i.test(value)) return value
      .replace(/(^|>)([^<>]+)(?=<|$)/g, (_, prefix, text) => prefix + tr(text))
      .replace(/(title|placeholder|aria-label)="([^"]+)"/g, (_, name, text) => `${name}="${tr(text).replaceAll('"', '&quot;')}"`);
    return value;
  }
  function trTemplate(strings, ...values) {
    const key = strings.reduce((out, part, i) => out + part + (i < values.length ? `{${i}}` : ''), '');
    return tr(key).replace(/\{(\d+)\}/g, (token, index) => index < values.length ? String(values[index]) : token);
  }
  return { tr, trTemplate };
}
