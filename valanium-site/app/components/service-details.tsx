'use client';
import { useId } from 'react';
import type { PublicService } from '../network-status';
const LABELS = { operational: 'Работает', degraded: 'Есть замечания', outage: 'Недоступна' };
export function ServiceDetails({ service, failed }: { service: PublicService; failed: boolean }) {
  const id = useId();
  return <details className="network-node-details network-service-details">
    <summary aria-controls={id}>
      <span className="service-symbol" data-node-state={failed ? 'unknown' : service.status} aria-hidden="true">●</span>
      <span className="network-node-name"><strong>{service.name}</strong></span>
      <em data-node-state={failed ? 'unknown' : service.status}>{failed ? 'Нет данных' : LABELS[service.status]}</em>
      <span className="node-expand" aria-hidden="true">⌄</span>
    </summary>
    <div className="node-detail-body" id={id}>
      <p className="service-description">{service.role}</p>
      <dl><div><dt>Последняя проверка службы</dt><dd>{!failed && service.checkedAt ? new Date(service.checkedAt).toLocaleString('ru-RU') : 'Нет свежих данных'}</dd></div>
        <div><dt>Время ответа проверки</dt><dd>{!failed && service.latency > 0 ? `${service.latency} мс` : 'Не измеряется'}</dd></div></dl>
      <p>Статус относится к этой службе. Нагрузка оборудования показана отдельно в разделе серверов.</p>
    </div>
  </details>;
}
