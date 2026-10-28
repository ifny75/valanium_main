'use client';

import { useEffect, useState } from 'react';
import { GithubIcon } from './icons';
import { SupportDialog } from './support-dialog';
import { SERVICES } from '../services';
import { OnionButton } from './onion-button';

/**
 * Одинаковая навигация на всех страницах; текущий раздел меняет только подсветку.
 */
export type HeaderPage = 'hub' | 'messenger' | 'mail' | 'vpn' | 'status' | 'apps';

type Link = { href: string; label: string; logo?: string };

const SERVICE_LINKS: Link[] = SERVICES.map((service) => ({
  href: service.href,
  label: service.short,
  logo: service.logo,
}));

export function DynamicHeader({ page = 'hub' }: { page?: HeaderPage }) {
  const [compact, setCompact] = useState(false);

  useEffect(() => {
    let frame = 0;

    const update = () => {
      setCompact(previous => window.scrollY > 88 ? true : window.scrollY < 40 ? false : previous);
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

  const brandHref = '/';

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
        <a className="brand" href={brandHref} aria-label="Valanium">
          <img src="/logos/brand.svg" alt="" />
          <b>Valanium</b>
        </a>

        <div className="nav-menu">
          {SERVICE_LINKS.map((link) => (
            <a
              key={link.href}
              href={link.href}
              className={link.href === `/${page}` ? 'is-active' : undefined}
              aria-current={link.href === `/${page}` ? 'page' : undefined}
            >
              {link.logo ? <img className="nav-logo" src={link.logo} alt="" /> : null}
              {link.label}
            </a>
          ))}
          <a className={page === 'apps' ? 'is-active' : undefined} href="/apps">Приложения</a>
          <a className={`nav-status-link${page === 'status' ? ' is-active' : ''}`} href="/status">Статус</a>
        </div>

        <div className="nav-actions">
          <OnionButton />
          <a className="status-mobile-link" href="/apps">Приложения</a>
          <SupportDialog variant="header" />
          <a className="github-link" href={page === 'vpn' ? 'https://github.com/ifny75/Valanium_Vpn' : 'https://github.com/ifny75/valanium_main'} target="_blank" rel="noreferrer noopener">
            <GithubIcon /><b>GitHub</b>
          </a>
        </div>
      </nav>
    </header>
  );
}
