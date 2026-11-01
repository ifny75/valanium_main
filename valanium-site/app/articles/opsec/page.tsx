/* eslint-disable @next/next/no-html-link-for-pages */
import type { Metadata } from 'next';
import type { ReactNode } from 'react';
import { DynamicHeader } from '../../components/dynamic-header';
import { ButtonArrow } from '../../components/animated-heading';
import sections from './content.json';
import './opsec.css';

const pdf = '/guides/opsec-internet-security-v2.pdf';
const title = 'OPSEC и безопасность в интернете';
export const metadata: Metadata = {
  title,
  description: 'Практическое руководство: защита аккаунтов, цифровой след, Tor, метаданные, операционные системы и почта. Полная статья и PDF для скачивания.',
  alternates: { canonical: 'https://valanium.com/articles/opsec' },
  openGraph: { title: `${title} — Valanium`, type: 'article', locale: 'ru_RU', url: 'https://valanium.com/articles/opsec' },
};

// Keep the supplied document in its original language; translate only the site chrome.
function linked(text: string): ReactNode {
  return text.split(/(https?:\/\/[^\s]+)/g).map((part, i) => /^https?:\/\//.test(part)
    ? <a key={i} href={part} target="_blank" rel="noopener noreferrer">{part}</a> : part);
}

export default function OpsecArticle() {
  return <main className="opsec-page" id="top">
    <DynamicHeader />
    <header className="opsec-hero shell">
      <a className="opsec-back" href="/">← На главную</a>
      <div className="opsec-label"><span /> Библиотека Valanium</div>
      <div data-no-translate lang="ru">
        <h1><em>OPSEC</em><br />и безопасность<br />в интернете</h1>
        <p className="opsec-lead">Как уменьшить цифровой след, защитить аккаунты и выбирать инструменты под свой риск.</p>
      </div>
      <div className="opsec-meta"><span>Практическое руководство</span><span>32 страницы</span><time dateTime="2026-11-01">01.11.2026</time><span>На русском</span></div>
      <div className="opsec-actions">
        <a className="stage-button stage-button-primary" href="#contents">Читать руководство <ButtonArrow /></a>
        <a className="stage-button" href={pdf} download="OPSEC_Internet_Security_Guide_v2.pdf">Скачать PDF ↓</a>
      </div>
      <p className="opsec-file-note">Оригинальный документ · PDF · 32 страницы</p>
      <div className="opsec-orbit" aria-hidden="true"><i /><i /><i /><span>МЕНЬШЕ<br />ЦИФРОВОГО<br /><em>СЛЕДА.</em></span></div>
    </header>

    <div className="opsec-layout shell">
      <aside className="opsec-sidebar" aria-label="Навигация по статье">
        <details open id="contents">
          <summary>Содержание <span>32</span></summary>
          <nav data-no-translate lang="ru">{sections.map((section, i) => <a href={`#${section.id}`} key={section.id}>
            <span>{String(i + 1).padStart(2, '0')}</span>{section.title}
          </a>)}</nav>
        </details>
        <a className="opsec-aside-download" href={pdf} download="OPSEC_Internet_Security_Guide_v2.pdf">Скачать руководство ↓</a>
      </aside>
      <article className="opsec-article" data-no-translate lang="ru">
        <div className="opsec-intro"><span>СНАЧАЛА — МОДЕЛЬ УГРОЗ</span><p>Защита строится на привычках, а не на одной программе.</p><small>Веб-версия предоставленного руководства от 01.11.2026, включая дополнение об операционных системах и почте. Содержание и ссылки сохранены; оригинальное оформление доступно в PDF.</small></div>
        {sections.map((section, i) => <section className="opsec-section" id={section.id} key={section.id}>
          {i === 20 ? <div className="opsec-part">ДОПОЛНЕНИЕ / ОПЕРАЦИОННЫЕ СИСТЕМЫ И ПОЧТА</div> : null}
          <div className="opsec-section-meta"><span>{String(i + 1).padStart(2, '0')}</span><a href={`${pdf}#page=${section.page}`} target="_blank" rel="noopener noreferrer">Страница {section.page} в PDF ↗</a></div>
          <h2>{section.title}</h2>
          {section.blocks.map((block, bi) => block.type === 'table'
            ? <div className="opsec-table-scroll" role="region" aria-label={`Таблица: ${section.title}`} tabIndex={0} key={bi}><table>
              <thead><tr>{block.rows?.[0].map((cell, ci) => <th scope="col" key={ci}>{linked(cell)}</th>)}</tr></thead>
              <tbody>{block.rows?.slice(1).map((row, ri) => <tr key={ri}>{row.map((cell, ci) => <td key={ci}>{linked(cell)}</td>)}</tr>)}</tbody>
            </table></div>
            : block.type === 'subheading' ? <h3 key={bi}>{block.text}</h3>
              : /^(?:exiftool |mat2 )/.test(block.text ?? '') ? <pre key={bi}><code>{block.text}</code></pre>
              : <p className={block.text?.startsWith('•') ? 'opsec-bullet' : undefined} key={bi}>{linked(block.text ?? '')}</p>)}
        </section>)}
        <div className="opsec-end"><h2>Сохраните под рукой.</h2><p>Полное руководство в исходном оформлении — для чтения без подключения к сети.</p><a className="stage-button stage-button-primary" href={pdf} download="OPSEC_Internet_Security_Guide_v2.pdf">Скачать PDF ↓</a></div>
      </article>
    </div>
    <footer className="site-footer shell"><span>© 2026 Valanium</span><nav aria-label="Служебные ссылки"><a href="/">На главную</a><a href="/privacy">Политика конфиденциальности</a><a href="#top">Наверх ↑</a></nav></footer>
  </main>;
}
