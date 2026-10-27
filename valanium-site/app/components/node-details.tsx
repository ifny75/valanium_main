'use client';
import { useId } from 'react';
import { COUNTRIES, type PublicNode } from '../network-status';
const LABELS = { operational: 'Онлайн', degraded: 'Есть замечания', outage: 'Недоступен' };
export function NodeDetails({ node, failed }: { node: PublicNode; failed: boolean }) {
  const id = useId();
  const country = COUNTRIES[node.country];
  const metrics = failed ? null : node.metrics;
  const percent = (value?: number | null) => typeof value === 'number' ? `${value}%` : 'Нет данных';
  const uptime = metrics?.uptimeHours;
  return <details className="network-node-details">
    <summary aria-controls={id}>
      <img className="node-country-flag" src={country.flag} alt={country.name} width="30" height="22" />
      <span className="network-node-name"><strong>{node.name}</strong><small>{country.name} · {node.role}</small></span>
      <span className="node-latency">{!failed && node.status === 'operational' && node.latency > 0 ? `${node.latency} мс` : '—'}</span>
      <em data-node-state={failed ? 'unknown' : node.status}>{failed ? 'Нет данных' : LABELS[node.status]}</em>
      <span className="node-expand" aria-hidden="true">⌄</span>
    </summary>
    <div className="node-detail-body" id={id}>
      <dl>
        <div><dt>Загрузка CPU</dt><dd>{percent(metrics?.cpuPercent)}</dd></div>
        <div><dt>Оперативная память</dt><dd>{percent(metrics?.memoryPercent)}</dd></div>
        <div><dt>Время работы</dt><dd>{typeof uptime === 'number' ? `${Math.floor(uptime / 24)} д. ${uptime % 24} ч.` : 'Нет данных'}</dd></div>
        <div><dt>Последняя проверка</dt><dd>{!failed && node.checkedAt ? new Date(node.checkedAt).toLocaleTimeString('ru-RU') : 'Нет свежих данных'}</dd></div>
      </dl>
      <p>Показатели CPU и памяти округлены. Задержка измерена инфраструктурой, это не пинг от вашего устройства.</p>
    </div>
  </details>;
}
