'use client';
import { useRef, useState } from 'react';

export const ONION_URL = 'http://5kghvwyxzmtzba4foenmg5pkhcoxv6iq2c6wf4pbg5uyjrviwkckvead.onion/';
export function OnionButton({ prominent = false }: { prominent?: boolean }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [copied, setCopied] = useState(false);
  return <>
    <button type="button" className={`onion-button${prominent ? ' onion-button-prominent' : ''}`}
      onClick={() => { setCopied(false); dialog.current?.showModal(); }} aria-haspopup="dialog">
      Onion
    </button>
    <dialog ref={dialog} className="onion-dialog" aria-labelledby={prominent ? 'onion-hero-title' : 'onion-nav-title'}
      onClick={event => { if (event.target === dialog.current) dialog.current.close(); }}>
      <div><form method="dialog"><button className="onion-close" aria-label="Закрыть" type="submit">×</button></form>
        <span className="section-eyebrow">Прямой Onion-доступ</span>
        <h2 id={prominent ? 'onion-hero-title' : 'onion-nav-title'}>Valanium в сети Tor</h2>
        <p>Откройте этот адрес в Tor Browser. Обычному браузеру для доступа к .onion требуется подключение к Tor.</p>
        <a className="onion-address" href={ONION_URL} target="_blank" rel="noreferrer noopener">{ONION_URL}</a>
        <button type="button" className="onion-button onion-button-prominent" onClick={async () => {
          try { await navigator.clipboard.writeText(ONION_URL); setCopied(true); }
          catch { setCopied(false); }
        }}>{copied ? 'Адрес скопирован ✓' : 'Скопировать Onion-адрес'}</button>
        <a className="onion-open" href={ONION_URL} target="_blank" rel="noreferrer noopener">Открыть Onion-сайт ↗</a>
        <p className="onion-footnote" role="status">{copied ? 'Вставьте адрес в Tor Browser.' : 'В приложении Onion уже выбран по умолчанию. Этот адрес открывает сайт.'}</p>
      </div>
    </dialog>
  </>;
}
