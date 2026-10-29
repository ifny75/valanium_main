'use client';
import { useLanguage } from './language-provider';
import { useId } from 'react';
import { COUNTRIES, type PublicNode } from '../network-status';
const LABELS = { operational: 'Онлайн', degraded: 'Есть замечания', outage: 'Недоступен' };
export function NodeDetails({ node, failed }: { node: PublicNode; failed: boolean }) {
  const { language } = useLanguage();
  const locale = language === 'en' ? 'en-US' : 'ru-RU';
  const id = useId();
  const country = COUNTRIES[node.country];
  const metrics = failed ? null : node.metrics;
  const percent = (value?: number | null) => typeof value === 'number' ? `${value}%` : 'Нет данных';
  const uptime = metrics?.uptimeHours;
  const rate = (value?: number | null) => typeof value === 'number'
    ? `${value.toLocaleString(locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} Мбит/с` : 'Нет данных';
  return <details className="network-node-details network-server-details">
    <summary aria-controls={id}>
      <img className="node-country-flag" src={country.flag} alt={country.name} width="30" height="22" />
      <span className="network-node-name"><strong>{node.name}</strong><small>{country.name} · {node.role}</small></span>
      <em data-node-state={failed ? 'unknown' : node.status}>{failed ? 'Нет данных' : LABELS[node.status]}</em>
      <span className="node-expand" aria-hidden="true">⌄</span>
    </summary>
    <div className="node-detail-body" id={id}>
      <dl>
        <div><dt>Загрузка CPU</dt><dd>{percent(metrics?.cpuPercent)}</dd></div>
        <div><dt>Оперативная память</dt><dd>{percent(metrics?.memoryPercent)}</dd></div>
        <div><dt>↓ Входящий трафик</dt><dd>{rate(metrics?.rxMbps)}</dd></div>
        <div><dt>↑ Исходящий трафик</dt><dd>{rate(metrics?.txMbps)}</dd></div>
        <div><dt>Время работы</dt><dd>{typeof uptime === 'number' ? language === 'en' ? `${Math.floor(uptime / 24)} d ${uptime % 24} h` : `${Math.floor(uptime / 24)} д. ${uptime % 24} ч.` : 'Нет данных'}</dd></div>
        <div><dt>Последняя проверка</dt><dd>{!failed && node.checkedAt ? new Date(node.checkedAt).toLocaleTimeString(locale) : 'Нет свежих данных'}</dd></div>
      </dl>
      <p>Трафик — средняя скорость за 1 секунду на внешнем сетевом интерфейсе, округлённая до 0,01 Мбит/с. Обновление каждые 15–30 секунд. Внутренние туннели повторно не учитываются.</p>
    </div>
  </details>;
}
