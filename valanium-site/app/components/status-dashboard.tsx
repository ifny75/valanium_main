'use client';
import { useNetworkStatus } from './use-network-status';
import { NodeDetails } from './node-details';
import { ServiceDetails } from './service-details';
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
    <section className="status-board" id="network" aria-label="Серверы Valanium">
      <header className="status-board-head"><div><span>Physical infrastructure · 03</span><h2>Серверы</h2></div><small><i /> Автообновление · 15 сек.</small></header>
      <p className="status-group-description">Три физических сервера. Страна, нагрузка, память и время работы оборудования.</p>
      {data?.servers.map(node => <article className={`status-service status-service-${failed ? 'unknown' : node.status}`} key={node.id}>
        <NodeDetails node={node} failed={failed} />
      </article>)}
      {!data && <p className="network-empty">{failed ? 'Не удалось получить состояние узлов' : 'Проверяем узлы…'}</p>}
    </section>
    <section className="status-board" aria-label="Службы Valanium">
      <header className="status-board-head"><div><span>Service health</span><h2>Службы</h2></div><small>{data ? `${data.services.length} служб` : 'Проверяем…'}</small></header>
      <p className="status-group-description">Доставка сообщений и сохранность данных. Подробности проверки — по нажатию.</p>
      <div className="compact-services">{data?.services.map(service => <article className={`status-service status-service-${failed ? 'unknown' : service.status}`} key={service.id}>
        <ServiceDetails service={service} failed={failed} />
      </article>)}</div>
      {!data && <p className="network-empty">{failed ? 'Не удалось получить состояние служб' : 'Проверяем службы…'}</p>}
    </section>
    <div className="status-note"><span>i</span><p>Нажмите на сервер или службу для подробностей. При потере связи данные помечаются как недоступные. IP-адреса и число пользователей не публикуются.</p>
      <time>{data ? `Обновлено ${new Date(data.checkedAt).toLocaleTimeString('ru-RU')}` : ''}</time></div>
  </>;
}
