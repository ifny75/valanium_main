'use client';

import { useEffect, useState } from 'react';
import { GithubIcon } from './icons';
import { SupportDialog } from './support-dialog';

export function DynamicHeader({ page = 'main' }: { page?: 'main' | 'status' }) {
  const [compact, setCompact] = useState(false);

  useEffect(() => {
    let frame = 0;

    const update = () => {
      setCompact(window.scrollY > 42);
      frame = 0;
    };

    const onScroll = () => {
      if (!frame) frame = window.requestAnimationFrame(update);
    };

    update();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      window.removeEventListener('scroll', onScroll);
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, []);

  return (
    <header className={`site-header${compact ? ' is-compact' : ''}`}>
      <nav
        className="nav"
        aria-label="Основная навигация"
        style={{
          backdropFilter: compact ? 'blur(28px) saturate(165%)' : 'blur(14px) saturate(135%)',
          WebkitBackdropFilter: compact ? 'blur(28px) saturate(165%)' : 'blur(14px) saturate(135%)',
        }}
      >
        <a className="brand" href={page === 'status' ? '/' : '#top'} aria-label={page === 'status' ? 'Valanium — на главную' : 'Valanium — к началу страницы'}>
          <img src="/valanium.svg" alt="" />
          <b>Valanium</b>
          <span>Public Beta</span>
        </a>

        <div className="nav-menu">
          <a href={page === 'status' ? '/#features' : '#features'}>Возможности</a>
          <a href={page === 'status' ? '/#routes' : '#routes'}>Маршруты</a>
          <a href={page === 'status' ? '/#download' : '#download'}>Скачать</a>
          <a className={`nav-status-link${page === 'status' ? ' is-active' : ''}`} href="/status"><i />Статус</a>
        </div>

        <div className="nav-actions">
          <a className="status-mobile-link" href="/status" aria-label="Статус сети"><i /></a>
          <SupportDialog variant="header" />
          <a className="github-link" href="https://github.com/ifny75/valanium" target="_blank" rel="noreferrer noopener">
            <GithubIcon /><b>GitHub</b>
          </a>
        </div>
      </nav>
    </header>
  );
}
