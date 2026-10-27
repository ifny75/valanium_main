'use client';
import { useNetworkStatus } from './use-network-status';
import { NodeDetails } from './node-details';
const LABELS = { operational: 'Все системы работают', degraded: 'Частичное ухудшение', outage: 'Есть недоступные системы' } as const;
export function StatusDashboard() {
  const { data, failed, busy, refresh } = useNetworkStatus();
  const overall = failed ? 'unknown' : data?.overall ?? 'unknown';
  return <>
    <section className={`status-summary status-${overall}`}>
      <span className="status-pulse" />
      <div><small>Состояние сети сейчас</small><h2>{failed ? 'Не удалось обновить статус' : data ? LABELS[data.overall] : 'Проверяем системы…'}</h2></div>
      <button type="button" onClick={() => void refresh()} disabled={busy}><span>↻</span> Обновить</button>
    </section>
    <section className="status-board" id="network" aria-label="Узлы сети Valanium">
      <header className="status-board-head"><div><span>Valanium network</span><h2>Сервисы и узлы</h2></div><small><i /> Автообновление · 15 сек.</small></header>
      {data?.nodes.map(node => <article className={`status-service status-service-${failed ? 'unknown' : node.status}`} key={node.id}>
        <NodeDetails node={node} failed={failed} />
      </article>)}
      {!data && <p className="network-empty">{failed ? 'Не удалось получить состояние узлов' : 'Проверяем узлы…'}</p>}
    </section>
    <div className="status-note"><span>i</span><p>Нажмите на узел, чтобы посмотреть показатели. При потере связи данные помечаются как недоступные. IP-адреса и число пользователей не публикуются.</p>
      <time>{data ? `Обновлено ${new Date(data.checkedAt).toLocaleTimeString('ru-RU')}` : ''}</time></div>
  </>;
}
