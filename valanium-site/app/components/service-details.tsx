'use client';
import { useId } from 'react';
import type { PublicService } from '../network-status';
const LABELS = { operational: 'Работает', degraded: 'Есть замечания', outage: 'Недоступна' };
export function ServiceDetails({ service, failed }: { service: PublicService; failed: boolean }) {
  const id = useId();
  return <details className="network-node-details network-service-details">
    <summary aria-controls={id}>
      <span className="service-icon" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
        {service.id === 'data' ? <><path d="M12 3 4 6v6c0 4 5 7 8 9 3-2 8-5 8-9V6l-8-3Z" /><path d="m8.5 12 2.3 2.3 4.7-5" /></>
          : <><path d="M20 11.5a8 8 0 0 1-8 8H4l1.6-4A8 8 0 1 1 20 11.5Z" /><path d="M8 10h8M8 13h5" /></>}
      </svg></span>
      <span className="service-copy"><strong>{service.name}</strong><small>{service.id === 'data' ? 'Защита данных' : 'Доставка сообщений'}</small>
        <em data-node-state={failed ? 'unknown' : service.status}><i aria-hidden="true" />{failed ? 'Нет данных' : LABELS[service.status]}</em>
      </span>
      <span className="node-expand" aria-hidden="true">⌄</span>
    </summary>
    <div className="node-detail-body" id={id}>
      <p className="service-description">{service.role}</p>
      <dl><div><dt>Последняя проверка службы</dt><dd>{!failed && service.checkedAt ? new Date(service.checkedAt).toLocaleString('ru-RU') : 'Нет свежих данных'}</dd></div>
        <div><dt>Время ответа проверки</dt><dd>{!failed && service.latency > 0 ? `${service.latency} мс` : 'Не измеряется'}</dd></div></dl>
    </div>
  </details>;
}
