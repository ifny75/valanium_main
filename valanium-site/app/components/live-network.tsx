'use client';
import { useNetworkStatus } from './use-network-status';
import { NodeDetails } from './node-details';
export function LiveNetwork() {
  const { data, failed, busy, refresh } = useNetworkStatus();
  return <section className="network-minimal shell" id="network" aria-labelledby="network-title">
    <header className="network-minimal-header">
      <h2 id="network-title">The Valanium Network</h2>
      <button type="button" onClick={() => void refresh()} disabled={busy}>{busy ? 'Обновляем…' : 'Обновить'}</button>
    </header>
    {failed && data && <p className="network-minimal-message" role="status">Нет свежих данных. Проверяем связь…</p>}
    <ol className="network-minimal-servers" aria-label="Серверы Valanium" aria-busy={busy}>
      {data?.servers.map(node => <li key={node.id}>
        <NodeDetails node={node} failed={failed} />
      </li>)}
    </ol>
    {!data && <p className="network-minimal-message" role="status">{failed ? 'Статус временно недоступен' : 'Получаем состояние серверов…'}</p>}
    <a className="network-minimal-status" href="/status">Подробный статус и службы</a>
  </section>;
}
